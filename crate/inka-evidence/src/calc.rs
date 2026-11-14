//! The calc evidence v1 wire format, shared by producers and the consumer.
//!
//! `docs/en/design/calc-evidence.md` fixes the contract and
//! `schemas/calc-evidence-v1.schema.json` the shape. As for test evidence,
//! one validation pass serves both directions: [`parse`] is the strict reader
//! inka's consumer builds on and [`CalcEvidenceBuilder`] the emission layer
//! the veriq adapter builds on, so an adapter cannot emit a file the consumer
//! rejects.
//!
//! A target is one verification of a calculation graph, carried with the
//! reviewable source of its whole definition-level upstream subgraph, the
//! argument nodes that subgraph assumes, and the leaf results the target must
//! satisfy. Whether the assumed IDs name real nodes, and what the interface
//! hash is, are the consumer's business; this module checks only what the
//! wire format itself promises.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::provenance::{self, Provenance};

/// The calc evidence format version this crate reads and writes.
pub const VERSION: u64 = 1;

/// Just the version stamp. Read on its own before the v1 shape is imposed,
/// so a newer file is refused for its version and not for the keys v1 lacks.
#[derive(Debug, Deserialize)]
struct Versioned {
    v: u64,
}

/// The file envelope: the version stamp, open provenance, and the targets.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct WireFile {
    #[expect(dead_code, reason = "admits the key; `Versioned` already read it")]
    v: u64,
    #[serde(default, deserialize_with = "present")]
    #[expect(
        dead_code,
        reason = "enforces the object shape; evaluation never reads it"
    )]
    provenance: Option<Provenance>,
    targets: Vec<Target>,
}

/// Reads an optional key that must hold a value when it is written: the
/// schema spells "missing" as an absent key, so `null` is an error.
fn present<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    T::deserialize(deserializer).map(Some)
}

/// One target as the wire carries it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    /// The target verification's path in the producer's graph, referenced
    /// by `calc:` in a bind (for veriq, `Scope::?name`).
    pub path: String,
    /// The definition-level upstream subgraph, the target itself included.
    pub definitions: Vec<Definition>,
    /// The inka node IDs the subgraph assumes, each once.
    pub assumes: Vec<String>,
    /// The required leaf results.
    pub leaves: Vec<Leaf>,
    /// Whether the producer's own validity conditions hold.
    pub valid: bool,
    /// Whether the producer marks the target as an expected failure.
    /// Emission omits the key when false, matching the schema's default.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub xfail: bool,
}

/// One definition of a target's upstream subgraph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    /// The definition's path in the producer's graph.
    pub path: String,
    /// What the definition is; presentation metadata.
    pub kind: DefinitionKind,
    /// The reviewable source string, CRLF/LF as produced.
    pub source: String,
}

impl Definition {
    #[must_use]
    pub fn new(path: impl Into<String>, kind: DefinitionKind, source: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            kind,
            source: source.into(),
        }
    }
}

/// The kinds of definition a calculation graph is made of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DefinitionKind {
    Model,
    Calculation,
    Verification,
}

impl DefinitionKind {
    /// The kind's one spelling: the JSON value and the word a reviewer sees.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            DefinitionKind::Model => "model",
            DefinitionKind::Calculation => "calculation",
            DefinitionKind::Verification => "verification",
        }
    }
}

/// One required leaf of a target and its observed result.
///
/// `result` is `None` when the leaf was declared but no result was reported.
/// A missing result is an absent key in both directions, never `null`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Leaf {
    pub id: String,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "present"
    )]
    pub result: Option<bool>,
}

impl Leaf {
    /// One required leaf; `None` records a declared-but-missing result.
    #[must_use]
    pub fn new(id: impl Into<String>, result: Option<bool>) -> Self {
        Self {
            id: id.into(),
            result,
        }
    }
}

