// SPDX-License-Identifier: MPL-2.0
//! Checks every crate's licence against `licences.toml`, so no GPL or unreviewed licence slips in.

use std::fs;
use std::path::Path;

use serde::Deserialize;
use serde_json::Value;

use crate::process;

/// The policy file: licences allowed for any crate, and narrow exceptions.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    allowed: Vec<String>,
    #[serde(default)]
    exceptions: Vec<Exception>,
}

/// One crate, at one exact version, allowed under a licence not listed above.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Exception {
    name: String,
    version: String,
    licence: String,
    reason: String,
}

/// Fail on the first crate whose licence the policy does not cover.
pub fn check(root: &Path) -> Result<(), String> {
    let policy = read_policy(root)?;
    let args = ["metadata", "--format-version", "1", "--locked"];
    let metadata: Value = serde_json::from_str(&process::cargo_output(root, &args)?)
        .map_err(|error| format!("cargo metadata printed invalid JSON: {error}"))?;
    let packages = metadata["packages"]
        .as_array()
        .ok_or("cargo metadata has no packages")?;
    let mut refused = Vec::new();
    for package in packages {
        if let Some(problem) = judge(&policy, package) {
            refused.push(problem);
        }
    }
    if refused.is_empty() {
        println!(
            "xtask: {} crates checked; every licence is allowed",
            packages.len()
        );
        return Ok(());
    }
    Err(format!(
        "licences not allowed by licences.toml:\n  {}",
        refused.join("\n  ")
    ))
}

/// `None` when the crate is allowed; otherwise why not.
fn judge(policy: &Policy, package: &Value) -> Option<String> {
    let name = package["name"].as_str().unwrap_or("?");
    let version = package["version"].as_str().unwrap_or("?");
    let Some(licence) = package["license"].as_str() else {
        return Some(format!("{name} {version}: no SPDX licence field"));
    };
    if satisfies(licence, &policy.allowed) {
        return None;
    }
    let excepted = policy.exceptions.iter().any(|exception| {
        exception.name == name
            && exception.version == version
            && exception.licence == licence
            && !exception.reason.trim().is_empty()
    });
    if excepted {
        return None;
    }
    Some(format!("{name} {version}: {licence}"))
}

fn read_policy(root: &Path) -> Result<Policy, String> {
    let path = root.join("licences.toml");
    let text = fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    toml::from_str(&text).map_err(|error| format!("licences.toml: {error}"))
}

/// Whether an SPDX expression can be met using only allowed licences.
/// `OR` needs one side allowed, `AND` needs both; `A WITH B` is one licence.
fn satisfies(expression: &str, allowed: &[String]) -> bool {
    let normalised = expression
        .replace('/', " OR ")
        .replace('(', " ( ")
        .replace(')', " ) ");
    let tokens: Vec<&str> = normalised.split_whitespace().collect();
    let mut parser = Parser {
        tokens: &tokens,
        position: 0,
        allowed,
    };
    let result = parser.or_expression();
    result.is_some_and(|ok| ok) && parser.position == tokens.len()
}

/// A tiny recursive-descent reader for SPDX expressions.
struct Parser<'a> {
    tokens: &'a [&'a str],
    position: usize,
    allowed: &'a [String],
}

impl Parser<'_> {
    fn or_expression(&mut self) -> Option<bool> {
        let mut value = self.and_expression()?;
        while self.take("OR") {
            value |= self.and_expression()?;
        }
        Some(value)
    }

    fn and_expression(&mut self) -> Option<bool> {
        let mut value = self.atom()?;
        while self.take("AND") {
            value &= self.atom()?;
        }
        Some(value)
    }

    fn atom(&mut self) -> Option<bool> {
        if self.take("(") {
            let value = self.or_expression()?;
            return self.take(")").then_some(value);
        }
        let mut licence = (*self.tokens.get(self.position)?).to_owned();
        self.position += 1;
        if self.take("WITH") {
            let exception = self.tokens.get(self.position)?;
            self.position += 1;
            licence = format!("{licence} WITH {exception}");
        }
        Some(self.allowed.contains(&licence))
    }

    fn take(&mut self, token: &str) -> bool {
        if self.tokens.get(self.position) == Some(&token) {
            self.position += 1;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allowed() -> Vec<String> {
        ["MIT", "Apache-2.0", "Apache-2.0 WITH LLVM-exception"]
            .map(String::from)
            .to_vec()
    }

    #[test]
    fn spdx_expressions_are_judged_by_their_operators() {
        let allowed = allowed();
        assert!(satisfies("MIT OR Apache-2.0", &allowed));
        assert!(satisfies("MIT/Apache-2.0", &allowed));
        assert!(satisfies(
            "Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT",
            &allowed
        ));
        assert!(satisfies("(MIT OR Apache-2.0) AND MIT", &allowed));
        assert!(!satisfies("(MIT OR Apache-2.0) AND Unicode-3.0", &allowed));
        assert!(!satisfies("GPL-3.0-only", &allowed));
        assert!(satisfies("Apache-2.0 OR GPL-2.0-only", &allowed));
        assert!(!satisfies("GPL-2.0 WITH LLVM-exception", &allowed));
        assert!(!satisfies("MIT AND", &allowed));
        assert!(!satisfies("(MIT", &allowed));
    }
}
