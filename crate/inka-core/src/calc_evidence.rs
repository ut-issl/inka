//! The calc evidence interface hash: the consumer side of the calc evidence
//! contract, as far as approvals are concerned.
//!
//! A `calc:` bind names a verification in a calculation graph (veriq is the
//! first producer). What an approver judges is not that one function but the
//! whole definition-level upstream subgraph behind it: every root model,
//! calculation and verification the target depends on, as reviewable source,
//! plus the set of leaf results the target must satisfy. This module computes
//! the hash each bind approval is bound to, over exactly that.
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

use sha2::{Digest, Sha256};

use crate::hash::{InterfaceHash, Preimage};
use crate::normalize;

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