/// A semantic defect of calc evidence the schema shape alone cannot express,
/// or that this crate enforces on both directions for the schema's sake.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidCalcEvidence {
    #[error("target path {path:?} is blank or contains whitespace")]
    InvalidTargetPath { path: String },
    #[error("target `{target}` declares no definitions")]
    NoDefinitions { target: String },
    #[error(
        "target `{target}` has a definition whose path {path:?} is blank or contains whitespace"
    )]
    InvalidDefinitionPath { target: String, path: String },
    #[error("target `{target}` declares definition `{path}` more than once")]
    DuplicateDefinition { target: String, path: String },
    #[error("target `{target}` has an empty source for definition `{path}`")]
    EmptySource { target: String, path: String },
    #[error("target `{target}` is not among its own definitions")]
    TargetNotDefined { target: String },
    #[error("target `{target}` assumes `{id}`, which is not an inka node id")]
    InvalidAssumptionId { target: String, id: String },
    #[error("target `{target}` assumes `{id}` more than once")]
    DuplicateAssumption { target: String, id: String },
    #[error("target `{target}` declares no leaves")]
    NoLeaves { target: String },
    #[error("target `{target}` has a leaf whose id {id:?} is blank or contains whitespace")]
    InvalidLeafId { target: String, id: String },
    #[error("target `{target}` declares leaf `{id}` more than once")]
    DuplicateLeaf { target: String, id: String },
    #[error("target path `{target}` appears more than once in the loaded evidence")]
    DuplicateTarget { target: String },
    #[error("provenance entry `{key}` has a blank key or value")]
    BlankProvenance { key: String },
}

/// A reason one calc evidence text could not be read as v1.
///
/// Path-free: which file carried the text is the caller's context.
#[derive(Debug, thiserror::Error)]
pub enum CalcParseError {
    #[error("not valid v{VERSION} JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("version {found}; this reader understands v{VERSION}")]
    UnsupportedVersion { found: u64 },
    #[error(transparent)]
    Invalid(#[from] InvalidCalcEvidence),
}

/// Reads one calc evidence file's text into its validated targets.
///
/// # Errors
///
/// Returns a [`CalcParseError`] when the text is not valid v1 JSON, declares
/// a version other than [`VERSION`], or fails a semantic check: see
/// [`InvalidCalcEvidence`] for the rules.
pub fn parse(text: &str) -> Result<Vec<Target>, CalcParseError> {
    let Versioned { v } = serde_json::from_str(text)?;
    if v != VERSION {
        return Err(CalcParseError::UnsupportedVersion { found: v });
    }
    let wire: WireFile = serde_json::from_str(text)?;

    let mut seen = BTreeSet::new();
    for target in &wire.targets {
        check(target)?;
        if !seen.insert(target.path.as_str()) {
            return Err(InvalidCalcEvidence::DuplicateTarget {
                target: target.path.clone(),
            }
            .into());
        }
    }
    Ok(wire.targets)
}

/// Whether `text` is a path the schema accepts: nonempty, with no character
/// of the Unicode `White_Space` property (the set the schema's patterns list).
fn is_path(text: &str) -> bool {
    !text.is_empty() && !text.chars().any(char::is_whitespace)
}

/// Whether `text` is an inka identifier: `[A-Za-z_][A-Za-z0-9_-]*`.
fn is_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Whether `text` is a fully-qualified inka node id: identifiers joined by
/// `::`, at least one.
fn is_node_id(text: &str) -> bool {
    text.split("::").all(is_identifier)
}

/// Checks one target against the rules both directions share.
pub(crate) fn check(target: &Target) -> Result<(), InvalidCalcEvidence> {
    if !is_path(&target.path) {
        return Err(InvalidCalcEvidence::InvalidTargetPath {
            path: target.path.clone(),
        });
    }
    let name = || target.path.clone();

    if target.definitions.is_empty() {
        return Err(InvalidCalcEvidence::NoDefinitions { target: name() });
    }
    let mut paths = BTreeSet::new();
    for definition in &target.definitions {
        if !is_path(&definition.path) {
            return Err(InvalidCalcEvidence::InvalidDefinitionPath {
                target: name(),
                path: definition.path.clone(),
            });
        }
        if !paths.insert(definition.path.as_str()) {
            return Err(InvalidCalcEvidence::DuplicateDefinition {
                target: name(),
                path: definition.path.clone(),
            });
        }
        if definition.source.is_empty() {
            return Err(InvalidCalcEvidence::EmptySource {
                target: name(),
                path: definition.path.clone(),
            });
        }
    }
    if !paths.contains(target.path.as_str()) {
        return Err(InvalidCalcEvidence::TargetNotDefined { target: name() });
    }

    let mut assumed = BTreeSet::new();
    for id in &target.assumes {
        if !is_node_id(id) {
            return Err(InvalidCalcEvidence::InvalidAssumptionId {
                target: name(),
                id: id.clone(),
            });
        }
        if !assumed.insert(id.as_str()) {
            return Err(InvalidCalcEvidence::DuplicateAssumption {
                target: name(),
                id: id.clone(),
            });
        }
    }

    if target.leaves.is_empty() {
        return Err(InvalidCalcEvidence::NoLeaves { target: name() });
    }
    let mut leaves = BTreeSet::new();
    for leaf in &target.leaves {
        if !is_path(&leaf.id) {
            return Err(InvalidCalcEvidence::InvalidLeafId {
                target: name(),
                id: leaf.id.clone(),
            });
        }
        if !leaves.insert(leaf.id.as_str()) {
            return Err(InvalidCalcEvidence::DuplicateLeaf {
                target: name(),
                id: leaf.id.clone(),
            });
        }
    }
    Ok(())
}

/// Assembles one calc evidence v1 file, validating as it goes.
///
/// Every target is checked at [`CalcEvidenceBuilder::add_target`] time with
/// the same pass [`parse`] uses, so nothing invalid accumulates and whatever
/// [`CalcEvidenceBuilder::finish`] returns is guaranteed to load. The builder
/// never touches the filesystem and never computes hashes.
#[derive(Debug, Default)]
pub struct CalcEvidenceBuilder {
    provenance: Provenance,
    targets: Vec<Target>,
    paths: BTreeSet<String>,
}

/// The envelope [`CalcEvidenceBuilder::finish`] serializes.
#[derive(Serialize)]
struct WireFileRef<'a> {
    v: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    provenance: Option<&'a Provenance>,
    targets: &'a [Target],
}

