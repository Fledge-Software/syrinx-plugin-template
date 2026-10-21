// SPDX-License-Identifier: MPL-2.0
//! The `package` file list: one canonical byte form, in the shape of Syrinx's update manifest.
//!
//! Syrinx signs only this list (`package.sig`); each file is bound to the
//! signature through its SHA-256 here. See README.md, "The package".

use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::path::Path;

use sha2::{Digest, Sha256};

/// The first line; it keeps a plugin list from ever passing as a Syrinx update.
pub const HEADER: &str = "syrinx-plugin-package 1";
/// The list's own file name, and its detached signature's.
pub const LIST_NAME: &str = "package";
pub const SIGNATURE_NAME: &str = "package.sig";

/// Bounds from Syrinx's update format, so a hostile list cannot exhaust memory.
const MAX_LIST_BYTES: usize = 2 * 1024 * 1024;
const MAX_FILES: usize = 4096;
const MAX_FILE_BYTES: u64 = 1024 * 1024 * 1024;
const MAX_COMPONENT_BYTES: usize = 64;
const MAX_COMPONENTS: usize = 8;
const MAX_PATH_BYTES: usize = 240;

/// Windows device names, refused as any path component with or without extension.
const DEVICE_NAMES: &[&str] = &[
    "con", "prn", "aux", "nul", "com0", "com1", "com2", "com3", "com4", "com5", "com6", "com7",
    "com8", "com9", "lpt0", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// One listed file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    pub sha256: String,
    pub size: u64,
    pub path: String,
}

/// The whole list: who the package is and every file in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageList {
    pub id: String,
    pub version: String,
    pub world: String,
    pub files: Vec<FileEntry>,
}

/// Why a list or a folder was refused.
#[derive(Debug)]
pub enum PackageError {
    Io(String),
    TooLarge,
    NotCanonical(String),
    BadPath(String),
    Collision(String),
    Missing(String),
    Unlisted(String),
    Mismatch(String),
}

impl fmt::Display for PackageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(detail) => write!(f, "could not read the package: {detail}"),
            Self::TooLarge => write!(f, "the package is over its size bounds"),
            Self::NotCanonical(detail) => write!(f, "the file list is not canonical: {detail}"),
            Self::BadPath(path) => write!(f, "path {path:?} is not allowed in a package"),
            Self::Collision(path) => write!(f, "path {path:?} collides with another listed path"),
            Self::Missing(path) => write!(f, "listed file {path:?} is missing"),
            Self::Unlisted(path) => write!(f, "file {path:?} is in the folder but not listed"),
            Self::Mismatch(path) => {
                write!(f, "file {path:?} does not match its listed hash or size")
            }
        }
    }
}

impl std::error::Error for PackageError {}

impl PackageList {
    /// List every file under `dir` except the list and its signature.
    pub fn from_folder(
        dir: &Path,
        id: &str,
        version: &str,
        world: &str,
    ) -> Result<Self, PackageError> {
        let mut files = Vec::new();
        for path in folder_files(dir)? {
            if is_list_or_signature(&path) {
                continue;
            }
            let bytes = fs::read(dir.join(&path)).map_err(io_error)?;
            files.push(FileEntry {
                sha256: sha256_hex(&bytes),
                size: bytes.len() as u64,
                path,
            });
        }
        let list = Self {
            id: id.to_owned(),
            version: version.to_owned(),
            world: world.to_owned(),
            files,
        };
        list.check_files()?;
        Ok(list)
    }

    /// The one byte form: LF endings, single spaces, files sorted by path.
    pub fn render(&self) -> String {
        let mut files = self.files.clone();
        files.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
        let mut text = format!(
            "{HEADER}\nid {}\nversion {}\nworld {}\n",
            self.id, self.version, self.world
        );
        for file in &files {
            text.push_str(&format!(
                "file {} {} {}\n",
                file.sha256, file.size, file.path
            ));
        }
        text
    }

    /// Parse strictly, then require that rendering gives back the same bytes.
    pub fn parse(text: &str) -> Result<Self, PackageError> {
        if text.len() > MAX_LIST_BYTES {
            return Err(PackageError::TooLarge);
        }
        let mut lines = text
            .strip_suffix('\n')
            .ok_or_else(|| not_canonical("no final newline"))?
            .split('\n');
        if lines.next() != Some(HEADER) {
            return Err(not_canonical("wrong first line"));
        }
        let id = field(lines.next(), "id")?;
        let version = field(lines.next(), "version")?;
        let world = field(lines.next(), "world")?;
        let files = lines.map(parse_file_line).collect::<Result<Vec<_>, _>>()?;
        let list = Self {
            id,
            version,
            world,
            files,
        };
        list.check_files()?;
        if list.render() != text {
            return Err(not_canonical("re-rendering changed the bytes"));
        }
        Ok(list)
    }

