// SPDX-License-Identifier: MPL-2.0
//! Contract tests: load the built package in the test host and drive every export.
//!
//! Run through `cargo xtask test`, which builds the package first and passes its
//! folder in `SYRINX_PLUGIN_PACKAGE`. All data is synthetic.

use std::fs;
use std::path::{Path, PathBuf};

use plugin_format::PackageList;
use plugin_format::package::LIST_NAME;
use plugin_test_host::{
    BridgeError, FieldKind, HostError, MAX_MESSAGE_BYTES, Outcome, SubmitResult, TestHost,
};

const GREET: &str = "hello.greet";

/// The package folder `cargo xtask build` produced.
fn package_dir() -> PathBuf {
    let dir = std::env::var_os("SYRINX_PLUGIN_PACKAGE")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../dist/hello-0.1.0"));
    assert!(
        dir.join(LIST_NAME).is_file(),
        "no built package at {}; run `cargo xtask test`, which builds it first",
        dir.display()
    );
    dir
}

fn load() -> TestHost {
    TestHost::load_package(&package_dir()).expect("the template package loads")
}

/// The template manifest with its commented storage request switched on.
fn manifest_requesting_storage() -> String {
    fs::read_to_string(package_dir().join("syrinx-plugin.toml"))
        .expect("read manifest")
        .replace("# [[capabilities]]", "[[capabilities]]")
        .replace("# kind = ", "kind = ")
        .replace("# reason = ", "reason = ")
        .replace("# required = ", "required = ")
}

/// A greeting form's values, as the palette would submit them.
fn greeting(name: &str) -> [(&str, &str); 3] {
    [("name", name), ("greeting", "hello"), ("shout", "false")]
}

fn finished_text(result: SubmitResult) -> String {
    match result {
        SubmitResult::Finished(Outcome::Done(Some(text))) => text,
        other => panic!("expected a finished greeting, got {other:?}"),
    }
}

/// A view request envelope from the interface.
fn request(id: u64, method: &str) -> String {
    format!(r#"{{"v":1,"kind":"request","id":{id},"method":"{method}","body":{{}}}}"#)
}

/// A scratch copy of the package, removed when dropped.
struct ScratchCopy(PathBuf);

impl ScratchCopy {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("syrinx-contract-{}-{name}", std::process::id()));
        if dir.exists() {
            fs::remove_dir_all(&dir).expect("remove an earlier copy");
        }
        copy_tree(&package_dir(), &dir);
        Self(dir)
    }

    /// Rewrite the file list after changing files, as a dishonest builder would.
    fn relist(&self) {
        let old = PackageList::parse(&fs::read_to_string(self.0.join(LIST_NAME)).expect("list"))
            .expect("parse list");
        let list =
            PackageList::from_folder(&self.0, &old.id, &old.version, &old.world).expect("relist");
        fs::write(self.0.join(LIST_NAME), list.render()).expect("write list");
    }
}

impl Drop for ScratchCopy {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("could not remove {}: {error}", self.0.display());
        }
    }
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("create folder");
    for entry in fs::read_dir(from).expect("list folder") {
        let entry = entry.expect("entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("type").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).expect("copy file");
        }
    }
}

#[test]
fn the_package_verifies_and_loads_without_running_plugin_code() {
    let host = load();
    assert_eq!(host.package().id, "hello");
    assert_eq!(host.package().world, "syrinx:plugin/plugin@0.1.0");
    assert_eq!(host.export_calls(), 0, "loading must not call the plugin");
}

