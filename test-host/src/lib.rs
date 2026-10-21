// SPDX-License-Identifier: MPL-2.0
//! A small stand-in for Syrinx's plugin host, so contract tests can load the built package.
//!
//! It follows note 10's host rules (package and manifest checks, grants enforced
//! per call, no re-entry, epoch deadlines, a memory cap, a validated bridge) but
//! it is not Syrinx: the real host is built in Syrinx-app #28 and #29.

mod bridge;
mod checks;
mod state;

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;
use std::{fmt, fs};

use plugin_format::{Manifest, ManifestError, PackageError, PackageList};
use wasmtime::component::{Component, HasSelf, Linker};
use wasmtime::{Config, Engine, Store, StoreLimitsBuilder};

pub use bridge::{BridgeError, Incoming, MAX_PENDING_PER_VIEW};
pub use state::{MAX_MESSAGE_BYTES, SYNTHETIC_NOW, ViewRecord};

pub use bindings::exports::syrinx::plugin::forms::{FieldKind, FieldValue, FormSpec, SubmitResult};
use bindings::exports::syrinx::plugin::lifecycle::Activation;
use bindings::exports::syrinx::plugin::views::ViewRequest;
pub use bindings::syrinx::plugin::host::LogLevel;
pub use bindings::syrinx::plugin::types::{Outcome, PluginError};
use bindings::{LinkOptions, Plugin};
use state::HostState;

/// Host-side bindings generated from the same wit/ as the plugin's.
mod bindings {
    wasmtime::component::bindgen!({ path: "../wit", world: "plugin" });
}

/// The version this stand-in reports to `lifecycle.activate`.
pub const HOST_VERSION: &str = "0.0.0-test-host";

/// Each linear memory may grow to at most this; the real figure comes from #28.
const MEMORY_LIMIT_BYTES: usize = 32 * 1024 * 1024;

/// One epoch tick, and the ticks a single export call may run for.
const EPOCH_TICK: Duration = Duration::from_millis(10);
const CALL_DEADLINE_TICKS: u64 = 200;

/// Why the host refused or stopped a plugin.
#[derive(Debug)]
pub enum HostError {
    Package(PackageError),
    Manifest(ManifestError),
    Component(String),
    /// The plugin trapped; this instance is discarded and refuses further calls.
    Trap(String),
    Failed,
    Refused(String),
    Bridge(BridgeError),
}

impl fmt::Display for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Package(error) => write!(f, "package refused: {error}"),
            Self::Manifest(error) => write!(f, "manifest refused: {error}"),
            Self::Component(detail) => write!(f, "component refused: {detail}"),
            Self::Trap(detail) => write!(f, "the plugin trapped: {detail}"),
            Self::Failed => write!(f, "the plugin trapped earlier and was discarded"),
            Self::Refused(detail) => write!(f, "refused: {detail}"),
            Self::Bridge(error) => write!(f, "view message refused: {error}"),
        }
    }
}

impl std::error::Error for HostError {}

/// Advances the engine's epoch while the host is alive, so a runaway call traps.
/// (Syrinx itself should tick only while a call is running, to keep idle CPU at zero.)
struct EpochTicker {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl EpochTicker {
    fn start(engine: Engine) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let stop_flag = Arc::clone(&stop);
        let thread = thread::spawn(move || {
            while !stop_flag.load(Ordering::Relaxed) {
                thread::sleep(EPOCH_TICK);
                engine.increment_epoch();
            }
        });
        Self {
            stop,
            thread: Some(thread),
        }
    }
}

impl Drop for EpochTicker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let Some(thread) = self.thread.take() else {
            return;
        };
        if thread.join().is_err() {
            eprintln!("the epoch ticker thread panicked");
        }
    }
}

/// One loaded plugin: its package, its store and its instance.
pub struct TestHost {
    package: PackageList,
    plugin: Plugin,
    store: Store<HostState>,
    failed: bool,
    export_calls: u64,
    /// Interface-chosen request ids waiting for a reply, per view.
    pending: BTreeMap<u64, Vec<u64>>,
    /// Views whose `views.opened` the plugin refused, with its error.
    refused_views: Vec<(u64, PluginError)>,
    next_request: u64,
    _ticker: EpochTicker,
}