    /// Check a folder against this list: every listed file matches, nothing else is there.
    pub fn verify_folder(&self, dir: &Path) -> Result<(), PackageError> {
        let listed: BTreeSet<&str> = self.files.iter().map(|file| file.path.as_str()).collect();
        for path in folder_files(dir)? {
            if !is_list_or_signature(&path) && !listed.contains(path.as_str()) {
                return Err(PackageError::Unlisted(path));
            }
        }
        self.files
            .iter()
            .try_for_each(|file| verify_file(dir, file))
    }

    /// The listed file at this path, if any.
    pub fn file(&self, path: &str) -> Option<&FileEntry> {
        self.files.iter().find(|file| file.path == path)
    }

    /// Bounds, path rules and collisions, for a list read or built.
    fn check_files(&self) -> Result<(), PackageError> {
        if self.files.is_empty() || self.files.len() > MAX_FILES {
            return Err(PackageError::TooLarge);
        }
        let mut seen = BTreeSet::new();
        for file in &self.files {
            check_path(&file.path)?;
            if file.size > MAX_FILE_BYTES {
                return Err(PackageError::TooLarge);
            }
            if !seen.insert(file.path.to_ascii_lowercase()) {
                return Err(PackageError::Collision(file.path.clone()));
            }
        }
        check_directory_collisions(&seen)
    }
}

/// A path is safe to join onto a staging folder on Windows (update-format rules).
pub fn check_path(path: &str) -> Result<(), PackageError> {
    let bad = || PackageError::BadPath(path.to_owned());
    let components: Vec<&str> = path.split('/').collect();
    if path.len() > MAX_PATH_BYTES || components.len() > MAX_COMPONENTS {
        return Err(bad());
    }
    if components
        .iter()
        .all(|component| is_safe_component(component))
    {
        return Ok(());
    }
    Err(bad())
}

/// SHA-256 as lowercase hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn is_safe_component(component: &str) -> bool {
    let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-');
    let stem = component.split('.').next().unwrap_or_default();
    !component.is_empty()
        && component.len() <= MAX_COMPONENT_BYTES
        && component.chars().all(allowed)
        && component != "."
        && component != ".."
        && !component.ends_with('.')
        && !DEVICE_NAMES.contains(&stem.to_ascii_lowercase().as_str())
}

/// `a` and `a/b` cannot both exist: one name cannot be a file and a folder.
fn check_directory_collisions(lowercase_paths: &BTreeSet<String>) -> Result<(), PackageError> {
    for path in lowercase_paths {
        let mut prefix = String::new();
        for component in path.split('/') {
            if !prefix.is_empty() && lowercase_paths.contains(&prefix) {
                return Err(PackageError::Collision(prefix));
            }
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(component);
        }
    }
    Ok(())
}

fn field(line: Option<&str>, name: &str) -> Result<String, PackageError> {
    let value = line
        .and_then(|line| line.strip_prefix(name))
        .and_then(|rest| rest.strip_prefix(' '))
        .ok_or_else(|| not_canonical(&format!("expected the {name} line")))?;
    if value.is_empty() || value.contains(' ') {
        return Err(not_canonical(&format!("bad {name} value")));
    }
    Ok(value.to_owned())
}

fn parse_file_line(line: &str) -> Result<FileEntry, PackageError> {
    let parts: Vec<&str> = line.split(' ').collect();
    let [keyword, sha256, size, path] = parts.as_slice() else {
        return Err(not_canonical("a file line needs four fields"));
    };
    let is_hash = sha256.len() == 64
        && sha256
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c));
    if *keyword != "file" || !is_hash {
        return Err(not_canonical("bad file line"));
    }
    let size = size
        .parse::<u64>()
        .map_err(|_| not_canonical("bad file size"))?;
    Ok(FileEntry {
        sha256: (*sha256).to_owned(),
        size,
        path: (*path).to_owned(),
    })
}

fn verify_file(dir: &Path, file: &FileEntry) -> Result<(), PackageError> {
    let full = dir.join(&file.path);
    let metadata =
        fs::symlink_metadata(&full).map_err(|_| PackageError::Missing(file.path.clone()))?;
    if !metadata.is_file() || metadata.len() != file.size {
        return Err(PackageError::Mismatch(file.path.clone()));
    }
    let bytes = fs::read(&full).map_err(io_error)?;
    if sha256_hex(&bytes) != file.sha256 {
        return Err(PackageError::Mismatch(file.path.clone()));
    }
    Ok(())
}

