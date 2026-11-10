//! Imported calc evidence: the consumer side of the calc evidence contract.
//!
//! A `calc:` bind names a verification in a calculation graph (veriq is the
//! first producer). What an approver judges is not that one function but the
//! whole definition-level upstream subgraph behind it: every root model,
//! calculation and verification the target depends on, as reviewable source,
//! plus the set of leaf results the target must satisfy. The wire format and
//! its validation live in the shared `inka-evidence` crate; this module adds
//! what only the consumer needs: reading the files an `inka.toml`
//! `[evidence] calcs` list names, anchoring every defect to the file that
//! carried it, and the interface hash each bind approval is bound to.
//!
//! Loading happens at project load, not only in evaluation, because calc
//! evidence carries structure: the argument nodes each target assumes become
//! edges of the graph (see [`crate::graph`]). A configured file that is
//! missing or malformed is a loud input error for every command, while a
//! `calc:` reference no loaded target answers stays evaluation's per-bind
//! "missing evidence".
//!
//! The producer emits raw source strings; inka never parses Python. Sources
//! are normalized CRLF→LF only, as test evidence bodies are: comments,
//! formatting and indentation are content, and a reformatting asks for fresh
//! approval. Which inka nodes the evidence assumes is not hashed here — the
//! assumed nodes' contract text fills the bind view hash's assumption slot,
//! see [`crate::hash`].
//!
//! # Layout (v1)
//!
//! ```text
//! preimage = field("inka:calc-evidence:v1") ‖ field(target path)
//!            ‖ entries(definitions) ‖ list(sorted leaf ids)
//! entries(definitions) = u64-LE(count) ‖ field(path[0]) ‖ field(normalized source[0]) ‖ ...
//! interface_hash = SHA-256(preimage)
//! ```
//!
//! Definitions are sorted by path bytewise and leaf IDs likewise, so the
//! order a producer lists them in is not content. A definition's path is: a
//! renamed calculation is a different definition, exactly as a renamed test
//! is a different test. The layout is frozen by the vectors under
//! `tests/golden/hash/calc-evidence-v1/`.

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use inka_evidence::calc::{CalcParseError, Target, VERSION};
use sha2::{Digest, Sha256};

pub use inka_evidence::calc::{DefinitionKind, InvalidCalcEvidence, Leaf};

use crate::hash::{InterfaceHash, Preimage};
use crate::normalize;
use crate::read::ReadText;

/// The domain tag framing every calc-evidence interface preimage.
pub const DOMAIN_TAG: &str = "inka:calc-evidence:v1";

/// One definition of the target's upstream subgraph, as the producer emitted it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Definition<'a> {
    /// The definition's path in the producer's graph, e.g. `Power::@budget`.
    pub path: &'a str,
    /// The reviewable source, CRLF/LF as delivered.
    pub source: &'a str,
}

/// Computes the frozen v1 calc-evidence interface hash.
///
/// `definitions` must include the target itself and must not repeat a path;
/// `leaf_ids` must not contain duplicates. The loader validates both before
/// hashing — a duplicate would be framed twice and silently widen the
/// preimage instead of erroring.
#[must_use]
pub fn interface_hash(
    target: &str,
    definitions: &[Definition<'_>],
    leaf_ids: &[&str],
) -> InterfaceHash {
    let mut sources: Vec<(&[u8], Vec<u8>)> = definitions
        .iter()
        .map(|definition| {
            (
                definition.path.as_bytes(),
                normalize::evidence_body(definition.source)
                    .into_owned()
                    .into_bytes(),
            )
        })
        .collect();
    sources.sort_unstable();
    let mut leaves: Vec<&[u8]> = leaf_ids.iter().map(|id| id.as_bytes()).collect();
    leaves.sort_unstable();

    let mut preimage = Preimage::default();
    preimage.field(DOMAIN_TAG.as_bytes());
    preimage.field(target.as_bytes());
    preimage.entries(&sources);
    preimage.list(&leaves);
    InterfaceHash::from_bytes(Sha256::digest(preimage.finish()).into())
}

/// One target's imported evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CalcTarget {
    /// The definition-level upstream subgraph, the target included, in
    /// delivery order.
    pub definitions: Vec<DefinitionSource>,
    /// The inka node IDs the subgraph assumes, each once, as delivered.
    pub assumes: Vec<String>,
    /// The required leaves with their observed results, in delivery order.
    pub leaves: Vec<Leaf>,
    /// Whether the producer's own validity conditions held.
    pub valid: bool,
    /// Whether the producer marks the target as an expected failure.
    pub xfail: bool,
    /// The hash the bind approval is bound to.
    pub interface_hash: InterfaceHash,
}