impl CalcEvidenceBuilder {
    /// An empty file: no provenance, no targets.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one descriptive provenance entry, replacing an earlier value
    /// under the same key.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidCalcEvidence::BlankProvenance`] when the key or the
    /// value is blank.
    pub fn provenance(
        &mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<(), InvalidCalcEvidence> {
        let key = key.into();
        let value = value.into();
        if !provenance::record(&mut self.provenance, &key, value) {
            return Err(InvalidCalcEvidence::BlankProvenance { key });
        }
        Ok(())
    }

    /// Fills `capsula_run_id` from the `CAPSULA_RUN_ID` environment variable
    /// that `capsula run` exports, when it is set and not already recorded.
    pub fn capsula_env(&mut self) {
        provenance::capsula_env(&mut self.provenance);
    }

    /// Fills `capsula_run_id` with `run_id`, unless it is absent or blank or
    /// an explicit provenance entry already claimed the key.
    pub fn capsula_run_id(&mut self, run_id: Option<String>) {
        provenance::capsula_run_id(&mut self.provenance, run_id);
    }

    /// Records one target.
    ///
    /// # Errors
    ///
    /// Returns an [`InvalidCalcEvidence`] — leaving the builder unchanged —
    /// when the target fails the shared checks or its path repeats across
    /// the file.
    pub fn add_target(&mut self, target: Target) -> Result<(), InvalidCalcEvidence> {
        check(&target)?;
        if self.paths.contains(&target.path) {
            return Err(InvalidCalcEvidence::DuplicateTarget {
                target: target.path,
            });
        }
        self.paths.insert(target.path.clone());
        self.targets.push(target);
        Ok(())
    }

    /// Serializes the file: pretty-printed v1 JSON ending in a newline,
    /// guaranteed to be accepted by [`parse`].
    ///
    /// # Panics
    ///
    /// Never in practice: the builder holds only strings, booleans and unit
    /// enums, which `serde_json` serializes infallibly.
    #[must_use]
    pub fn finish(&self) -> String {
        let wire = WireFileRef {
            v: VERSION,
            provenance: (!self.provenance.is_empty()).then_some(&self.provenance),
            targets: &self.targets,
        };
        let mut json = serde_json::to_string_pretty(&wire)
            .expect("the builder only holds serializable strings, booleans and enums");
        json.push('\n');
        json
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_ids_follow_the_identifier_grammar() {
        for valid in ["x", "_x", "power::eclipse_model_valid", "a-b::c_d::E9"] {
            assert!(is_node_id(valid), "{valid}");
        }
        for invalid in ["", "9x", "a b", "power::", "::a", "a.b", "a::b::"] {
            assert!(!is_node_id(invalid), "{invalid}");
        }
    }

    #[test]
    fn a_version_other_than_one_is_rejected_by_name() {
        let error = parse(r#"{"v": 2, "targets": []}"#).expect_err("v2 is not this format");

        assert!(matches!(
            error,
            CalcParseError::UnsupportedVersion { found: 2 }
        ));
    }

    #[test]
    fn a_newer_version_is_named_before_its_shape_is_judged() {
        // A v2 file may carry keys v1 does not know; the reader must say
        // "v2", not "unknown field".
        let text = r#"{"v": 2, "digest": "x", "targets": [{"shape": "new"}]}"#;

        let error = parse(text).expect_err("v2 is not this format");

        assert!(matches!(
            error,
            CalcParseError::UnsupportedVersion { found: 2 }
        ));
    }

    #[test]
    fn a_null_result_is_rejected_as_the_schema_rejects_it() {
        // A missing result is an absent key; `null` is not a boolean.
        let text = r#"{"v": 1, "targets": [{"path": "P::?v",
            "definitions": [{"path": "P::?v", "kind": "verification", "source": "s"}],
            "assumes": [], "leaves": [{"id": "P::?v", "result": null}], "valid": true}]}"#;

        assert!(matches!(parse(text), Err(CalcParseError::Json(_))));
    }

