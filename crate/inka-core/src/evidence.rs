//! Imported CI evidence. Producers and consumers share the v2 contract, while
//! execution and language-specific extraction remain outside evaluation.

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::hash::InterfaceHash;

/// Evidence inputs configured by the argument project.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceConfig {
    #[serde(default)]
    pub sources: BTreeMap<String, SourceConfig>,
}

/// A stable source identity and the selected local bundle.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceConfig {
    pub id: String,
    pub file: std::path::PathBuf,
}

/// One source revision and execution attempt, transported as JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Bundle {
    pub v: u32,
    pub source: Source,
    pub execution: Execution,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "environment_object"
    )]
    pub environment: Option<serde_json::Map<String, serde_json::Value>>,
    pub targets: Vec<Target>,
}

// Absence is allowed; an explicit null is not an environment object.
fn environment_object<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<serde_json::Map<String, serde_json::Value>>, D::Error> {
    serde_json::Map::deserialize(deserializer).map(Some)
}

/// Provenance of the source, not of the argument repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub id: String,
    pub revision: String,
}

/// The explicitly imported run/attempt; evaluation never searches for success.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Execution {
    pub id: String,
    pub attempt: u32,
}

/// Reviewable evidence content and its required execution results.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub id: String,
    pub kind: Kind,
    pub profile: String,
    pub content: String,
    pub interface_hash: String,
    pub required: Vec<String>,
    pub results: Vec<CaseResult>,
}

/// Extraction granularity; consumers do not parse the selected language.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    File,
    Function,
}

impl Kind {
    /// The canonical spelling included in the interface digest.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Function => "function",
        }
    }
}

/// An observed outcome for an explicitly required case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaseResult {
    pub case: String,
    pub result: ResultKind,
}

/// Only pass can establish evidence. Absence of a record means unreported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultKind {
    Pass,
    Fail,
    Skip,
    Cancelled,
}

impl std::fmt::Display for ResultKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Skip => "skip",
            Self::Cancelled => "cancelled",
        })
    }
}

/// A malformed or inconsistent input, rather than a non-passing test.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct EvidenceError(pub String);

fn nonblank(value: &str, field: &str) -> Result<(), EvidenceError> {
    if value.trim().is_empty() {
        Err(EvidenceError(format!("{field} must not be blank")))
    } else {
        Ok(())
    }
}