/// One definition as delivered: its path, kind and reviewable source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DefinitionSource {
    /// The definition's path in the producer's graph.
    pub path: String,
    /// What the definition is; presentation metadata, not hashed.
    pub kind: DefinitionKind,
    /// The reviewable source, CRLF/LF as delivered.
    pub source: String,
}

/// Every target loaded from the configured calc evidence files, keyed by
/// target path.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CalcEvidenceSet {
    targets: BTreeMap<String, CalcTarget>,
}

impl CalcEvidenceSet {
    /// Builds a set from already-parsed targets, hashing each one.
    ///
    /// This is what [`load_files`] does once the files are read; it is
    /// public so a caller holding targets from elsewhere — a test, an
    /// in-memory producer — gets the same set a file would give.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidCalcEvidence::DuplicateTarget`] when two targets
    /// share a path: a routing conflict, not a merge.
    pub fn from_targets(
        targets: impl IntoIterator<Item = Target>,
    ) -> Result<Self, InvalidCalcEvidence> {
        let mut set = Self::default();
        for target in targets {
            set.insert(target)?;
        }
        Ok(set)
    }

    fn insert(&mut self, target: Target) -> Result<(), InvalidCalcEvidence> {
        if self.targets.contains_key(&target.path) {
            return Err(InvalidCalcEvidence::DuplicateTarget {
                target: target.path,
            });
        }
        let definitions: Vec<Definition<'_>> = target
            .definitions
            .iter()
            .map(|definition| Definition {
                path: &definition.path,
                source: &definition.source,
            })
            .collect();
        let leaf_ids: Vec<&str> = target.leaves.iter().map(|leaf| leaf.id.as_str()).collect();
        let interface_hash = interface_hash(&target.path, &definitions, &leaf_ids);
        self.targets.insert(
            target.path,
            CalcTarget {
                definitions: target
                    .definitions
                    .into_iter()
                    .map(|definition| DefinitionSource {
                        path: definition.path,
                        kind: definition.kind,
                        source: definition.source,
                    })
                    .collect(),
                assumes: target.assumes,
                leaves: target.leaves,
                valid: target.valid,
                xfail: target.xfail,
                interface_hash,
            },
        );
        Ok(())
    }

    /// The evidence for one target path, or `None` when the loaded evidence
    /// does not mention it — evaluation's "missing evidence".
    #[must_use]
    pub fn get(&self, path: &str) -> Option<&CalcTarget> {
        self.targets.get(path)
    }

    /// Every loaded target, ordered by path.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &CalcTarget)> {
        self.targets
            .iter()
            .map(|(path, target)| (path.as_str(), target))
    }

    /// Whether no target was loaded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }
}

/// A reason loading the configured calc evidence failed.
///
/// Every variant is a CLI input error: the configured delivery premise does
/// not hold. None of them is the per-bind "missing evidence" outcome.
#[derive(Debug, thiserror::Error)]
pub enum CalcEvidenceError {
    #[error("cannot read calc evidence file `{}`: {source}", path.display())]
    Unreadable {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error(
        "calc evidence file `{}` is not valid v{VERSION} JSON: {source}",
        path.display()
    )]
    Malformed {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error(
        "calc evidence file `{}` has version {found}; this build reads v{VERSION}",
        path.display()
    )]
    UnsupportedVersion { path: PathBuf, found: u64 },
    #[error("invalid calc evidence in `{}`: {reason}", path.display())]
    Invalid {
        path: PathBuf,
        #[source]
        reason: InvalidCalcEvidence,
    },
}

impl CalcEvidenceError {
    /// Anchors a path-free parse defect to the file that carried it.
    fn of_file(error: CalcParseError, path: PathBuf) -> Self {
        match error {
            CalcParseError::Json(source) => Self::Malformed { path, source },
            CalcParseError::UnsupportedVersion { found } => {
                Self::UnsupportedVersion { path, found }
            }
            CalcParseError::Invalid(reason) => Self::Invalid { path, reason },
        }
    }
}

