//! Imported test evidence: the consumer side of the test evidence contract.
//!
//! The wire format, its validation and the emission layer live in the shared
//! `inka-evidence` crate, so a producer cannot emit what this consumer
//! rejects. This module adds what only the consumer needs: reading the files
//! an `inka.toml` `[evidence]` section names, anchoring every defect to the
//! file that carried it, and the interface hash each bind approval is bound
//! to.
//!
//! Loading is offline and content-only: no source checkout, no language
//! parsing, no network. A configured file that is missing or malformed is a
//! loud input error — the delivery premise is broken — while a test-id that
//! is simply absent from valid evidence stays a per-bind "missing evidence"
//! outcome, which evaluation reports as an unestablished bind, not an abort.
//!
//! The evidence JSON carries no declared digest. The interface hash is
//! computed here, over the test-id, the CRLF→LF-normalized contract body and
//! the bytewise-sorted required case IDs, framed with the same
//! `field`/`list` rules as the frozen v1 view hash under its own domain tag.
//! The layout is frozen by the vectors under
//! `tests/golden/hash/test-evidence-v1/`.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use inka_evidence::{ParseError, VERSION};
use sha2::{Digest, Sha256};

pub use inka_evidence::{Execution, InvalidEvidence, Outcome};

use crate::hash::{InterfaceHash, Preimage};
use crate::normalize;
use crate::read::ReadText;

/// The domain tag framing every test-evidence interface preimage.
const DOMAIN_TAG: &str = "inka:test-evidence:v1";

/// One test's imported evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestEvidence {
    /// The reviewable contract body, exactly as delivered.
    pub body: String,
    /// The declared required executions, in delivery order.
    pub executions: Vec<Execution>,
    /// The hash the bind approval is bound to.
    pub interface_hash: InterfaceHash,
}

impl TestEvidence {
    /// Whether this evidence can establish a bind: every required execution
    /// reported `pass`. A fail, skip, cancelled or missing result answers no.
    #[must_use]
    pub fn establishes(&self) -> bool {
        self.executions
            .iter()
            .all(|execution| execution.outcome == Some(Outcome::Pass))
    }
}

/// Every test loaded from the configured evidence files, keyed by test-id.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EvidenceSet {
    tests: BTreeMap<String, TestEvidence>,
}

impl EvidenceSet {
    /// The evidence for one test-id, or `None` when the loaded evidence does
    /// not mention it — evaluation's "missing evidence".
    #[must_use]
    pub fn get(&self, test_id: &str) -> Option<&TestEvidence> {
        self.tests.get(test_id)
    }

    /// Every loaded test, ordered by test-id.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &TestEvidence)> {
        self.tests.iter().map(|(id, test)| (id.as_str(), test))
    }
}

/// Computes the frozen v1 test-evidence interface hash.
///
/// ```text
/// preimage = field("inka:test-evidence:v1") ‖ field(test-id)
///            ‖ field(normalized body) ‖ list(sorted case IDs)
/// interface_hash = SHA-256(preimage)
/// ```
///
/// The body is normalized CRLF→LF only; a lone CR, indentation and trailing
/// whitespace stay content. Case IDs are sorted bytewise, so reordering
/// the declared executions is not an edit; adding, removing or renaming one
/// is. Outcomes never enter the hash.
///
/// `case_ids` must not contain duplicates; [`load_files`] validates that
/// before hashing. A duplicate would be framed twice and silently widen the
/// preimage instead of erroring.
#[must_use]
pub fn interface_hash(test_id: &str, body: &str, case_ids: &[&str]) -> InterfaceHash {
    let normalized = normalize::evidence_body(body);
    let mut ids: Vec<&[u8]> = case_ids.iter().map(|id| id.as_bytes()).collect();
    ids.sort_unstable();

    let mut preimage = Preimage::default();
    preimage.field(DOMAIN_TAG.as_bytes());
    preimage.field(test_id.as_bytes());
    preimage.field(normalized.as_bytes());
    preimage.list(&ids);
    InterfaceHash::from_bytes(Sha256::digest(preimage.finish()).into())
}

/// A reason loading the configured evidence failed.
///
/// Every variant is a CLI input error: the configured delivery premise does
/// not hold. None of them is the per-bind "missing evidence" outcome.
#[derive(Debug, thiserror::Error)]
pub enum EvidenceError {
    #[error("cannot read test evidence file `{}`: {source}", path.display())]
    Unreadable {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error(
        "test evidence file `{}` is not valid v{VERSION} JSON: {source}",
        path.display()
    )]
    Malformed {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error(
        "test evidence file `{}` has version {found}; this build reads v{VERSION}",
        path.display()
    )]
    UnsupportedVersion { path: PathBuf, found: u64 },
    #[error("invalid test evidence in `{}`: {reason}", path.display())]
    Invalid {
        path: PathBuf,
        #[source]
        reason: InvalidEvidence,
    },
}

impl EvidenceError {
    /// Anchors a path-free parse defect to the file that carried it.
    fn of_file(error: ParseError, path: PathBuf) -> Self {
        match error {
            ParseError::Json(source) => Self::Malformed { path, source },
            ParseError::UnsupportedVersion { found } => Self::UnsupportedVersion { path, found },
            ParseError::Invalid(reason) => Self::Invalid { path, reason },
        }
    }
}

/// Reads and validates every configured evidence file into one set.
///
/// `files` are manifest-relative paths, joined onto `base_dir` the same way
/// `values.files` are. Test-ids must be unique across the whole set: two
/// files delivering the same id is a routing conflict, not a merge.
///
/// # Errors
///
/// Returns an [`EvidenceError`] when a file cannot be read, is not valid v1
/// JSON, declares a version other than 1, or fails a semantic check: a blank
/// test or case id, an empty contract body, an empty execution list, or a
/// duplicate case or test-id.
pub fn load_files(
    base_dir: &Path,
    files: &[PathBuf],
    reader: &dyn ReadText,
) -> Result<EvidenceSet, EvidenceError> {
    let mut tests = BTreeMap::new();
    for relative in files {
        let path = base_dir.join(relative);
        let text = reader
            .read_text(&path)
            .map_err(|source| EvidenceError::Unreadable {
                path: path.clone(),
                source,
            })?;
        let parsed = inka_evidence::parse(&text)
            .map_err(|error| EvidenceError::of_file(error, path.clone()))?;
        for test in parsed {
            if tests.contains_key(&test.id) {
                return Err(EvidenceError::Invalid {
                    path,
                    reason: InvalidEvidence::DuplicateTest { test_id: test.id },
                });
            }
            let ids: Vec<&str> = test
                .executions
                .iter()
                .map(|execution| execution.case.as_str())
                .collect();
            let interface_hash = interface_hash(&test.id, &test.body, &ids);
            tests.insert(
                test.id,
                TestEvidence {
                    body: test.body,
                    executions: test.executions,
                    interface_hash,
                },
            );
        }
    }
    Ok(EvidenceSet { tests })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recomputing_the_interface_hash_is_deterministic() {
        // Arrange & Act
        let hash = interface_hash("id", "body", &["e"]);

        // Assert
        assert_eq!(interface_hash("id", "body", &["e"]), hash);
        assert_ne!(hash.as_bytes(), &[0u8; 32]);
    }

    #[test]
    fn case_order_does_not_change_the_hash() {
        // Outcomes cannot even reach the signature; this records the intent
        // that only the sorted case-id set is part of the interface.
        let hash = interface_hash("id", "body", &["a", "b"]);
        assert_eq!(interface_hash("id", "body", &["b", "a"]), hash);
    }
}