/// Whether a local evidence alias has the DSL identifier spelling.
#[must_use]
pub fn valid_alias(alias: &str) -> bool {
    let mut bytes = alias.bytes();
    bytes
        .next()
        .is_some_and(|b| b.is_ascii_alphabetic() || b == b'_')
        && bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

impl EvidenceConfig {
    /// Reject ambiguous routing before opening any evidence files.
    ///
    /// # Errors
    /// Returns an error for invalid aliases, blank fields or duplicate sources.
    pub fn validate(&self) -> Result<(), EvidenceError> {
        let mut ids = HashSet::new();
        for (alias, config) in &self.sources {
            if !valid_alias(alias) {
                return Err(EvidenceError(format!("invalid evidence alias `{alias}`")));
            }
            nonblank(&config.id, "evidence source id")?;
            if config.file.as_os_str().is_empty() {
                return Err(EvidenceError(format!(
                    "evidence source `{alias}` has an empty file"
                )));
            }
            if !ids.insert(&config.id) {
                return Err(EvidenceError(format!(
                    "duplicate evidence source `{}`",
                    config.id
                )));
            }
        }
        Ok(())
    }
}

impl Bundle {
    /// Parse and validate a complete v2 bundle. Serde rejects duplicate known
    /// object fields before validation, including duplicate version fields.
    ///
    /// # Errors
    /// Returns malformed JSON/schema errors or inconsistent evidence errors.
    pub fn parse(text: &str) -> Result<Self, EvidenceError> {
        let bundle: Self = serde_json::from_str(text)
            .map_err(|error| EvidenceError(format!("invalid evidence JSON: {error}")))?;
        bundle.validate()?;
        Ok(bundle)
    }

    /// Validate hand-built bundles as strictly as imported bundles.
    ///
    /// # Errors
    /// Rejects unsupported versions, duplicate targets and inconsistent content.
    pub fn validate(&self) -> Result<(), EvidenceError> {
        if self.v != 2 {
            return Err(EvidenceError(format!(
                "unsupported evidence version {} (expected 2)",
                self.v
            )));
        }
        nonblank(&self.source.id, "source.id")?;
        nonblank(&self.source.revision, "source.revision")?;
        nonblank(&self.execution.id, "execution.id")?;
        if self.execution.attempt == 0 {
            return Err(EvidenceError("execution.attempt must be positive".into()));
        }
        let mut ids = HashSet::new();
        for target in &self.targets {
            if !ids.insert(&target.id) {
                return Err(EvidenceError(format!(
                    "duplicate evidence target `{}`",
                    target.id
                )));
            }
            target.validate(&self.source.id)?;
        }
        Ok(())
    }
}

impl Target {
    /// Compute the v2 digest without trusting a producer's claimed hash.
    #[must_use]
    pub fn hash(&self, source: &str) -> InterfaceHash {
        fn field(hash: &mut Sha256, text: &str) {
            hash.update((text.len() as u64).to_le_bytes());
            hash.update(text.as_bytes());
        }
        let mut hash = Sha256::new();
        for text in [
            "inka:ci-interface:v2",
            source,
            &self.id,
            self.kind.name(),
            &self.profile,
            &self.content.replace("\r\n", "\n"),
        ] {
            field(&mut hash, text);
        }
        let mut required: Vec<&str> = self.required.iter().map(String::as_str).collect();
        required.sort_unstable();
        hash.update((required.len() as u64).to_le_bytes());
        for case in required {
            field(&mut hash, case);
        }
        InterfaceHash::from_bytes(hash.finalize().into())
    }

    /// Validate target shape, required cases and claimed content digest.
    ///
    /// # Errors
    /// Returns an error for inconsistent content or execution selection/results.
    pub fn validate(&self, source: &str) -> Result<(), EvidenceError> {
        nonblank(&self.id, "target.id")?;
        nonblank(&self.profile, "target.profile")?;
        if self.content.is_empty() || self.required.is_empty() {
            return Err(EvidenceError(format!(
                "target `{}` requires content and at least one case",
                self.id
            )));
        }
        let mut required = HashSet::new();
        for case in &self.required {
            nonblank(case, "required case")?;
            if !required.insert(case) {
                return Err(EvidenceError(format!("duplicate required case `{case}`")));
            }
        }
        let mut results = HashSet::new();
        for result in &self.results {
            if !required.contains(&result.case) || !results.insert(&result.case) {
                return Err(EvidenceError(format!(
                    "undeclared or duplicate result case `{}`",
                    result.case
                )));
            }
        }
        if self.interface_hash != self.hash(source).to_string() {
            return Err(EvidenceError(format!(
                "interface hash does not match delivered content for `{}`",
                self.id
            )));
        }
        Ok(())
    }

    /// Non-passing or absent required cases, in deterministic ID order.
    #[must_use]
    pub fn failures(&self) -> Vec<CaseFailure> {
        let results: BTreeMap<&str, ResultKind> = self
            .results
            .iter()
            .map(|result| (result.case.as_str(), result.result))
            .collect();
        let mut required: Vec<&String> = self.required.iter().collect();
        required.sort_unstable();
        required
            .into_iter()
            .filter_map(|case| {
                let result = results.get(case.as_str()).copied();
                (result != Some(ResultKind::Pass)).then(|| CaseFailure {
                    case: case.clone(),
                    result,
                })
            })
            .collect()
    }
}

/// An own evidence cause, preserving the distinction between missing and fail.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CaseFailure {
    pub case: String,
    pub result: Option<ResultKind>,
}