    #[test]
    fn a_null_provenance_is_rejected_as_the_schema_rejects_it() {
        let text = r#"{"v": 1, "provenance": null, "targets": []}"#;

        assert!(matches!(parse(text), Err(CalcParseError::Json(_))));
    }

    #[test]
    fn an_invalid_path_or_leaf_id_is_named_with_its_whitespace_visible() {
        let target = |path: &str, definition: &str, leaf: &str| Target {
            path: path.to_owned(),
            definitions: vec![Definition::new(
                definition,
                DefinitionKind::Verification,
                "s",
            )],
            assumes: Vec::new(),
            leaves: vec![Leaf::new(leaf, Some(true))],
            valid: true,
            xfail: false,
        };

        let messages = [
            target("P::\t?v", "P::\t?v", "P::?v"),
            target("P::?v", "P::\u{a0}?v", "P::?v"),
            target("P::?v", "P::?v", "P::?v .cell"),
        ]
        .map(|target| check(&target).expect_err("invalid").to_string());

        assert!(messages[0].contains(r#""P::\t?v""#), "{}", messages[0]);
        assert!(messages[1].contains(r#""P::\u{a0}?v""#), "{}", messages[1]);
        assert!(messages[2].contains(r#""P::?v .cell""#), "{}", messages[2]);
    }

    #[test]
    fn unknown_fields_outside_provenance_are_rejected() {
        let text = r#"{"v": 1, "targets": [{"path": "P::?v", "digest": "x",
            "definitions": [{"path": "P::?v", "kind": "verification", "source": "s"}],
            "assumes": [], "leaves": [{"id": "P::?v"}], "valid": true}]}"#;

        assert!(matches!(parse(text), Err(CalcParseError::Json(_))));
    }

    #[test]
    fn provenance_accepts_unknown_keys() {
        let text = r#"{"v": 1, "provenance": {"anything": {"nested": true}}, "targets": []}"#;

        assert_eq!(parse(text).expect("open provenance keys parse"), []);
    }
}