impl TestHost {
    /// Verify a package folder, check its manifest and component, and instantiate it.
    pub fn load_package(dir: &Path) -> Result<Self, HostError> {
        let package = read_package(dir)?;
        let manifest_text = read_text(dir, "syrinx-plugin.toml")?;
        Self::load(dir, package, &manifest_text)
    }

    /// As `load_package`, but with a different manifest text, to test grants
    /// a manifest variant requests. The package's own files are still verified.
    pub fn load_package_with_manifest(dir: &Path, manifest_text: &str) -> Result<Self, HostError> {
        let package = read_package(dir)?;
        Self::load(dir, package, manifest_text)
    }

    fn load(dir: &Path, package: PackageList, manifest_text: &str) -> Result<Self, HostError> {
        let manifest = Manifest::parse(manifest_text).map_err(HostError::Manifest)?;
        checks::package_matches_manifest(&package, &manifest)?;
        let engine = engine()?;
        let module = fs::read(dir.join(&manifest.entry.module))
            .map_err(|error| HostError::Component(error.to_string()))?;
        checks::refuse_precompiled(&module)?;
        let component = Component::new(&engine, &module)
            .map_err(|error| HostError::Component(format!("{error:#}")))?;
        checks::component_matches_manifest(&engine, &component, &manifest)?;
        let (plugin, store) = instantiate(&engine, &component, manifest)?;
        Ok(Self {
            package,
            plugin,
            store,
            failed: false,
            export_calls: 0,
            pending: BTreeMap::new(),
            refused_views: Vec::new(),
            next_request: 1,
            _ticker: EpochTicker::start(engine),
        })
    }

    /// Record the owner's grant. Only a capability the manifest requests can be granted.
    pub fn grant(&mut self, kind: &str) -> Result<(), HostError> {
        if !self.store.data().manifest.requests(kind) {
            return Err(HostError::Refused(format!(
                "the manifest does not request {kind}"
            )));
        }
        let state = self.store.data_mut();
        state.revoked.remove(kind);
        state.granted.insert(kind.to_owned());
        Ok(())
    }

    /// The owner withdrew a grant; later calls get `revoked`.
    pub fn revoke(&mut self, kind: &str) {
        let state = self.store.data_mut();
        if state.granted.remove(kind) {
            state.revoked.insert(kind.to_owned());
        }
    }

    /// Views the plugin refused to open, with the error it gave.
    pub fn refused_views(&self) -> &[(u64, PluginError)] {
        &self.refused_views
    }

    pub fn package(&self) -> &PackageList {
        &self.package
    }

    /// How many export calls reached the module, to prove a refusal cost none.
    pub fn export_calls(&self) -> u64 {
        self.export_calls
    }

    pub fn logs(&self) -> &[(LogLevel, String)] {
        &self.store.data().logs
    }

    pub fn view(&self, view: u64) -> Option<&ViewRecord> {
        self.store.data().views.get(&view)
    }

    pub fn open_views(&self) -> Vec<u64> {
        self.store.data().views.keys().copied().collect()
    }

    /// The origin a view of this plugin is served from.
    pub fn origin(&self) -> String {
        format!("https://syrinx-plugin.{}.invalid", self.package.id)
    }

    pub fn activate(&mut self) -> Result<Result<(), PluginError>, HostError> {
        let info = Activation {
            host_version: HOST_VERSION.into(),
            granted: self.store.data().granted.iter().cloned().collect(),
        };
        self.call(|plugin, store| plugin.syrinx_plugin_lifecycle().call_activate(store, &info))
    }

    pub fn deactivate(&mut self) -> Result<(), HostError> {
        self.call(|plugin, store| plugin.syrinx_plugin_lifecycle().call_deactivate(store))
    }

    /// Run a command; only commands the manifest declares reach the plugin.
    pub fn run_command(&mut self, id: &str) -> Result<Result<(), PluginError>, HostError> {
        self.require_command(id)?;
        let id = id.to_owned();
        self.call(|plugin, store| plugin.syrinx_plugin_commands().call_run(store, &id))
    }

    pub fn form_for(&mut self, id: &str) -> Result<Result<FormSpec, PluginError>, HostError> {
        self.require_form(id)?;
        let id = id.to_owned();
        self.call(|plugin, store| plugin.syrinx_plugin_forms().call_form_for(store, &id))
    }

