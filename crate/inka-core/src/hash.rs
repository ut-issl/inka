//! View hashes: the content-addressed identity of what an approver approved.
//!
//! An approval record names a target and a `view_hash`. Evaluation counts that
//! record only while the hash it carries still equals the one recomputed from
//! the current argument, so this module decides — for the whole system — which
//! edits invalidate an approval and which do not.
//!
//! What a hash covers is deliberately *local*: a node's own contract text, its
//! deadline annotation, and, for a decomposition, the contract text of its
//! direct children. Nothing at grandchild depth is included. Editing a leaf
//! therefore invalidates that leaf's approval and its parents' structure
//! approvals, and stops there: re-approval cost stays proportional to the edit.
//!
//! What is excluded is equally deliberate. IDs, tags, comments, and the
//! `ci:` / `veriq:` reference strings are not contract content — an approver
//! judged the claim, not its name, its filing, or where the evidence lives — so
//! renaming a node or retagging it never asks anyone to approve again. A
//! deadline *is* content: `until:` is a jointly-approved decline placed toward
//! the future, and moving it changes what was agreed. Interpolation runs before
//! all of this, so a hash never sees a `{$var}`: changing a value in
//! `values.toml` changes the contract and invalidates exactly the approvals of
//! the nodes that interpolate it.
//!
//! # Layout (v1)
//!
//! The preimage is a flat byte string built from two framing rules:
//!
//! ```text
//! field(bytes) = u64-LE(bytes.len()) ‖ bytes
//! list(items)  = u64-LE(items.len()) ‖ field(item[0]) ‖ field(item[1]) ‖ ...
//! ```
//!
//! Length prefixes make the encoding unambiguous: no concatenation of field
//! contents can imitate a different one, so two distinct local views cannot
//! share a preimage. Each kind then frames its own fields after a domain tag,
//! which keeps a fiat and a structure with identical text apart:
//!
//! ```text
//! objective = field("inka:objective:v1") ‖ field(desc) ‖ list(annotations) ‖ list(children)
//! structure = field("inka:structure:v1") ‖ field(desc) ‖ list(annotations) ‖ list(children)
//! fiat      = field("inka:fiat:v1")      ‖ field(desc) ‖ list(annotations)
//! bind      = field("inka:bind:v1")      ‖ field(desc) ‖ list(annotations)
//!             ‖ field(interface_hash) ‖ list(assumptions)
//! ```
//!
//! `view_hash = SHA-256(preimage)`.
//!
//! Children are listed by their contract text, sorted bytewise, so reordering a
//! `children:` list is not an edit. Annotations are `key:value` strings sorted
//! by key. The bind assumption list is empty until assumption aggregation
//! lands; the slot is framed now so filling it later is not a layout change.
//!
//! This layout is frozen by the golden vectors under `tests/golden/hash/v1/`.
//! An incompatible change ships as a parallel v2 with its own domain tags, so
//! recorded approvals never silently change meaning.

use std::fmt;
use std::str::FromStr;

use sha2::{Digest, Sha256};

use crate::normalize;
use crate::view::{ArgumentView, Deadline, Mode, ViewNode};

/// Bytes in a SHA-256 digest.
const DIGEST_LEN: usize = 32;

/// Prefix every hash this system writes down carries, so the algorithm is
/// legible in a record rather than inferred from a hex length.
const DIGEST_PREFIX: &str = "sha256:";

/// The hash an approval record is bound to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ViewHash([u8; DIGEST_LEN]);

impl ViewHash {
    /// The raw digest.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; DIGEST_LEN] {
        &self.0
    }
}

/// Displays as `sha256:<hex>`, the exact spelling an approval record stores.
impl fmt::Display for ViewHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_prefixed_hex(f, &self.0)
    }
}

/// A content-addressed hash of what a bind block's evidence *checks*.
///
/// Supplied by the evidence provider, not computed here: what identifies an
/// interface is the provider's business, and a bind's view hash only has to
/// commit to whatever the provider named. Including it means editing the test
/// a bind rests on invalidates the bind approval, while editing the
/// implementation it exercises does not — CI is then free to re-decide truth
/// without anyone re-approving validity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InterfaceHash([u8; DIGEST_LEN]);