/// Every regular file under `dir`, as `/`-separated relative paths. Links are refused.
fn folder_files(dir: &Path) -> Result<Vec<String>, PackageError> {
    let mut found = Vec::new();
    let mut pending = vec![String::new()];
    while let Some(relative) = pending.pop() {
        let entries = fs::read_dir(dir.join(&relative)).map_err(io_error)?;
        for entry in entries {
            let entry = entry.map_err(io_error)?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = if relative.is_empty() {
                name
            } else {
                format!("{relative}/{name}")
            };
            let kind = entry.file_type().map_err(io_error)?;
            if kind.is_dir() {
                pending.push(path);
            } else if kind.is_file() {
                found.push(path);
            } else {
                return Err(PackageError::BadPath(path));
            }
        }
    }
    found.sort();
    Ok(found)
}

fn is_list_or_signature(path: &str) -> bool {
    path == LIST_NAME || path == SIGNATURE_NAME
}

fn not_canonical(detail: &str) -> PackageError {
    PackageError::NotCanonical(detail.to_owned())
}

fn io_error(error: std::io::Error) -> PackageError {
    PackageError::Io(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str) -> FileEntry {
        FileEntry {
            sha256: sha256_hex(path.as_bytes()),
            size: path.len() as u64,
            path: path.to_owned(),
        }
    }

    fn list(paths: &[&str]) -> PackageList {
        PackageList {
            id: "hello".into(),
            version: "0.1.0".into(),
            world: "syrinx:plugin/plugin@0.1.0".into(),
            files: paths.iter().map(|path| entry(path)).collect(),
        }
    }

    #[test]
    fn a_rendered_list_parses_back_to_itself_sorted_by_path() {
        let text = list(&["ui/app.js", "plugin.wasm", "LICENSE"]).render();
        assert!(text.starts_with("syrinx-plugin-package 1\nid hello\nversion 0.1.0\n"));
        let parsed = PackageList::parse(&text).expect("parse");
        let paths: Vec<&str> = parsed.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["LICENSE", "plugin.wasm", "ui/app.js"]);
    }

    #[test]
    fn any_other_byte_form_is_refused() {
        let text = list(&["plugin.wasm", "ui/app.js"]).render();
        let unsorted = text.replace("plugin.wasm", "zz.wasm");
        for variant in [
            text.replace('\n', "\r\n"),
            text.trim_end().to_owned(),
            text.replacen("id hello", "id  hello", 1),
            text.replacen(HEADER, "syrinx-update-manifest 1", 1),
            unsorted,
            format!("{text}\n"),
        ] {
            assert!(
                PackageList::parse(&variant).is_err(),
                "accepted {variant:?}"
            );
        }
    }

    #[test]
    fn unsafe_paths_are_refused() {
        for path in [
            "../x",
            "/x",
            "a//b",
            "a\\b",
            "note.md:hidden",
            "CON",
            "nul.txt",
            "a.",
            "a/./b",
            "",
            "ü.js",
        ] {
            assert!(check_path(path).is_err(), "accepted {path:?}");
        }
        assert!(check_path("ui/app.js").is_ok());
        assert!(check_path("console.js").is_ok());
    }

    #[test]
    fn case_and_file_folder_collisions_are_refused() {
        let case = list(&["UI/app.js", "ui/app.js"]).render();
        assert!(matches!(
            PackageList::parse(&case),
            Err(PackageError::Collision(_))
        ));
        let folder = list(&["ui", "ui/app.js"]).render();
        assert!(matches!(
            PackageList::parse(&folder),
            Err(PackageError::Collision(_))
        ));
    }

    #[test]
    fn a_folder_must_hold_exactly_the_listed_files() {
        let dir = std::env::temp_dir().join(format!("syrinx-package-test-{}", std::process::id()));
        if dir.exists() {
            fs::remove_dir_all(&dir).expect("remove an earlier run's folder");
        }
        fs::create_dir_all(dir.join("ui")).expect("make folder");
        fs::write(dir.join("plugin.wasm"), b"synthetic module").expect("write");
        fs::write(dir.join("ui/app.js"), b"// synthetic").expect("write");
        let built = PackageList::from_folder(&dir, "hello", "0.1.0", "w@0.1.0").expect("list");
        fs::write(dir.join(LIST_NAME), built.render()).expect("write list");
        assert!(built.verify_folder(&dir).is_ok());

        fs::write(dir.join("ui/app.js"), b"// changed").expect("write");
        assert!(matches!(
            built.verify_folder(&dir),
            Err(PackageError::Mismatch(_))
        ));
        fs::write(dir.join("ui/app.js"), b"// synthetic").expect("write");
        fs::write(dir.join("extra.dll"), b"x").expect("write");
        assert!(matches!(
            built.verify_folder(&dir),
            Err(PackageError::Unlisted(_))
        ));
        fs::remove_dir_all(&dir).expect("clean up");
    }
}