    pub fn submit(
        &mut self,
        id: &str,
        submission_id: &str,
        values: &[(&str, &str)],
    ) -> Result<SubmitResult, HostError> {
        self.require_form(id)?;
        let values: Vec<FieldValue> = values
            .iter()
            .map(|(name, value)| FieldValue {
                name: (*name).into(),
                value: (*value).into(),
            })
            .collect();
        let (id, submission_id) = (id.to_owned(), submission_id.to_owned());
        self.call(|plugin, store| {
            plugin
                .syrinx_plugin_forms()
                .call_submit(store, &id, &submission_id, &values)
        })
    }

    /// The owner closed a tab: forget the view, then tell the plugin.
    pub fn close_view(&mut self, view: u64) -> Result<(), HostError> {
        if self.store.data_mut().views.remove(&view).is_none() {
            return Err(HostError::Refused(format!("no open view {view}")));
        }
        self.pending.remove(&view);
        self.call(|plugin, store| plugin.syrinx_plugin_views().call_closed(store, view))
    }

    /// The view's page was reloaded: a new generation, and nothing old is pending.
    pub fn reload_view(&mut self, view: u64) -> Result<u64, HostError> {
        let record = self
            .store
            .data_mut()
            .views
            .get_mut(&view)
            .ok_or_else(|| HostError::Refused(format!("no open view {view}")))?;
        record.generation += 1;
        let generation = record.generation;
        self.pending.remove(&view);
        Ok(generation)
    }

    /// One `chrome.webview.postMessage` from a view, checked before the plugin sees it.
    /// Returns the response envelope, or `None` for a cancel.
    pub fn view_message(
        &mut self,
        view: u64,
        origin: &str,
        generation: u64,
        raw: &str,
    ) -> Result<Option<String>, HostError> {
        let incoming = self.check_view_message(view, origin, generation, raw)?;
        let Incoming::Request { id, method, body } = incoming else {
            return Ok(None);
        };
        let request = ViewRequest {
            view,
            request_id: self.allocate_request(),
            method,
            json: body.to_string(),
        };
        self.pending.entry(view).or_default().push(id);
        let reply =
            self.call(|plugin, store| plugin.syrinx_plugin_views().call_message(store, &request));
        if let Some(waiting) = self.pending.get_mut(&view) {
            waiting.retain(|pending| *pending != id);
        }
        Ok(Some(bridge::response(id, reply_body(reply?))))
    }

    fn check_view_message(
        &self,
        view: u64,
        origin: &str,
        generation: u64,
        raw: &str,
    ) -> Result<Incoming, HostError> {
        let state = self.store.data();
        let record = state
            .views
            .get(&view)
            .ok_or(HostError::Bridge(BridgeError::StaleView))?;
        let methods = state
            .manifest
            .tab_type(&record.kind)
            .map(|tab| tab.ui_methods.as_slice())
            .unwrap_or_default();
        let expected_origin = self.origin();
        let sender = bridge::Sender {
            expected_origin: &expected_origin,
            origin,
            current_generation: record.generation,
            generation,
            declared_methods: methods,
            pending: self.pending.get(&view).map_or(&[], Vec::as_slice),
        };
        bridge::check(&sender, raw).map_err(HostError::Bridge)
    }

    /// Request ids are never reused, across views and across reloads.
    fn allocate_request(&mut self) -> u64 {
        let id = self.next_request;
        self.next_request += 1;
        id
    }

    fn require_command(&self, id: &str) -> Result<(), HostError> {
        if self.store.data().manifest.command(id).is_some() {
            return Ok(());
        }
        Err(HostError::Refused(format!("command {id} is not declared")))
    }

    fn require_form(&self, id: &str) -> Result<(), HostError> {
        let has_form = self
            .store
            .data()
            .manifest
            .command(id)
            .is_some_and(|command| command.form);
        if has_form {
            return Ok(());
        }
        Err(HostError::Refused(format!("command {id} has no form")))
    }

    /// One export call under a fresh deadline; a trap discards the instance.
    fn call<R>(
        &mut self,
        export: impl FnOnce(&Plugin, &mut Store<HostState>) -> wasmtime::Result<R>,
    ) -> Result<R, HostError> {
        let result = self.call_once(export)?;
        self.deliver_opened_views()?;
        Ok(result)
    }