impl InterfaceHash {
    /// Wraps a digest an evidence provider produced.
    #[must_use]
    pub const fn from_bytes(bytes: [u8; DIGEST_LEN]) -> Self {
        Self(bytes)
    }

    /// The raw digest.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; DIGEST_LEN] {
        &self.0
    }
}

impl fmt::Display for InterfaceHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write_prefixed_hex(f, &self.0)
    }
}

/// Reads the `sha256:<hex>` spelling providers and records use.
impl FromStr for InterfaceHash {
    type Err = InvalidDigest;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let invalid = || InvalidDigest(text.to_owned());
        let hex = text.strip_prefix(DIGEST_PREFIX).ok_or_else(invalid)?;
        if hex.len() != DIGEST_LEN * 2 {
            return Err(invalid());
        }

        let mut bytes = [0u8; DIGEST_LEN];
        for (byte, pair) in bytes.iter_mut().zip(hex.as_bytes().chunks_exact(2)) {
            // Digit by digit rather than through an integer parser, which
            // would also accept a signed `+ff` as a byte.
            let high = hex_digit(pair[0]).ok_or_else(invalid)?;
            let low = hex_digit(pair[1]).ok_or_else(invalid)?;
            *byte = (high << 4) | low;
        }
        Ok(Self(bytes))
    }
}

/// The value of one lowercase or uppercase ASCII hex digit.
fn hex_digit(digit: u8) -> Option<u8> {
    match digit {
        b'0'..=b'9' => Some(digit - b'0'),
        b'a'..=b'f' => Some(digit - b'a' + 10),
        b'A'..=b'F' => Some(digit - b'A' + 10),
        _ => None,
    }
}

/// A digest string that is not `sha256:` followed by 64 hex digits.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("expected a `sha256:<64 hex digits>` digest, found `{0}`")]
pub struct InvalidDigest(pub String);

/// Everything one approval covers, and nothing else.
///
/// The fields are private and each mode has its own constructor, so a local
/// view that the language cannot produce — a fiat with children, a structure
/// with an interface hash — is not constructible either.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalView<'a> {
    mode: Mode,
    description: &'a str,
    deadline: Option<&'a Deadline>,
    discharge: Discharge<'a>,
}

/// How the node is discharged, and the extra content that carries.
///
/// Pairing the payload with the mode in one value rather than leaving a bag of
/// optionals is what lets hashing read the shape it needs without a fallible
/// unwrap in the middle of a frozen layout.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Discharge<'a> {
    /// Contract text of the direct children, in any order: hashing sorts them.
    Decomposition(Vec<&'a str>),
    Evidence(InterfaceHash),
    Assertion,
}

impl<'a> LocalView<'a> {
    /// The local view of an objective, which is discharged by decomposition.
    #[must_use]
    pub fn objective(
        description: &'a str,
        deadline: Option<&'a Deadline>,
        children: Vec<&'a str>,
    ) -> Self {
        Self {
            mode: Mode::Objective,
            description,
            deadline,
            discharge: Discharge::Decomposition(children),
        }
    }

    /// The local view of a structure, which is discharged by decomposition.
    #[must_use]
    pub fn structure(
        description: &'a str,
        deadline: Option<&'a Deadline>,
        children: Vec<&'a str>,
    ) -> Self {
        Self {
            mode: Mode::Structure,
            description,
            deadline,
            discharge: Discharge::Decomposition(children),
        }
    }

    /// The local view of a bind, which is discharged against evidence.
    #[must_use]
    pub fn bind(
        description: &'a str,
        deadline: Option<&'a Deadline>,
        interface: InterfaceHash,
    ) -> Self {
        Self {
            mode: Mode::Bind,
            description,
            deadline,
            discharge: Discharge::Evidence(interface),
        }
    }

    /// The local view of a fiat, which is accepted without further argument.
    #[must_use]
    pub fn fiat(description: &'a str, deadline: Option<&'a Deadline>) -> Self {
        Self {
            mode: Mode::Fiat,
            description,
            deadline,
            discharge: Discharge::Assertion,
        }
    }

