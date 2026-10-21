// SPDX-License-Identifier: MPL-2.0
//! The plugin's entry points: each WIT export hands over to the logic in `greeter`.

mod greeter;

use std::sync::{Mutex, MutexGuard};

use bindings::exports::syrinx::plugin::{commands, events, forms, lifecycle, search_feed, views};
use bindings::syrinx::plugin::host::{self, LogLevel};
use bindings::syrinx::plugin::types::{Outcome, PluginError};
use greeter::{Greeter, HostApi};

/// Rust bindings generated from wit/ at compile time. The calling-convention
/// glue wit-bindgen writes is `unsafe`; no hand-written code is.
#[allow(unsafe_code)]
mod bindings {
    wit_bindgen::generate!({ path: "../wit", world: "plugin" });

    // Export the entry points only in the WebAssembly build; a native test
    // build cannot link symbols named like `syrinx:plugin/commands@0.1.0#run`.
    #[cfg(target_arch = "wasm32")]
    use super::Plugin;
    #[cfg(target_arch = "wasm32")]
    export!(Plugin);
}

/// The one instance's state; the host never calls in from two threads.
static STATE: Mutex<Greeter> = Mutex::new(Greeter::new());

/// Take the state, recovering it if an earlier call panicked (that call
/// already trapped, and the host discards a trapped instance anyway).
fn state() -> MutexGuard<'static, Greeter> {
    STATE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The host functions from wit/, behind the trait the logic uses.
struct Syrinx;

impl HostApi for Syrinx {
    fn log(&self, level: LogLevel, message: &str) {
        host::log(level, message);
    }
    fn settings_get(&self, key: &str) -> Result<Option<Vec<u8>>, PluginError> {
        host::settings_get(key)
    }
    fn settings_set(&self, key: &str, value: &[u8]) -> Result<(), PluginError> {
        host::settings_set(key, value)
    }
    fn open_view(&self, kind: &str, address: &str) -> Result<u64, PluginError> {
        host::open_view(kind, address)
    }
    fn set_view_metadata(
        &self,
        view: u64,
        title: &str,
        badge: Option<&str>,
    ) -> Result<(), PluginError> {
        host::set_view_metadata(view, title, badge)
    }
    fn post_view_event(&self, view: u64, topic: &str, json: &str) -> Result<(), PluginError> {
        host::post_view_event(view, topic, json)
    }
    fn now(&self) -> i64 {
        host::now()
    }
}

/// The type the generated bindings export.
struct Plugin;

impl lifecycle::Guest for Plugin {
    fn activate(info: lifecycle::Activation) -> Result<(), PluginError> {
        state().activate(&Syrinx, &info.granted)
    }

    fn deactivate() {
        *state() = Greeter::new();
    }
}

impl commands::Guest for Plugin {
    fn run(id: String) -> Result<(), PluginError> {
        state().run(&Syrinx, &id)
    }
}

impl forms::Guest for Plugin {
    fn form_for(id: String) -> Result<forms::FormSpec, PluginError> {
        state().form_for(&id)
    }

    fn validate(id: String, values: Vec<forms::FieldValue>) -> Vec<forms::FieldError> {
        state().validate(&id, &values)
    }

    fn submit(
        id: String,
        submission_id: String,
        values: Vec<forms::FieldValue>,
    ) -> forms::SubmitResult {
        state().submit(&Syrinx, &id, &submission_id, &values)
    }
}

impl views::Guest for Plugin {
    fn opened(view: u64, kind: String, _address: String) -> Result<(), PluginError> {
        state().view_opened(&Syrinx, view, &kind)
    }

    fn message(request: views::ViewRequest) -> Result<String, PluginError> {
        state().view_message(&Syrinx, &request)
    }

    fn closed(view: u64) {
        state().view_closed(view);
    }
}

/// This plugin declares no search feed, so every feed is unknown.
impl search_feed::Guest for Plugin {
    fn changes(
        feed: String,
        _after: Option<String>,
        _max: u32,
    ) -> Result<search_feed::Batch, PluginError> {
        Err(PluginError::NotFound(format!("no feed {feed}")))
    }

    fn resolve(feed: String, _id: String) -> Result<String, PluginError> {
        Err(PluginError::NotFound(format!("no feed {feed}")))
    }
}

/// This plugin never answers a form with `pending`, so nothing is delivered.
impl events::Guest for Plugin {
    fn deliver(_request: u64, _outcome: Outcome) {}
}
