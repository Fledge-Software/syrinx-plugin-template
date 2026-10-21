// SPDX-License-Identifier: MPL-2.0
//! Reads `syrinx-plugin.toml` and refuses it with a named error, as Syrinx's host will.

use std::collections::BTreeSet;
use std::fmt;

use serde::Deserialize;

/// The only manifest format this crate understands.
pub const SCHEMA: u32 = 1;

/// The WIT worlds a plugin may target, current track first.
pub const SUPPORTED_WORLDS: &[&str] = &["syrinx:plugin/plugin@0.1.0"];

/// Capability kinds the note-10 design names. Unknown kinds are refused.
const KNOWN_KINDS: &[&str] = &[
    "storage",
    "folder",
    "drafts",
    "index",
    "open-external",
    "network",
    "parse",
    "terminal",
    "git",
    "language-servers",
];

/// Kinds that run programs as the owner; the sandbox cannot limit them.
const FULL_TRUST_KINDS: &[&str] = &["terminal", "git", "language-servers"];

/// The longest id or contributed id the host accepts.
const MAX_ID_LEN: usize = 64;

/// The whole manifest, exactly as written; unknown fields are refused.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema: u32,
    pub plugin: PluginInfo,
    pub interface: Interface,
    pub entry: Entry,
    #[serde(default)]
    pub contributes: Contributes,
    #[serde(default)]
    pub capabilities: Vec<CapabilityRequest>,
}

/// Who the plugin is.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    pub authors: Vec<String>,
    pub repository: String,
    pub licence: String,
    pub min_syrinx: String,
}

/// Which WIT world the component is built against.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Interface {
    pub world: String,
}

/// Where the module and interface files are, and when to start the plugin.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub module: String,
    pub ui: Option<String>,
    #[serde(default)]
    pub activation: Vec<String>,
}

/// Everything the palette, tabs and search need before the plugin runs.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Contributes {
    #[serde(default)]
    pub commands: Vec<Command>,
    #[serde(default)]
    pub tab_types: Vec<TabType>,
    #[serde(default)]
    pub search_feeds: Vec<SearchFeed>,
}

/// One palette command.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Command {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub form: bool,
}

/// One kind of tab the plugin draws in its own view.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct TabType {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub ui_methods: Vec<String>,
}

/// One search feed the host pulls from.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchFeed {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub tiers: Vec<String>,
}

/// One capability request, with the reason the owner reads.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityRequest {
    pub kind: String,
    pub reason: String,
    pub required: bool,
    pub trust: Option<String>,
    pub access: Option<String>,
    pub count: Option<toml::Value>,
    pub watch: Option<bool>,
    pub feeds: Option<Vec<String>>,
}

/// Why a manifest was refused. Each names what to fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManifestError {
    Syntax(String),
    UnsupportedSchema(u32),
    UnsupportedWorld(String),
    BadPluginId(String),
    BadVersion(String),
    BadContributedId(String),
    WrongPrefix { id: String, plugin: String },
    DuplicateId(String),
    BadMethod(String),
    UnknownCapability(String),
    MissingReason(String),
    FullTrustUndeclared(String),
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax(detail) => write!(f, "the manifest is not valid: {detail}"),
            Self::UnsupportedSchema(found) => {
                write!(
                    f,
                    "manifest schema {found} is not supported (expected {SCHEMA})"
                )
            }
            Self::UnsupportedWorld(world) => write!(f, "world {world:?} is not supported"),
            Self::BadPluginId(id) => write!(
                f,
                "plugin id {id:?} must be lowercase letters, digits and hyphens, starting with a letter"
            ),
            Self::BadVersion(version) => {
                write!(f, "version {version:?} is not MAJOR.MINOR.PATCH")
            }
            Self::BadContributedId(id) => write!(
                f,
                "id {id:?} must be dot-separated segments of lowercase letters, digits and hyphens"
            ),
            Self::WrongPrefix { id, plugin } => {
                write!(f, "id {id:?} must start with the plugin id {plugin:?}")
            }
            Self::DuplicateId(id) => write!(f, "id {id:?} is declared twice"),
            Self::BadMethod(method) => write!(
                f,
                "view method {method:?} must be lowercase letters, digits and hyphens"
            ),
            Self::UnknownCapability(kind) => write!(f, "capability kind {kind:?} is unknown"),
            Self::MissingReason(kind) => {
                write!(f, "capability {kind:?} needs a reason the owner can read")
            }
            Self::FullTrustUndeclared(kind) => write!(
                f,
                "capability {kind:?} runs programs as the owner and must say trust = \"full\""
            ),
        }
    }
}

