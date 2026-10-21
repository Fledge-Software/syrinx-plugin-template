// SPDX-License-Identifier: MPL-2.0
//! Builds the package twice, from two copies of the source in different folders, and compares.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use plugin_format::PackageList;
use plugin_format::package::LIST_NAME;

use crate::process;

/// Top-level entries copied into each scratch build; build output is never copied.
const SOURCE_ENTRIES: &[&str] = &[
    ".cargo",
    "Cargo.lock",
    "Cargo.toml",
    "LICENSE",
    "format",
    "licences.toml",
    "plugin",
    "rust-toolchain.toml",
    "syrinx-plugin.toml",
    "test-host",
    "ui",
    "wit",
    "xtask",
];

/// Build in two fresh folders and require byte-identical `package` lists.
pub fn check(root: &Path) -> Result<(), String> {
    let scratch = root.join("target").join("repro");
    if scratch.exists() {
        fs::remove_dir_all(&scratch)
            .map_err(|error| format!("cannot clear {}: {error}", scratch.display()))?;
    }
    let first = build_copy(root, &scratch.join("first-build"))?;
    let second = build_copy(root, &scratch.join("second").join("nested-build"))?;
    if first != second {
        return Err(format!(
            "the two builds differ:\n--- first\n{}\n--- second\n{}",
            first.render(),
            second.render()
        ));
    }
    let wasm = first
        .file("plugin.wasm")
        .map_or("?", |file| file.sha256.as_str());
    println!("xtask: reproducible; plugin.wasm sha256 {wasm}");
    fs::remove_dir_all(&scratch)
        .map_err(|error| format!("cannot clean {}: {error}", scratch.display()))
}

/// Copy the source to `dir`, build it there with its own target folder, read its list.
fn build_copy(root: &Path, dir: &Path) -> Result<PackageList, String> {
    for entry in SOURCE_ENTRIES {
        copy_entry(&root.join(entry), &dir.join(entry))?;
    }
    let mut command = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    command
        .args(["xtask", "build"])
        .current_dir(dir)
        .env("CARGO_TARGET_DIR", dir.join("target"));
    process::run(command, &format!("cargo xtask build in {}", dir.display()))?;
    let package_dir = find_package(&dir.join("dist"))?;
    let text =
        fs::read_to_string(package_dir.join(LIST_NAME)).map_err(|error| error.to_string())?;
    PackageList::parse(&text).map_err(|error| error.to_string())
}

fn find_package(dist: &Path) -> Result<PathBuf, String> {
    let mut entries = fs::read_dir(dist)
        .map_err(|error| format!("cannot list {}: {error}", dist.display()))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    entries.sort();
    entries
        .into_iter()
        .next()
        .ok_or_else(|| format!("{} holds no package", dist.display()))
}

fn copy_entry(from: &Path, to: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(from)
        .map_err(|error| format!("cannot read {}: {error}", from.display()))?;
    if metadata.is_dir() {
        fs::create_dir_all(to).map_err(|error| error.to_string())?;
        for entry in fs::read_dir(from).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            copy_entry(&entry.path(), &to.join(entry.file_name()))?;
        }
        return Ok(());
    }
    if !metadata.is_file() {
        return Err(format!("{} is a link; refusing to copy it", from.display()));
    }
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::copy(from, to)
        .map(|_| ())
        .map_err(|error| format!("cannot copy {}: {error}", from.display()))
}