    fn call_once<R>(
        &mut self,
        export: impl FnOnce(&Plugin, &mut Store<HostState>) -> wasmtime::Result<R>,
    ) -> Result<R, HostError> {
        if self.failed {
            return Err(HostError::Failed);
        }
        self.export_calls += 1;
        self.store.set_epoch_deadline(CALL_DEADLINE_TICKS);
        export(&self.plugin, &mut self.store).map_err(|trap| {
            self.failed = true;
            HostError::Trap(format!("{trap:#}"))
        })
    }

    /// Tell the plugin about views it opened, now that its call has returned
    /// (the host never calls into a plugin from inside one of its imports).
    fn deliver_opened_views(&mut self) -> Result<(), HostError> {
        while let Some(view) = self.store.data_mut().opens_to_deliver.pop_front() {
            let Some(record) = self.store.data().views.get(&view).cloned() else {
                continue;
            };
            let opened = self.call_once(|plugin, store| {
                plugin
                    .syrinx_plugin_views()
                    .call_opened(store, view, &record.kind, &record.address)
            })?;
            if let Err(error) = opened {
                self.store.data_mut().views.remove(&view);
                self.refused_views.push((view, error));
            }
        }
        Ok(())
    }
}

fn engine() -> Result<Engine, HostError> {
    let mut config = Config::new();
    config.epoch_interruption(true);
    Engine::new(&config).map_err(|error| HostError::Component(format!("{error:#}")))
}

/// Link WASI 0.2 with an empty context and the `syrinx:plugin` imports, then instantiate.
fn instantiate(
    engine: &Engine,
    component: &Component,
    manifest: Manifest,
) -> Result<(Plugin, Store<HostState>), HostError> {
    let component_error = |error: wasmtime::Error| HostError::Component(format!("{error:#}"));
    let mut linker: Linker<HostState> = Linker::new(engine);
    wasmtime_wasi::p2::add_to_linker_sync(&mut linker).map_err(component_error)?;
    // Unstable interfaces (home-widgets) are compiled in but linked only when
    // switched on here; the template switches none on.
    let options = LinkOptions::default();
    Plugin::add_to_linker::<_, HasSelf<_>>(&mut linker, &options, |state| state)
        .map_err(component_error)?;
    let limits = StoreLimitsBuilder::new()
        .memory_size(MEMORY_LIMIT_BYTES)
        .instances(16)
        .memories(4)
        .tables(8)
        .build();
    let mut store = Store::new(engine, HostState::new(manifest, limits));
    store.limiter(|state| &mut state.limits);
    store.set_epoch_deadline(CALL_DEADLINE_TICKS);
    let plugin = Plugin::instantiate(&mut store, component, &linker).map_err(component_error)?;
    Ok((plugin, store))
}

fn read_package(dir: &Path) -> Result<PackageList, HostError> {
    let text = read_text(dir, plugin_format::package::LIST_NAME)?;
    let package = PackageList::parse(&text).map_err(HostError::Package)?;
    package.verify_folder(dir).map_err(HostError::Package)?;
    Ok(package)
}

fn read_text(dir: &Path, name: &str) -> Result<String, HostError> {
    fs::read_to_string(dir.join(name))
        .map_err(|error| HostError::Package(PackageError::Io(format!("{name}: {error}"))))
}

/// Turn the plugin's reply into a response body: valid JSON, or a typed error.
fn reply_body(
    reply: Result<String, PluginError>,
) -> Result<serde_json::Value, (&'static str, String)> {
    let json = reply.map_err(error_parts)?;
    serde_json::from_str(&json)
        .map_err(|_| ("internal", "the plugin replied with invalid JSON".into()))
}

fn error_parts(error: PluginError) -> (&'static str, String) {
    match error {
        PluginError::InvalidInput(message) => ("invalid-input", message),
        PluginError::NotFound(message) => ("not-found", message),
        PluginError::Denied(message) => ("denied", message),
        PluginError::Revoked(message) => ("revoked", message),
        PluginError::Unavailable(message) => ("unavailable", message),
        PluginError::Internal(message) => ("internal", message),
    }
}
