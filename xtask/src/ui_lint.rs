// SPDX-License-Identifier: MPL-2.0
//! Refuses interface files that expect something Syrinx's plugin-view CSP forbids.
//!
//! The CSP would block these anyway; finding them at build time means the
//! author learns why before the owner sees a blank tab.

use std::fs;
use std::path::Path;

/// File types an interface may ship.
const ALLOWED_EXTENSIONS: &[&str] = &["html", "js", "css", "svg", "png", "woff2"];

/// Text that means a remote load, dynamic code or an inline script or style.
const FORBIDDEN_EVERYWHERE: &[(&str, &str)] = &[
    (
        "http://",
        "a remote address (the CSP allows only the plugin's own origin)",
    ),
    (
        "https://",
        "a remote address (the CSP allows only the plugin's own origin)",
    ),
    ("//cdn", "a remote address"),
];
const FORBIDDEN_IN_JS: &[(&str, &str)] = &[
    ("eval(", "dynamic code (no 'unsafe-eval')"),
    ("new Function(", "dynamic code (no 'unsafe-eval')"),
    (
        ".innerHTML",
        "HTML built from strings; use textContent and createElement",
    ),
    ("document.write", "HTML built from strings"),
    (
        "fetch(",
        "a network request; ask the plugin through the bridge instead",
    ),
    (
        "XMLHttpRequest",
        "a network request; ask the plugin through the bridge instead",
    ),
    ("WebSocket", "a network connection"),
    (
        "localStorage",
        "browser storage; the plugin view is InPrivate and state lives in Rust",
    ),
    (
        "indexedDB",
        "browser storage; the plugin view is InPrivate and state lives in Rust",
    ),
];
const FORBIDDEN_IN_HTML: &[(&str, &str)] = &[
    (
        " style=",
        "an inline style attribute (style-src has no 'unsafe-inline')",
    ),
    (
        "<style",
        "an inline style element (style-src has no 'unsafe-inline')",
    ),
    (" onclick=", "an inline event handler"),
    (" onload=", "an inline event handler"),
    (" onerror=", "an inline event handler"),
    ("javascript:", "a javascript: address"),
];

/// Check every file under `ui/`.
pub fn check(dir: &Path) -> Result<(), String> {
    let mut problems = Vec::new();
    check_dir(dir, dir, &mut problems)?;
    if problems.is_empty() {
        return Ok(());
    }
    Err(format!(
        "ui/ breaks the plugin-view rules:\n  {}",
        problems.join("\n  ")
    ))
}

fn check_dir(root: &Path, dir: &Path, problems: &mut Vec<String>) -> Result<(), String> {
    let entries =
        fs::read_dir(dir).map_err(|error| format!("cannot list {}: {error}", dir.display()))?;
    for entry in entries {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path.is_dir() {
            check_dir(root, &path, problems)?;
            continue;
        }
        let shown = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .display()
            .to_string();
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default();
        if !ALLOWED_EXTENSIONS.contains(&extension) {
            problems.push(format!(
                "{shown}: .{extension} files are not served to a plugin view"
            ));
            continue;
        }
        if matches!(extension, "html" | "js" | "css" | "svg") {
            let text = fs::read_to_string(&path).map_err(|error| format!("{shown}: {error}"))?;
            check_text(&shown, extension, &text, problems);
        }
    }
    Ok(())
}

fn check_text(shown: &str, extension: &str, text: &str, problems: &mut Vec<String>) {
    let mut rules: Vec<(&str, &str)> = FORBIDDEN_EVERYWHERE.to_vec();
    match extension {
        "js" => rules.extend_from_slice(FORBIDDEN_IN_JS),
        "html" => rules.extend_from_slice(FORBIDDEN_IN_HTML),
        _ => {}
    }
    for (needle, why) in rules {
        if text.contains(needle) {
            problems.push(format!("{shown}: contains {needle:?}, {why}"));
        }
    }
    if extension == "html" && has_inline_script(text) {
        problems.push(format!(
            "{shown}: has an inline <script>; load a file with src= instead"
        ));
    }
}

/// A `<script>` element without `src=` is inline code, which the CSP blocks.
fn has_inline_script(html: &str) -> bool {
    html.match_indices("<script").any(|(start, _)| {
        let tag_end = html[start..]
            .find('>')
            .map_or(html.len(), |end| start + end);
        !html[start..tag_end].contains("src=")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn problems(extension: &str, text: &str) -> Vec<String> {
        let mut found = Vec::new();
        check_text("file", extension, text, &mut found);
        found
    }

    #[test]
    fn clean_files_pass() {
        assert!(problems("html", r#"<script type="module" src="app.js"></script>"#).is_empty());
        assert!(problems("js", "node.textContent = text;").is_empty());
    }

    #[test]
    fn forbidden_patterns_are_reported() {
        assert_eq!(problems("html", "<script>alert(1)</script>").len(), 1);
        assert_eq!(problems("html", r#"<p style="color:red">"#).len(), 1);
        assert_eq!(problems("js", "eval(code)").len(), 1);
        assert_eq!(problems("js", "el.innerHTML = x").len(), 1);
        assert_eq!(
            problems("css", "@import url(https://example.com/a.css);").len(),
            1
        );
    }
}