    /// Assembles the local view of one projected node.
    ///
    /// `interface` is the evidence interface hash and is consulted for bind
    /// nodes only; every other mode ignores it.
    ///
    /// # Errors
    ///
    /// Returns [`LocalViewError::MissingInterfaceHash`] when a bind node is
    /// given no interface hash, and [`LocalViewError::UndefinedChild`] when a
    /// node names a child the view does not define. A view projected from a
    /// built graph never contains an unresolved reference, so the latter says
    /// the view was assembled by hand and is incomplete.
    pub fn of_node(
        view: &'a ArgumentView,
        node: &'a ViewNode,
        interface: Option<InterfaceHash>,
    ) -> Result<Self, LocalViewError> {
        let deadline = node.deadline.as_ref();
        match node.mode {
            Mode::Objective => Ok(Self::objective(
                &node.description,
                deadline,
                child_descriptions(view, node)?,
            )),
            Mode::Structure => Ok(Self::structure(
                &node.description,
                deadline,
                child_descriptions(view, node)?,
            )),
            Mode::Fiat => Ok(Self::fiat(&node.description, deadline)),
            Mode::Bind => {
                let interface = interface.ok_or_else(|| LocalViewError::MissingInterfaceHash {
                    id: node.id.clone(),
                })?;
                Ok(Self::bind(&node.description, deadline, interface))
            }
        }
    }

    /// Computes the v1 view hash.
    ///
    /// Descriptions are normalized here rather than trusted to arrive
    /// normalized: normalization is idempotent, so a caller that already
    /// normalized pays nothing, and one that did not cannot produce a hash
    /// that disagrees with everyone else's.
    #[must_use]
    pub fn view_hash(&self) -> ViewHash {
        let mut preimage = Preimage::default();
        preimage.field(domain_tag(self.mode).as_bytes());
        preimage.field(normalize::description(self.description).as_bytes());
        preimage.list(&annotations(self.deadline));

        match &self.discharge {
            Discharge::Decomposition(children) => {
                let mut children: Vec<String> =
                    children.iter().map(|c| normalize::description(c)).collect();
                // Bytewise, on the normalized text: reordering a `children:`
                // list is a filing change, not a contract change.
                children.sort_unstable();
                preimage.list(&children);
            }
            Discharge::Evidence(interface) => {
                preimage.field(interface.as_bytes());
                // The assumption slot, empty until assumption aggregation
                // lands. Framing it now keeps filling it from being a layout
                // change that would invalidate every recorded bind approval.
                preimage.list::<String>(&[]);
            }
            Discharge::Assertion => {}
        }

        ViewHash(Sha256::digest(preimage.finish()).into())
    }
}

/// A local view that could not be assembled from a projected node.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LocalViewError {
    #[error("bind `{id}` cannot be hashed without its evidence interface hash")]
    MissingInterfaceHash { id: String },
    #[error("node `{parent}` names a child `{child}` the view does not define")]
    UndefinedChild { parent: String, child: String },
}

/// The domain tag separating the four kinds of hash.
fn domain_tag(mode: Mode) -> &'static str {
    match mode {
        Mode::Objective => "inka:objective:v1",
        Mode::Structure => "inka:structure:v1",
        Mode::Bind => "inka:bind:v1",
        Mode::Fiat => "inka:fiat:v1",
    }
}

/// The canonical form of a node's annotations: `key:value` sorted by key.
///
/// Only the deadline group is annotation content today. A `reason` is taken as
/// written, without its quotes: it is a single-line string with no escapes, so
/// it has no normal form to reduce to.
fn annotations(deadline: Option<&Deadline>) -> Vec<String> {
    let Some(deadline) = deadline else {
        return Vec::new();
    };
    let promote = deadline.promote.map(|mode| match mode {
        Mode::Objective => "objective",
        Mode::Structure => "structure",
        Mode::Bind => "bind",
        Mode::Fiat => "fiat",
    });

    let mut pairs: Vec<(&str, &str)> = vec![("until", &deadline.until)];
    if let Some(reason) = &deadline.reason {
        pairs.push(("reason", reason));
    }
    if let Some(promote) = promote {
        pairs.push(("promote", promote));
    }
    pairs.sort_unstable_by_key(|&(key, _)| key);
    pairs
        .into_iter()
        .map(|(key, value)| format!("{key}:{value}"))
        .collect()
}

