// SPDX-License-Identifier: MPL-2.0
//! `cargo xtask`: the one command that builds, packages and checks this plugin.

mod assemble;
mod licences;
mod process;
mod repro;
mod ui_lint;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use plugin_format::Manifest;

/// What `cargo xtask help` prints.
const USAGE: &str = "\
usage: cargo xtask <task>

  build        build plugin.wasm and assemble dist/<id>-<version>/ with its file list
  test         build, then run the unit and contract tests
  check        everything CI runs: format, Clippy, UI rules, licences, build, tests
  licences     check every crate's licence against licences.toml
  repro        build twice in two different folders and compare the bytes
  check-tag T  refuse unless tag T equals v<version> from syrinx-plugin.toml
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    let result = match words.as_slice() {
        ["build"] => build(&workspace_root()).map(|_| ()),
        ["test"] => test(),
        ["check"] => check(),
        ["licences"] => licences::check(&workspace_root()),
        ["repro"] => repro::check(&workspace_root()),
        ["check-tag", tag] => check_tag(tag),
        ["help"] | [] => {
            print!("{USAGE}");
            Ok(())
        }
        _ => Err(format!("unknown task\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("xtask: {message}");
            ExitCode::FAILURE
        }
    }
}

/// The repository root: xtask lives one folder below it.
pub fn workspace_root() -> PathBuf {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map_or_else(|| manifest_dir.to_path_buf(), Path::to_path_buf)
}

/// Read and check syrinx-plugin.toml under `root`.
pub fn read_manifest(root: &Path) -> Result<Manifest, String> {
    let path = root.join("syrinx-plugin.toml");
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    Manifest::parse(&text).map_err(|error| format!("syrinx-plugin.toml: {error}"))
}

/// Check the UI, build the component and assemble the package folder.
pub fn build(root: &Path) -> Result<PathBuf, String> {
    let manifest = read_manifest(root)?;
    ui_lint::check(&root.join("ui"))?;
    let module = assemble::build_module(root)?;
    let package = assemble::package(root, &manifest, &module)?;
    println!("xtask: package ready in {}", package.display());
    Ok(package)
}

/// Build, then run every test with the package's location passed in.
fn test() -> Result<(), String> {
    let root = workspace_root();
    let package = build(&root)?;
    let package = package.display().to_string();
    process::cargo(
        &root,
        &["test", "--workspace", "--locked"],
        &[("SYRINX_PLUGIN_PACKAGE", package.as_str())],
    )?;
    let mut node = Command::new("node");
    node.args(["--test", "tests/ui/bridge.test.mjs"])
        .current_dir(&root);
    process::run(
        node,
        "node --test tests/ui/bridge.test.mjs (needs Node.js 20 or later)",
    )
}

/// Everything CI's `ci.yml` runs, in the same order, except the reproducibility build.
fn check() -> Result<(), String> {
    let root = workspace_root();
    let deny_warnings = ["--", "-D", "warnings"];
    process::cargo(&root, &["fmt", "--all", "--check"], &[])?;
    let native = ["clippy", "--workspace", "--all-targets", "--locked"];
    process::cargo(&root, &[&native[..], &deny_warnings].concat(), &[])?;
    let wasm = [
        "clippy",
        "-p",
        "hello-plugin",
        "--target",
        "wasm32-wasip2",
        "--locked",
    ];
    process::cargo(&root, &[&wasm[..], &deny_warnings].concat(), &[])?;
    licences::check(&root)?;
    test()
}

/// A release tag must name exactly the manifest's version.
fn check_tag(tag: &str) -> Result<(), String> {
    let manifest = read_manifest(&workspace_root())?;
    let expected = format!("v{}", manifest.plugin.version);
    if tag == expected {
        return Ok(());
    }
    Err(format!(
        "tag {tag} does not match the manifest version (expected {expected})"
    ))
}