#[test]
fn the_command_opens_a_tab_that_closes_and_reopens() {
    let mut host = load();
    host.activate().expect("activate call").expect("activates");
    host.run_command(GREET).expect("run call").expect("runs");
    let views = host.open_views();
    assert_eq!(views.len(), 1);
    let view = views[0];
    let record = host.view(view).expect("view record");
    assert_eq!(record.kind, "hello.greetings");
    assert_eq!(record.title, "Greetings");

    let origin = host.origin();
    let reply = host
        .view_message(
            view,
            &origin,
            record.generation,
            &request(1, "list-greetings"),
        )
        .expect("checked")
        .expect("a response");
    assert_eq!(
        reply,
        r#"{"body":{"greetings":[]},"id":1,"kind":"response","ok":true,"v":1}"#
    );

    host.close_view(view).expect("close");
    assert!(host.open_views().is_empty());
    host.run_command(GREET)
        .expect("run call")
        .expect("runs again");
    let reopened = host.open_views();
    assert_eq!(reopened.len(), 1);
    assert_ne!(reopened[0], view, "view ids are never reused");
    host.deactivate().expect("deactivate");
}

#[test]
fn the_form_validates_submits_and_updates_an_open_tab() {
    let mut host = load();
    host.activate().expect("activate call").expect("activates");
    let form = host.form_for(GREET).expect("form call").expect("a form");
    assert_eq!(form.title, "Say hello");
    assert!(matches!(form.fields[0].kind, FieldKind::Text(_)));
    assert!(matches!(form.fields[1].kind, FieldKind::Choice(_)));
    assert!(matches!(form.fields[2].kind, FieldKind::Toggle));

    host.run_command(GREET)
        .expect("run call")
        .expect("opens the tab");
    let view = host.open_views()[0];

    let invalid = host
        .submit(GREET, "submission-1", &greeting(" "))
        .expect("submit call");
    let SubmitResult::Invalid(errors) = invalid else {
        panic!("an empty name must be invalid");
    };
    assert_eq!(errors[0].name, "name");

    let done = host
        .submit(GREET, "submission-2", &greeting("Ada"))
        .expect("submit call");
    assert_eq!(finished_text(done), "Hello, Ada!");
    let record = host.view(view).expect("view");
    assert_eq!(record.badge.as_deref(), Some("1"));
    assert_eq!(record.events.len(), 1);
    assert!(record.events[0].contains(r#""kind":"event""#));
    assert!(record.events[0].contains(r#""topic":"greeting-added""#));
    assert!(record.events[0].contains("Hello, Ada!"));

    let repeated = host
        .submit(GREET, "submission-2", &greeting("Ada"))
        .expect("submit call");
    assert_eq!(finished_text(repeated), "Hello, Ada!");
    assert_eq!(
        host.view(view).expect("view").badge.as_deref(),
        Some("1"),
        "no duplicate"
    );
}

#[test]
fn an_ungranted_capability_is_denied_and_the_plugin_copes() {
    let mut host = load();
    let refused = host.grant("storage");
    assert!(
        matches!(refused, Err(HostError::Refused(_))),
        "a capability the manifest does not request cannot be granted"
    );
    host.activate().expect("activate call").expect("activates");
    let done = host
        .submit(GREET, "submission-1", &greeting("Ada"))
        .expect("submit call");
    assert_eq!(finished_text(done), "Hello, Ada!");
    assert!(
        host.logs()
            .iter()
            .any(|(_, line)| line.contains("storage is not granted")),
        "the plugin saw the typed `denied` error: {:?}",
        host.logs()
    );
}

#[test]
fn a_granted_capability_works_and_revoking_it_denies_again() {
    let dir = package_dir();
    let mut host =
        TestHost::load_package_with_manifest(&dir, &manifest_requesting_storage()).expect("loads");
    host.grant("storage").expect("requested, so grantable");
    host.activate().expect("activate call").expect("activates");
    host.submit(GREET, "submission-1", &greeting("Ada"))
        .expect("submit call");
    host.deactivate().expect("deactivate");
    host.activate().expect("activate call").expect("activates");
    let form = host.form_for(GREET).expect("form call").expect("form");
    assert_eq!(
        form.fields[0].initial.as_deref(),
        Some("Ada"),
        "read back from storage"
    );

    host.revoke("storage");
    host.submit(GREET, "submission-2", &greeting("Bo"))
        .expect("submit call");
    assert!(
        host.logs()
            .iter()
            .any(|(_, line)| line.contains("storage is not granted"))
    );
}

#[test]
fn the_bridge_refuses_bad_messages_before_any_plugin_call() {
    let mut host = load();
    host.run_command(GREET).expect("run call").expect("opens");
    let view = host.open_views()[0];
    let origin = host.origin();
    let generation = host.view(view).expect("view").generation;
    let calls = host.export_calls();

    let cases: Vec<(String, String, u64, BridgeError)> = vec![
        (
            "https://accounts.example.com".into(),
            request(1, "list-greetings"),
            generation,
            BridgeError::WrongOrigin,
        ),
        (
            origin.clone(),
            request(1, "read-files"),
            generation,
            BridgeError::UndeclaredMethod("read-files".into()),
        ),
        (
            origin.clone(),
            "x".repeat(MAX_MESSAGE_BYTES + 1),
            generation,
            BridgeError::TooLarge,
        ),
        (
            origin.clone(),
            request(1, "list-greetings").replace("\"v\":1", "\"v\":9"),
            generation,
            BridgeError::UnsupportedVersion,
        ),
    ];
    for (from, raw, generation, expected) in cases {
        let result = host.view_message(view, &from, generation, &raw);
        match result {
            Err(HostError::Bridge(error)) => assert_eq!(error, expected),
            other => panic!("expected {expected:?}, got {other:?}"),
        }
    }
    assert_eq!(
        host.export_calls(),
        calls,
        "no refused message reached the plugin"
    );

    let reloaded = host.reload_view(view).expect("reload");
    let stale = host.view_message(view, &origin, generation, &request(2, "list-greetings"));
    assert!(matches!(
        stale,
        Err(HostError::Bridge(BridgeError::StaleView))
    ));
    let cancel = r#"{"v":1,"kind":"cancel","id":2}"#;
    assert_eq!(
        host.view_message(view, &origin, reloaded, cancel)
            .expect("cancel"),
        None
    );
    let fresh = host.view_message(view, &origin, reloaded, &request(3, "list-greetings"));
    assert!(
        fresh
            .expect("checked")
            .expect("response")
            .contains(r#""ok":true"#)
    );
}

#[test]
fn undeclared_commands_and_forms_never_reach_the_plugin() {
    let mut host = load();
    assert!(matches!(
        host.run_command("hello.other"),
        Err(HostError::Refused(_))
    ));
    assert!(matches!(
        host.form_for("other.greet"),
        Err(HostError::Refused(_))
    ));
    assert_eq!(host.export_calls(), 0);
}

#[test]
fn a_changed_or_extra_file_is_refused() {
    let changed = ScratchCopy::new("changed");
    fs::write(changed.0.join("ui/app.js"), "// tampered\n").expect("write");
    let result = TestHost::load_package(&changed.0);
    assert!(
        matches!(result, Err(HostError::Package(_))),
        "changed file accepted"
    );

    let extra = ScratchCopy::new("extra");
    fs::write(extra.0.join("helper.dll"), "synthetic").expect("write");
    let result = TestHost::load_package(&extra.0);
    assert!(
        matches!(result, Err(HostError::Package(_))),
        "unlisted file accepted"
    );
}

#[test]
fn a_manifest_that_disagrees_with_the_package_is_refused() {
    let dir = package_dir();
    let manifest = fs::read_to_string(dir.join("syrinx-plugin.toml"))
        .expect("read manifest")
        .replace("version = \"0.1.0\"", "version = \"0.2.0\"");
    let result = TestHost::load_package_with_manifest(&dir, &manifest);
    assert!(matches!(result, Err(HostError::Refused(_))));
}

#[test]
fn precompiled_native_code_is_refused() {
    let copy = ScratchCopy::new("precompiled");
    let module = fs::read(copy.0.join("plugin.wasm")).expect("read module");
    let engine = wasmtime::Engine::default();
    let native = engine.precompile_component(&module).expect("precompile");
    fs::write(copy.0.join("plugin.wasm"), native).expect("write");
    copy.relist();
    let result = TestHost::load_package(&copy.0);
    assert!(
        matches!(result, Err(HostError::Component(ref detail)) if detail.contains("precompiled")),
        "got {:?}",
        result.err()
    );
}
