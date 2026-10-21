// SPDX-License-Identifier: MPL-2.0
//! What the test host keeps per plugin, and its answers to the plugin's host calls.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use plugin_format::Manifest;
use wasmtime::StoreLimits;
use wasmtime::component::ResourceTable;
use wasmtime_wasi::{WasiCtx, WasiCtxView, WasiView};

use crate::bindings::syrinx::plugin::host::{self, LogLevel};
use crate::bindings::syrinx::plugin::types::{self, PluginError};

/// Bounds the host applies to values lifted from the plugin.
const MAX_LOG_BYTES: usize = 4 * 1024;
const MAX_LOG_LINES: usize = 256;
const MAX_SETTINGS_KEY_BYTES: usize = 64;
const MAX_SETTINGS_VALUE_BYTES: usize = 64 * 1024;
const MAX_TITLE_CHARS: usize = 128;
pub const MAX_MESSAGE_BYTES: usize = 1024 * 1024;

/// A fixed synthetic clock, so tests never depend on the real time.
pub const SYNTHETIC_NOW: i64 = 1_700_000_000_000;

/// One open plugin view, as the host's tab model would hold it.
#[derive(Debug, Clone)]
pub struct ViewRecord {
    pub kind: String,
    pub address: String,
    pub generation: u64,
    pub title: String,
    pub badge: Option<String>,
    /// Envelopes sent to the view's interface, oldest first.
    pub events: Vec<String>,
}

/// The store's data: WASI's empty context plus everything Syrinx tracks.
pub struct HostState {
    wasi: WasiCtx,
    table: ResourceTable,
    pub limits: StoreLimits,
    pub manifest: Manifest,
    pub granted: BTreeSet<String>,
    /// Kinds the owner granted and then withdrew; calls answer `revoked`.
    pub revoked: BTreeSet<String>,
    settings: BTreeMap<String, Vec<u8>>,
    pub logs: Vec<(LogLevel, String)>,
    pub views: BTreeMap<u64, ViewRecord>,
    /// Views opened during an export; `views.opened` is called after it returns.
    pub opens_to_deliver: VecDeque<u64>,
    pub changed_feeds: Vec<String>,
    pub changed_widgets: Vec<String>,
    pub cancelled: Vec<u64>,
    next_view: u64,
}

impl HostState {
    pub fn new(manifest: Manifest, limits: StoreLimits) -> Self {
        Self {
            // No preopened folders, no environment, no arguments, no network.
            wasi: WasiCtx::builder().build(),
            table: ResourceTable::new(),
            limits,
            manifest,
            granted: BTreeSet::new(),
            revoked: BTreeSet::new(),
            settings: BTreeMap::new(),
            logs: Vec::new(),
            views: BTreeMap::new(),
            opens_to_deliver: VecDeque::new(),
            changed_feeds: Vec::new(),
            changed_widgets: Vec::new(),
            cancelled: Vec::new(),
            next_view: 1,
        }
    }

    /// Fail unless the owner granted this capability: `revoked` if the owner
    /// withdrew it, `denied` if it was never granted.
    fn require(&self, kind: &str) -> Result<(), PluginError> {
        if self.granted.contains(kind) {
            return Ok(());
        }
        if self.revoked.contains(kind) {
            return Err(PluginError::Revoked(kind.to_owned()));
        }
        Err(PluginError::Denied(kind.to_owned()))
    }

    /// The view must exist; ids are never reused, so a stale id is simply unknown.
    fn view_mut(&mut self, view: u64) -> Result<&mut ViewRecord, PluginError> {
        self.views
            .get_mut(&view)
            .ok_or_else(|| PluginError::NotFound(format!("no open view {view}")))
    }
}

impl WasiView for HostState {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

impl types::Host for HostState {}

impl host::Host for HostState {
    fn log(&mut self, level: LogLevel, message: String) {
        if self.logs.len() == MAX_LOG_LINES {
            return;
        }
        let bounded: String = message.chars().take(MAX_LOG_BYTES).collect();
        self.logs.push((level, bounded));
    }

    fn settings_get(&mut self, key: String) -> Result<Option<Vec<u8>>, PluginError> {
        self.require("storage")?;
        check_settings_key(&key)?;
        Ok(self.settings.get(&key).cloned())
    }

    fn settings_set(&mut self, key: String, value: Vec<u8>) -> Result<(), PluginError> {
        self.require("storage")?;
        check_settings_key(&key)?;
        if value.len() > MAX_SETTINGS_VALUE_BYTES {
            return Err(PluginError::InvalidInput("the value is too large".into()));
        }
        self.settings.insert(key, value);
        Ok(())
    }

    fn open_view(&mut self, kind: String, address: String) -> Result<u64, PluginError> {
        let Some(tab) = self.manifest.tab_type(&kind) else {
            return Err(PluginError::InvalidInput(format!(
                "tab type {kind} is not declared"
            )));
        };
        let title = tab.title.clone();
        let view = self.next_view;
        self.next_view += 1;
        self.views.insert(
            view,
            ViewRecord {
                kind,
                address,
                generation: 1,
                title,
                badge: None,
                events: Vec::new(),
            },
        );
        self.opens_to_deliver.push_back(view);
        Ok(view)
    }

    fn set_view_metadata(
        &mut self,
        view: u64,
        title: String,
        badge: Option<String>,
    ) -> Result<(), PluginError> {
        if title.chars().count() > MAX_TITLE_CHARS {
            return Err(PluginError::InvalidInput("the title is too long".into()));
        }
        let record = self.view_mut(view)?;
        record.title = title;
        record.badge = badge;
        Ok(())
    }

    fn post_view_event(
        &mut self,
        view: u64,
        topic: String,
        json: String,
    ) -> Result<(), PluginError> {
        if json.len() > MAX_MESSAGE_BYTES {
            return Err(PluginError::InvalidInput("the event is too large".into()));
        }
        let body: serde_json::Value = serde_json::from_str(&json)
            .map_err(|_| PluginError::InvalidInput("the event body is not JSON".into()))?;
        let envelope = serde_json::json!({ "v": 1, "kind": "event", "topic": topic, "body": body });
        self.view_mut(view)?.events.push(envelope.to_string());
        Ok(())
    }

    fn feed_changed(&mut self, feed: String) {
        self.changed_feeds.push(feed);
    }

    /// Gated `@unstable(feature = home-widgets)`; never linked by this host.
    fn widget_changed(&mut self, widget: String) {
        self.changed_widgets.push(widget);
    }

    fn cancel(&mut self, request: u64) {
        self.cancelled.push(request);
    }

    fn now(&mut self) -> i64 {
        SYNTHETIC_NOW
    }
}

fn check_settings_key(key: &str) -> Result<(), PluginError> {
    let well_formed = !key.is_empty()
        && key.len() <= MAX_SETTINGS_KEY_BYTES
        && key
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '.'));
    if well_formed {
        return Ok(());
    }
    Err(PluginError::InvalidInput(format!(
        "settings key {key:?} is not allowed"
    )))
}
