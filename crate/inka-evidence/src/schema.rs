//! The serde mirror of `schemas/test-evidence-v1.schema.json`, plus the
//! semantic checks the schema shape alone cannot express.
//!
//! [`parse`] is strict: unknown fields are rejected everywhere except inside
//! `provenance`, whose keys are explicitly open descriptive metadata that the
//! consumer never reads. The same [`check`] pass guards emission, so the two
//! directions cannot drift apart.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// The evidence format version this crate reads and writes.
pub const VERSION: u64 = 1;

/// The file envelope: the version stamp, open provenance, and the tests.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WireFile {
    pub(crate) v: u64,
    /// Kept only so an object-shaped `provenance` deserializes; never read.
    #[serde(default)]
    #[allow(dead_code)]
    pub(crate) provenance: Option<serde_json::Map<String, serde_json::Value>>,
    pub(crate) tests: Vec<Test>,
}

/// One test as the wire carries it: the opaque id a bind references, the
/// reviewable contract body, and the declared required executions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Test {
    pub id: String,
    pub body: String,
    pub executions: Vec<Execution>,
}

/// One declared execution and its observed result.
///
/// `case` identifies the execution within its test; `outcome` is `None` when
/// the execution was declared but no result was reported — a missing result,
/// which never establishes a bind. Emission writes a missing result as an
/// absent key, never as `null`, matching the schema's enum.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Execution {
    pub case: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<Outcome>,
}

/// The outcome vocabulary of one required execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Pass,
    Fail,
    Skip,
    Cancelled,
}

/// A semantic defect the schema shape alone cannot express.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum InvalidEvidence {
    #[error("a test id is blank")]
    BlankTestId,
    #[error("test `{test_id}` has an empty contract body")]
    EmptyBody { test_id: String },
    #[error("test `{test_id}` declares no executions")]
    NoExecutions { test_id: String },
    #[error("test `{test_id}` contains a blank case id")]
    BlankCase { test_id: String },
    #[error("test `{test_id}` declares case `{case}` more than once")]
    DuplicateCase { test_id: String, case: String },
    #[error("test id `{test_id}` appears more than once in the loaded evidence")]
    DuplicateTest { test_id: String },
}

/// A reason one evidence text could not be read as v1.
///
/// Path-free: which file carried the text is the caller's context, and the
/// consumer in `inka-core` wraps each variant with it.
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("not valid v{VERSION} JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("version {found}; this reader understands v{VERSION}")]
    UnsupportedVersion { found: u64 },
    #[error(transparent)]
    Invalid(#[from] InvalidEvidence),
}

/// Reads one evidence file's text into its validated tests.
///
/// # Errors
///
/// Returns a [`ParseError`] when the text is not valid v1 JSON, declares a
/// version other than [`VERSION`], or fails a semantic check: a blank test
/// or case id, an empty contract body, an empty execution list, or a
/// duplicate case or test id within the file.
pub fn parse(text: &str) -> Result<Vec<Test>, ParseError> {
    let wire: WireFile = serde_json::from_str(text)?;
    if wire.v != VERSION {
        return Err(ParseError::UnsupportedVersion { found: wire.v });
    }

    let mut seen = BTreeSet::new();
    for test in &wire.tests {
        check(&test.id, &test.body, &test.executions)?;
        if !seen.insert(test.id.as_str()) {
            return Err(InvalidEvidence::DuplicateTest {
                test_id: test.id.clone(),
            }
            .into());
        }
    }
    Ok(wire.tests)
}

/// Checks one test against the rules both directions share.
pub(crate) fn check(id: &str, body: &str, executions: &[Execution]) -> Result<(), InvalidEvidence> {
    if id.trim().is_empty() {
        return Err(InvalidEvidence::BlankTestId);
    }
    let test_id = || id.to_owned();
    if body.is_empty() {
        return Err(InvalidEvidence::EmptyBody { test_id: test_id() });
    }
    if executions.is_empty() {
        return Err(InvalidEvidence::NoExecutions { test_id: test_id() });
    }

    let mut seen = BTreeSet::new();
    for execution in executions {
        if execution.case.trim().is_empty() {
            return Err(InvalidEvidence::BlankCase { test_id: test_id() });
        }
        if !seen.insert(execution.case.as_str()) {
            return Err(InvalidEvidence::DuplicateCase {
                test_id: test_id(),
                case: execution.case.clone(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_version_other_than_one_is_rejected_by_name() {
        let error = parse(r#"{"v": 2, "tests": []}"#).expect_err("v2 is not this format");

        assert!(matches!(error, ParseError::UnsupportedVersion { found: 2 }));
    }

    #[test]
    fn a_duplicate_test_id_within_one_file_is_rejected() {
        let text = r#"{"v": 1, "tests": [
            {"id": "t", "body": "b", "executions": [{"case": "e"}]},
            {"id": "t", "body": "b", "executions": [{"case": "e"}]}]}"#;

        let error = parse(text).expect_err("a duplicate id is a routing conflict");

        assert!(matches!(
            error,
            ParseError::Invalid(InvalidEvidence::DuplicateTest { .. })
        ));
    }

    #[test]
    fn unknown_fields_outside_provenance_are_rejected() {
        let text = r#"{"v": 1, "tests": [
            {"id": "t", "body": "b", "digest": "sha256:00", "executions": [{"case": "e"}]}]}"#;

        assert!(matches!(parse(text), Err(ParseError::Json(_))));
    }

    #[test]
    fn provenance_accepts_unknown_keys() {
        let text = r#"{"v": 1, "provenance": {"anything": {"nested": true}}, "tests": []}"#;

        assert_eq!(parse(text).expect("open provenance keys parse"), []);
    }
}