/// A reference could be interpreted but cannot resolve selected evidence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, thiserror::Error)]
#[serde(tag = "cause", rename_all = "snake_case")]
pub enum MissingEvidence {
    #[error("invalid CI reference `{reference}`: expected <alias>/<target-id>")]
    InvalidReference { reference: String },
    #[error("unknown evidence source alias `{alias}`")]
    UnknownSource { alias: String },
    #[error("no imported evidence bundle for `{alias}`")]
    MissingBundle { alias: String },
    #[error("evidence source `{alias}` has no target `{target}`")]
    MissingTarget { alias: String, target: String },
}

/// Validated imported inputs. Missing bundle files are retained as missing data.
#[derive(Debug, Clone, Default)]
pub struct EvidenceSet {
    sources: BTreeMap<String, (String, Option<Bundle>)>,
}

impl EvidenceSet {
    /// Insert an explicitly selected source, validating hand-built data too.
    ///
    /// # Errors
    /// Rejects invalid/duplicate aliases, duplicate sources or mismatched bundles.
    pub fn insert(
        &mut self,
        alias: String,
        source: String,
        bundle: Option<Bundle>,
    ) -> Result<(), EvidenceError> {
        nonblank(&source, "source identity")?;
        if !valid_alias(&alias)
            || self.sources.contains_key(&alias)
            || self.sources.values().any(|(id, _)| id == &source)
        {
            return Err(EvidenceError(format!(
                "invalid or duplicate evidence source `{alias}`"
            )));
        }
        if let Some(bundle) = &bundle {
            bundle.validate()?;
            if bundle.source.id != source {
                return Err(EvidenceError(format!(
                    "source identity mismatch for `{alias}`: expected `{source}`, got `{}`",
                    bundle.source.id
                )));
            }
        }
        self.sources.insert(alias, (source, bundle));
        Ok(())
    }

    /// Resolve a reference without executing, fetching or interpreting source.
    ///
    /// # Errors
    /// Returns the specific missing-evidence cause for the referring bind.
    pub fn resolve(&self, reference: &str) -> Result<(&Source, &Target), MissingEvidence> {
        let (alias, id) = reference
            .split_once('/')
            .filter(|(alias, id)| valid_alias(alias) && !id.trim().is_empty())
            .ok_or_else(|| MissingEvidence::InvalidReference {
                reference: reference.into(),
            })?;
        let (_, bundle) =
            self.sources
                .get(alias)
                .ok_or_else(|| MissingEvidence::UnknownSource {
                    alias: alias.into(),
                })?;
        let bundle = bundle
            .as_ref()
            .ok_or_else(|| MissingEvidence::MissingBundle {
                alias: alias.into(),
            })?;
        let target = bundle
            .targets
            .iter()
            .find(|target| target.id == id)
            .ok_or_else(|| MissingEvidence::MissingTarget {
                alias: alias.into(),
                target: id.into(),
            })?;
        Ok((&bundle.source, target))
    }
}

/// Load the project's selected bundles. Missing files become missing evidence;
/// unreadable, corrupt or inconsistent files abort input loading with their path.
///
/// # Errors
/// Returns a configuration, read, parse or validation error.
pub fn load_sources(
    project_dir: &Path,
    config: &EvidenceConfig,
) -> Result<EvidenceSet, EvidenceError> {
    config.validate()?;
    let mut set = EvidenceSet::default();
    for (alias, source) in &config.sources {
        let path = project_dir.join(&source.file);
        let bundle = match fs::read_to_string(&path) {
            Ok(text) => Some(
                Bundle::parse(&text)
                    .map_err(|error| EvidenceError(format!("{}: {error}", path.display())))?,
            ),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(EvidenceError(format!("{}: {error}", path.display()))),
        };
        set.insert(alias.clone(), source.id.clone(), bundle)?;
    }
    Ok(set)
}