/// Contract text of a node's direct children, in declaration order.
fn child_descriptions<'a>(
    view: &'a ArgumentView,
    node: &ViewNode,
) -> Result<Vec<&'a str>, LocalViewError> {
    node.children
        .iter()
        .map(|child| {
            view.node(child)
                .map(|child| child.description.as_str())
                .ok_or_else(|| LocalViewError::UndefinedChild {
                    parent: node.id.clone(),
                    child: child.clone(),
                })
        })
        .collect()
}

/// The length-prefixed byte string a view hash is taken over.
#[derive(Default)]
struct Preimage(Vec<u8>);

impl Preimage {
    /// Appends `u64-LE(len) ‖ bytes`.
    fn field(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(&length(bytes.len()));
        self.0.extend_from_slice(bytes);
    }

    /// Appends `u64-LE(count)` followed by each element as a field.
    fn list<T: AsRef<[u8]>>(&mut self, items: &[T]) {
        self.0.extend_from_slice(&length(items.len()));
        for item in items {
            self.field(item.as_ref());
        }
    }

    fn finish(self) -> Vec<u8> {
        self.0
    }
}

/// A length as the eight little-endian bytes the framing prescribes.
fn length(value: usize) -> [u8; 8] {
    u64::try_from(value)
        .expect("a description or list length fits in u64")
        .to_le_bytes()
}