/// Reads and validates every configured calc evidence file into one set.
///
/// `files` are manifest-relative paths, joined onto `base_dir` the same way
/// `values.files` are. Target paths must be unique across the whole set.
///
/// # Errors
///
/// Returns a [`CalcEvidenceError`] when a file cannot be read, is not valid
/// v1 JSON, declares a version other than 1, or fails a semantic check of the
/// shared crate ([`InvalidCalcEvidence`]) — including a target path delivered
/// twice, within one file or across files.
pub fn load_files(
    base_dir: &Path,
    files: &[PathBuf],
    reader: &dyn ReadText,
) -> Result<CalcEvidenceSet, CalcEvidenceError> {
    let mut set = CalcEvidenceSet::default();
    for relative in files {
        let path = base_dir.join(relative);
        let text = reader
            .read_text(&path)
            .map_err(|source| CalcEvidenceError::Unreadable {
                path: path.clone(),
                source,
            })?;
        let targets = inka_evidence::calc::parse(&text)
            .map_err(|error| CalcEvidenceError::of_file(error, path.clone()))?;
        for target in targets {
            set.insert(target)
                .map_err(|reason| CalcEvidenceError::Invalid {
                    path: path.clone(),
                    reason,
                })?;
        }
    }
    Ok(set)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TARGET: &str = "Power::?verify_margin";

    fn definition<'a>(path: &'a str, source: &'a str) -> Definition<'a> {
        Definition { path, source }
    }

    fn hash(definitions: &[Definition<'_>], leaves: &[&str]) -> InterfaceHash {
        interface_hash(TARGET, definitions, leaves)
    }

    #[test]
    fn definition_order_is_not_content() {
        // Arrange
        let budget = definition("Power::@budget", "def budget(): ...");
        let verify = definition("Power::?verify_margin", "def verify_margin(): ...");

        // Act
        let declared = hash(&[budget, verify], &[TARGET]);
        let reordered = hash(&[verify, budget], &[TARGET]);

        // Assert
        assert_eq!(declared, reordered);
    }

    #[test]
    fn leaf_order_is_not_content() {
        // Arrange
        let verify = definition(TARGET, "def verify_margin(): ...");

        // Act
        let declared = hash(
            &[verify],
            &["Power::?verify_margin.a", "Power::?verify_margin.b"],
        );
        let reordered = hash(
            &[verify],
            &["Power::?verify_margin.b", "Power::?verify_margin.a"],
        );

        // Assert
        assert_eq!(declared, reordered);
    }

    #[test]
    fn a_crlf_source_hashes_like_its_lf_twin_and_nothing_else_is_normalized() {
        // Arrange
        let lf = definition(TARGET, "def f():\n    return 1  \n");
        let crlf = definition(TARGET, "def f():\r\n    return 1  \r\n");
        let trimmed = definition(TARGET, "def f():\n    return 1\n");

        // Act & Assert
        assert_eq!(hash(&[lf], &[TARGET]), hash(&[crlf], &[TARGET]));
        assert_ne!(hash(&[lf], &[TARGET]), hash(&[trimmed], &[TARGET]));
    }

    #[test]
    fn editing_an_upstream_source_changes_the_hash() {
        // Arrange
        let verify = definition(TARGET, "def verify_margin(): ...");
        let before = definition("Power::@budget", "def budget(): return 1");
        let after = definition("Power::@budget", "def budget(): return 2");

        // Act & Assert
        assert_ne!(
            hash(&[before, verify], &[TARGET]),
            hash(&[after, verify], &[TARGET])
        );
    }

    #[test]
    fn renaming_an_upstream_definition_changes_the_hash() {
        // Arrange
        let verify = definition(TARGET, "def verify_margin(): ...");
        let before = definition("Power::@budget", "def budget(): ...");
        let after = definition("Power::@power_budget", "def budget(): ...");

        // Act & Assert
        assert_ne!(
            hash(&[before, verify], &[TARGET]),
            hash(&[after, verify], &[TARGET])
        );
    }

    #[test]
    fn the_required_leaf_set_is_content() {
        // Arrange
        let verify = definition(TARGET, "def verify_margin(): ...");

        // Act & Assert: dropping a leaf cannot keep looking like the old interface.
        assert_ne!(
            hash(
                &[verify],
                &["Power::?verify_margin.a", "Power::?verify_margin.b"]
            ),
            hash(&[verify], &["Power::?verify_margin.a"])
        );
    }

    #[test]
    fn the_target_path_is_content() {
        // Arrange
        let verify = definition(TARGET, "def verify_margin(): ...");

        // Act & Assert
        assert_ne!(
            interface_hash(TARGET, &[verify], &[TARGET]),
            interface_hash("Thermal::?verify_margin", &[verify], &[TARGET])
        );
    }

    #[test]
    fn source_and_path_fields_cannot_be_confused() {
        // Arrange: moving bytes across the field boundary.
        let one = definition("Power::@ab", "c");
        let other = definition("Power::@a", "bc");

        // Act & Assert
        assert_ne!(hash(&[one], &[TARGET]), hash(&[other], &[TARGET]));
    }
}