impl std::error::Error for ManifestError {}

impl Manifest {
    /// Parse and check a manifest; nothing invalid gets past this.
    pub fn parse(text: &str) -> Result<Self, ManifestError> {
        let manifest: Manifest =
            toml::from_str(text).map_err(|error| ManifestError::Syntax(error.to_string()))?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// The world's version, the part after `@` (for example `0.1.0`).
    pub fn world_version(&self) -> &str {
        self.interface
            .world
            .rsplit_once('@')
            .map_or("", |(_, version)| version)
    }

    /// Whether the manifest asks for a capability of this kind.
    pub fn requests(&self, kind: &str) -> bool {
        self.capabilities.iter().any(|request| request.kind == kind)
    }

    /// The tab type with this id, if declared.
    pub fn tab_type(&self, id: &str) -> Option<&TabType> {
        self.contributes.tab_types.iter().find(|tab| tab.id == id)
    }

    /// The command with this id, if declared.
    pub fn command(&self, id: &str) -> Option<&Command> {
        self.contributes
            .commands
            .iter()
            .find(|command| command.id == id)
    }

    fn validate(&self) -> Result<(), ManifestError> {
        if self.schema != SCHEMA {
            return Err(ManifestError::UnsupportedSchema(self.schema));
        }
        if !SUPPORTED_WORLDS.contains(&self.interface.world.as_str()) {
            return Err(ManifestError::UnsupportedWorld(
                self.interface.world.clone(),
            ));
        }
        if !is_plugin_id(&self.plugin.id) {
            return Err(ManifestError::BadPluginId(self.plugin.id.clone()));
        }
        if !is_version(&self.plugin.version) {
            return Err(ManifestError::BadVersion(self.plugin.version.clone()));
        }
        self.validate_contributed_ids()?;
        self.validate_methods()?;
        self.capabilities.iter().try_for_each(validate_capability)
    }

    /// Every contributed id is well formed, prefixed and unique.
    fn validate_contributed_ids(&self) -> Result<(), ManifestError> {
        let contributes = &self.contributes;
        let ids = contributes
            .commands
            .iter()
            .map(|command| &command.id)
            .chain(contributes.tab_types.iter().map(|tab| &tab.id))
            .chain(contributes.search_feeds.iter().map(|feed| &feed.id));
        let mut seen = BTreeSet::new();
        for id in ids {
            self.validate_contributed_id(id)?;
            if !seen.insert(id.as_str()) {
                return Err(ManifestError::DuplicateId(id.clone()));
            }
        }
        Ok(())
    }

    fn validate_contributed_id(&self, id: &str) -> Result<(), ManifestError> {
        if !is_dotted_id(id) {
            return Err(ManifestError::BadContributedId(id.to_owned()));
        }
        let first_segment = id.split('.').next().unwrap_or_default();
        if first_segment != self.plugin.id {
            return Err(ManifestError::WrongPrefix {
                id: id.to_owned(),
                plugin: self.plugin.id.clone(),
            });
        }
        Ok(())
    }

    /// View methods are plain kebab-case names.
    fn validate_methods(&self) -> Result<(), ManifestError> {
        let methods = self
            .contributes
            .tab_types
            .iter()
            .flat_map(|tab| tab.ui_methods.iter());
        for method in methods {
            if !is_segment(method) {
                return Err(ManifestError::BadMethod(method.clone()));
            }
        }
        Ok(())
    }
}

/// A request names a known kind, says why, and admits full trust.
fn validate_capability(request: &CapabilityRequest) -> Result<(), ManifestError> {
    let kind = request.kind.as_str();
    if !KNOWN_KINDS.contains(&kind) {
        return Err(ManifestError::UnknownCapability(kind.to_owned()));
    }
    if request.reason.trim().is_empty() {
        return Err(ManifestError::MissingReason(kind.to_owned()));
    }
    let says_full = request.trust.as_deref() == Some("full");
    if FULL_TRUST_KINDS.contains(&kind) && !says_full {
        return Err(ManifestError::FullTrustUndeclared(kind.to_owned()));
    }
    Ok(())
}

/// Lowercase letters, digits and hyphens, starting with a letter.
fn is_plugin_id(id: &str) -> bool {
    id.len() <= MAX_ID_LEN && id.starts_with(|c: char| c.is_ascii_lowercase()) && is_segment(id)
}

/// One segment of a dotted id.
fn is_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// Dot-separated segments, as Syrinx's `CommandId` expects.
fn is_dotted_id(id: &str) -> bool {
    id.len() <= MAX_ID_LEN && id.split('.').all(is_segment)
}

/// Exactly three dot-separated decimal numbers without leading zeros.
fn is_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|part| {
            !part.is_empty()
                && part.chars().all(|c| c.is_ascii_digit())
                && (part.len() == 1 || !part.starts_with('0'))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEMPLATE: &str = include_str!("../../syrinx-plugin.toml");

    fn with(find: &str, replace: &str) -> Result<Manifest, ManifestError> {
        assert!(TEMPLATE.contains(find), "fixture text {find:?} is missing");
        Manifest::parse(&TEMPLATE.replacen(find, replace, 1))
    }

    #[test]
    fn the_template_manifest_is_valid_and_requests_nothing() {
        let manifest = Manifest::parse(TEMPLATE).expect("template manifest");
        assert_eq!(manifest.plugin.id, "hello");
        assert_eq!(manifest.world_version(), "0.1.0");
        assert!(manifest.capabilities.is_empty());
        assert!(manifest.command("hello.greet").is_some_and(|c| c.form));
        assert!(manifest.tab_type("hello.greetings").is_some());
    }

    #[test]
    fn the_commented_storage_request_is_valid_once_uncommented() {
        let uncommented = TEMPLATE
            .replace("# [[capabilities]]", "[[capabilities]]")
            .replace("# kind = ", "kind = ")
            .replace("# reason = ", "reason = ")
            .replace("# required = ", "required = ");
        let manifest = Manifest::parse(&uncommented).expect("manifest with storage");
        assert!(manifest.requests("storage"));
    }

    #[test]
    fn a_command_without_the_plugin_prefix_is_refused() {
        let error = with("id = \"hello.greet\"", "id = \"other.greet\"").unwrap_err();
        assert!(matches!(error, ManifestError::WrongPrefix { .. }));
    }

    #[test]
    fn a_duplicate_contributed_id_is_refused() {
        let error = with("id = \"hello.greetings\"", "id = \"hello.greet\"").unwrap_err();
        assert_eq!(error, ManifestError::DuplicateId("hello.greet".into()));
    }

    #[test]
    fn an_unsupported_world_is_refused() {
        let error = with("plugin/plugin@0.1.0", "plugin/plugin@0.2.0").unwrap_err();
        assert!(matches!(error, ManifestError::UnsupportedWorld(_)));
    }

    #[test]
    fn an_unknown_field_is_refused() {
        let error = with("schema = 1", "schema = 1\nsurprise = true").unwrap_err();
        assert!(matches!(error, ManifestError::Syntax(_)));
    }

    #[test]
    fn a_capability_without_a_reason_or_full_trust_is_refused() {
        let base = format!("{TEMPLATE}\n[[capabilities]]\nkind = \"terminal\"\nrequired = true\n");
        let missing = Manifest::parse(&format!("{base}reason = \" \"\n")).unwrap_err();
        assert_eq!(missing, ManifestError::MissingReason("terminal".into()));
        let undeclared = Manifest::parse(&format!("{base}reason = \"Run tests.\"\n")).unwrap_err();
        assert_eq!(
            undeclared,
            ManifestError::FullTrustUndeclared("terminal".into())
        );
        let declared = format!("{base}reason = \"Run tests.\"\ntrust = \"full\"\n");
        assert!(Manifest::parse(&declared).is_ok());
    }

    #[test]
    fn ids_and_versions_follow_their_rules() {
        assert!(is_plugin_id("notes-2"));
        assert!(!is_plugin_id("2notes"));
        assert!(!is_plugin_id("Notes"));
        assert!(is_dotted_id("notes.new-note"));
        assert!(!is_dotted_id("notes..x"));
        assert!(is_version("10.0.1"));
        assert!(!is_version("01.0.0"));
        assert!(!is_version("1.0"));
    }
}
