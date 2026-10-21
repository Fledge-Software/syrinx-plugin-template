// SPDX-License-Identifier: MPL-2.0
//! Running cargo and reporting a failed spawn or a failed exit, never swallowing either.

use std::path::Path;
use std::process::Command;

/// Run `cargo <args>` in `dir` with extra environment variables.
pub fn cargo(dir: &Path, args: &[&str], env: &[(&str, &str)]) -> Result<(), String> {
    let mut command = Command::new(cargo_program());
    command.args(args).current_dir(dir);
    for (name, value) in env {
        command.env(name, value);
    }
    run(command, &format!("cargo {}", args.join(" ")))
}

/// Run `cargo <args>` and return what it printed on standard output.
pub fn cargo_output(dir: &Path, args: &[&str]) -> Result<String, String> {
    let label = format!("cargo {}", args.join(" "));
    let output = Command::new(cargo_program())
        .args(args)
        .current_dir(dir)
        .output()
        .map_err(|error| format!("could not start {label}: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("{label} failed ({}):\n{stderr}", output.status));
    }
    String::from_utf8(output.stdout).map_err(|_| format!("{label} printed invalid UTF-8"))
}

/// Run a prepared command, naming it in any error.
pub fn run(mut command: Command, label: &str) -> Result<(), String> {
    println!("xtask: {label}");
    let status = command
        .status()
        .map_err(|error| format!("could not start {label}: {error}"))?;
    if status.success() {
        return Ok(());
    }
    Err(format!("{label} failed ({status})"))
}

/// The cargo that started xtask, so the pinned toolchain is used throughout.
fn cargo_program() -> String {
    std::env::var("CARGO").unwrap_or_else(|_| "cargo".into())
}