fn write_prefixed_hex(f: &mut fmt::Formatter<'_>, bytes: &[u8; DIGEST_LEN]) -> fmt::Result {
    f.write_str(DIGEST_PREFIX)?;
    for byte in bytes {
        write!(f, "{byte:02x}")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::view::{ViewEdge, ViewNode};

    const INTERFACE: InterfaceHash = InterfaceHash::from_bytes([0x11; DIGEST_LEN]);

    fn deadline(until: &str, reason: Option<&str>, promote: Option<Mode>) -> Deadline {
        Deadline {
            until: until.to_owned(),
            reason: reason.map(str::to_owned),
            promote,
        }
    }

    #[test]
    fn the_four_kinds_never_share_a_hash_for_the_same_text() {
        // Arrange & Act: identical text, identical (absent) annotations.
        let objective = LocalView::objective("the claim", None, Vec::new()).view_hash();
        let structure = LocalView::structure("the claim", None, Vec::new()).view_hash();
        let fiat = LocalView::fiat("the claim", None).view_hash();
        let bind = LocalView::bind("the claim", None, INTERFACE).view_hash();

        // Assert: the domain tag separates them.
        let distinct = [objective, structure, fiat, bind];
        for (index, left) in distinct.iter().enumerate() {
            for right in &distinct[index + 1..] {
                assert_ne!(left, right);
            }
        }
    }

    #[test]
    fn reordering_children_leaves_the_hash_alone() {
        // Arrange & Act
        let declared = LocalView::structure("parent", None, vec!["alpha", "beta"]).view_hash();
        let reordered = LocalView::structure("parent", None, vec!["beta", "alpha"]).view_hash();

        // Assert: a `children:` list is filing, not contract content.
        assert_eq!(declared, reordered);
    }

    #[test]
    fn a_changed_child_description_changes_the_parents_hash() {
        let before = LocalView::structure("parent", None, vec!["alpha"]).view_hash();
        let after = LocalView::structure("parent", None, vec!["alpha edited"]).view_hash();

        assert_ne!(before, after);
    }

    #[test]
    fn adding_a_child_changes_the_hash_even_when_it_repeats_one() {
        // Arrange & Act: the element count is framed, so a duplicate is not
        // absorbed into its twin.
        let one = LocalView::structure("parent", None, vec!["alpha"]).view_hash();
        let two = LocalView::structure("parent", None, vec!["alpha", "alpha"]).view_hash();

        assert_ne!(one, two);
    }

    #[test]
    fn framing_keeps_a_split_description_from_imitating_a_joined_one() {
        // Arrange & Act: without length prefixes both would concatenate to the
        // same bytes.
        let split = LocalView::structure("ab", None, vec!["cd", "ef"]).view_hash();
        let joined = LocalView::structure("abc", None, vec!["d", "ef"]).view_hash();

        assert_ne!(split, joined);
    }

    #[test]
    fn internal_line_breaks_and_spacing_are_contract_content() {
        // Arrange & Act
        let broken = LocalView::fiat("the sun angle\nwithin 30 deg", None).view_hash();
        let joined = LocalView::fiat("the sun angle within 30 deg", None).view_hash();
        let spaced = LocalView::fiat("the sun angle  within 30 deg", None).view_hash();

        // Assert: near-verbatim, so none of these are the same contract.
        assert_ne!(broken, joined);
        assert_ne!(joined, spaced);
    }

    #[test]
    fn normalization_equivalent_text_hashes_the_same() {
        // Arrange: fence indentation, CRLF, trailing spaces, and blank padding
        // differ; the contract does not.
        let raw = "\r\n  first line   \r\n\r\n    second line\r\n  \r\n";
        let normalized = "first line\n\n  second line";

        // Act & Assert
        assert_eq!(
            LocalView::fiat(raw, None).view_hash(),
            LocalView::fiat(normalized, None).view_hash()
        );
    }

    #[test]
    fn a_deadline_is_contract_content() {
        let bare = LocalView::fiat("taken as given", None).view_hash();
        let dated = deadline("M3", None, None);
        let dated = LocalView::fiat("taken as given", Some(&dated)).view_hash();

        assert_ne!(bare, dated);
    }

    #[test]
    fn every_part_of_a_deadline_changes_the_hash() {
        let base = deadline("M3", Some("awaiting the test"), Some(Mode::Bind));
        let later = deadline("M4", Some("awaiting the test"), Some(Mode::Bind));
        let requoted = deadline("M3", Some("awaiting the rig"), Some(Mode::Bind));
        let repromoted = deadline("M3", Some("awaiting the test"), Some(Mode::Structure));

        let hash = |d: &Deadline| LocalView::fiat("taken as given", Some(d)).view_hash();
        assert_ne!(hash(&base), hash(&later));
        assert_ne!(hash(&base), hash(&requoted));
        assert_ne!(hash(&base), hash(&repromoted));
    }

    #[test]
    fn annotations_are_key_value_strings_sorted_by_key() {
        // Arrange
        let deadline = deadline(
            "M3",
            Some("awaiting the mass-properties test"),
            Some(Mode::Bind),
        );

        // Act
        let canonical = annotations(Some(&deadline));

        // Assert: the spelling `spec.md` fixes.
        assert_eq!(
            canonical,
            [
                "promote:bind",
                "reason:awaiting the mass-properties test",
                "until:M3",
            ]
        );
    }

    #[test]
    fn a_node_without_a_deadline_has_no_annotations() {
        assert!(annotations(None).is_empty());
    }

    #[test]
    fn the_evidence_interface_hash_is_part_of_a_bind_hash() {
        // Arrange & Act: same contract, a different test behind it.
        let one = LocalView::bind("checked in CI", None, INTERFACE).view_hash();
        let other = LocalView::bind(
            "checked in CI",
            None,
            InterfaceHash::from_bytes([0x22; DIGEST_LEN]),
        )
        .view_hash();

        assert_ne!(one, other);
    }

    #[test]
    fn a_view_hash_displays_as_a_prefixed_lowercase_digest() {
        // Arrange & Act
        let rendered = LocalView::fiat("taken as given", None)
            .view_hash()
            .to_string();

        // Assert
        let hex = rendered
            .strip_prefix("sha256:")
            .expect("a view hash names its algorithm");
        assert_eq!(hex.len(), 64);
        assert!(
            hex.chars()
                .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase())
        );
    }

    #[test]
    fn an_interface_hash_round_trips_through_its_displayed_form() {
        // Arrange
        let original = InterfaceHash::from_bytes([0xab; DIGEST_LEN]);

        // Act
        let parsed: InterfaceHash = original
            .to_string()
            .parse()
            .expect("the displayed form parses back");

        // Assert
        assert_eq!(parsed, original);
    }

    #[test]
    fn rejects_a_digest_string_that_is_not_a_prefixed_sha256() {
        for text in [
            "0000000000000000000000000000000000000000000000000000000000000000",
            "sha256:abc",
            "sha512:0000000000000000000000000000000000000000000000000000000000000000",
            "sha256:zz00000000000000000000000000000000000000000000000000000000000000",
            // An integer parser would read the first byte as `+f`.
            "sha256:+f00000000000000000000000000000000000000000000000000000000000000",
        ] {
            assert_eq!(
                text.parse::<InterfaceHash>(),
                Err(InvalidDigest(text.to_owned())),
                "`{text}` should not parse"
            );
        }
    }

    /// A two-node view: one structure over one fiat.
    fn view() -> ArgumentView {
        ArgumentView {
            nodes: vec![
                ViewNode {
                    id: "leaf".to_owned(),
                    mode: Mode::Fiat,
                    namespace: "safety".to_owned(),
                    description: "taken as given".to_owned(),
                    tags: Vec::new(),
                    evidence: None,
                    deadline: None,
                    children: Vec::new(),
                },
                ViewNode {
                    id: "root".to_owned(),
                    mode: Mode::Structure,
                    namespace: "safety".to_owned(),
                    description: "the claim".to_owned(),
                    tags: Vec::new(),
                    evidence: None,
                    deadline: None,
                    children: vec!["leaf".to_owned()],
                },
            ],
            edges: vec![ViewEdge {
                parent: "root".to_owned(),
                child: "leaf".to_owned(),
            }],
            roots: Vec::new(),
        }
    }

    #[test]
    fn assembles_a_local_view_from_a_projected_node() {
        // Arrange
        let view = view();
        let root = view.node("root").expect("the fixture defines root");

        // Act
        let local = LocalView::of_node(&view, root, None).expect("a structure needs no interface");

        // Assert: the parent's own text plus its children's, and nothing else.
        assert_eq!(
            local,
            LocalView::structure("the claim", None, vec!["taken as given"])
        );
    }

    #[test]
    fn refuses_to_hash_a_bind_without_its_interface_hash() {
        // Arrange
        let mut view = view();
        view.nodes[1].mode = Mode::Bind;
        view.nodes[1].children.clear();
        let bind = view.node("root").expect("the fixture defines root");

        // Act
        let error = LocalView::of_node(&view, bind, None).expect_err("a bind needs evidence");

        // Assert
        assert_eq!(
            error,
            LocalViewError::MissingInterfaceHash {
                id: "root".to_owned()
            }
        );
    }

    #[test]
    fn reports_a_child_the_view_does_not_define() {
        // Arrange: a hand-built view; a projected one cannot contain this.
        let mut view = view();
        view.nodes[1].children = vec!["absent".to_owned()];
        let root = view.node("root").expect("the fixture defines root");

        // Act
        let error = LocalView::of_node(&view, root, None).expect_err("the child is undefined");

        // Assert
        assert_eq!(
            error,
            LocalViewError::UndefinedChild {
                parent: "root".to_owned(),
                child: "absent".to_owned(),
            }
        );
    }

    #[test]
    fn ignores_an_interface_hash_offered_to_a_non_bind_node() {
        // Arrange
        let view = view();
        let root = view.node("root").expect("the fixture defines root");

        // Act
        let offered = LocalView::of_node(&view, root, Some(INTERFACE)).expect("a structure builds");
        let plain = LocalView::of_node(&view, root, None).expect("a structure builds");

        // Assert: only a bind is discharged against evidence.
        assert_eq!(offered.view_hash(), plain.view_hash());
    }
}
