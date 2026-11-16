//! View hashes: the content-addressed identity of what an approver approved.
//!
//! An approval record names a target and a `view_hash`. Evaluation counts that
//! record only while the hash it carries still equals the one recomputed from
//! the current argument, so this module decides — for the whole system — which
//! edits invalidate an approval and which do not.
//!
//! What a hash covers is deliberately *local*: a node's own contract text, its
//! deadline annotation, the comments attached to it, and, for a decomposition,
//! the contract text of its direct children with the comments on those child
//! entries. Nothing at grandchild depth is included. Editing a leaf therefore
//! invalidates that leaf's approval and its parents' structure approvals, and
//! stops there: re-approval cost stays proportional to the edit.
//!
//! What is excluded is equally deliberate. IDs, tags, file comments, and the
//! `test:` / `veriq:` reference strings are not contract content — an approver
//! judged the claim, not its name, its filing, or where the evidence lives — so
//! renaming a node or retagging it never asks anyone to approve again. A
//! deadline *is* content: `until:` is a jointly-approved decline placed toward
//! the future, and moving it changes what was agreed. A comment is content
//! too: it is presented with its element at review, so what it says is part of
//! what was approved. Interpolation runs before
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
//! entries(items) = u64-LE(items.len()) ‖ field(desc[0]) ‖ field(comment[0]) ‖ ...
//!
//! objective = field("inka:objective:v1") ‖ field(desc) ‖ list(annotations)
//!             ‖ list(comments) ‖ entries(children)
//! structure = field("inka:structure:v1") ‖ field(desc) ‖ list(annotations)
//!             ‖ list(comments) ‖ entries(children)
//! fiat      = field("inka:fiat:v1")      ‖ field(desc) ‖ list(annotations)
//!             ‖ list(comments)
//! bind      = field("inka:bind:v1")      ‖ field(desc) ‖ list(annotations)
//!             ‖ list(comments) ‖ field(interface_hash) ‖ list(assumptions)
//! ```
//!
//! `view_hash = SHA-256(preimage)`.
//!
//! Annotations are `key:value` strings sorted by key. Comments are
//! `anchor:text` strings sorted bytewise, one per anchor that carries a
//! comment, the anchor being `block` or a property name. A child entry is the
//! child's contract text and the comment on the entry (empty when there is
//! none); entries are sorted by text, then comment, bytewise, so reordering a
//! `children:` list is not an edit. The bind assumption list holds the
//! normalized contract text of every node the evidence assumes, sorted
//! bytewise; a bind whose evidence assumes nothing frames an empty list.
//!
//! This layout is frozen by the golden vectors under `tests/golden/hash/v1/`.
//! An incompatible change ships as a parallel v2 with its own domain tags, so
//! recorded approvals never silently change meaning.

use std::fmt;
use std::str::FromStr;

use serde::{Serialize, Serializer};
use sha2::{Digest, Sha256};

use crate::normalize;
use crate::view::{ArgumentView, Deadline, Mode, NodeComments, ViewNode};

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

/// Reads the `sha256:<hex>` spelling an approval record stores.
impl FromStr for ViewHash {
    type Err = InvalidDigest;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        parse_digest(text).map(Self)
    }
}

/// Serializes as the `sha256:<hex>` string, so a record carrying a view hash
/// writes the exact spelling the spec fixes.
impl Serialize for ViewHash {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
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
        parse_digest(text).map(Self)
    }
}

/// Reads `sha256:` followed by 64 hex digits into the raw digest.
fn parse_digest(text: &str) -> Result<[u8; DIGEST_LEN], InvalidDigest> {
    let invalid = || InvalidDigest(text.to_owned());
    let hex = text.strip_prefix(DIGEST_PREFIX).ok_or_else(invalid)?;
    if hex.len() != DIGEST_LEN * 2 {
        return Err(invalid());
    }

    let mut bytes = [0u8; DIGEST_LEN];
    let (pairs, _) = hex.as_bytes().as_chunks::<2>();
    for (byte, &[high, low]) in bytes.iter_mut().zip(pairs) {
        // Digit by digit rather than through an integer parser, which
        // would also accept a signed `+ff` as a byte.
        let high = hex_digit(high).ok_or_else(invalid)?;
        let low = hex_digit(low).ok_or_else(invalid)?;
        *byte = (high << 4) | low;
    }
    Ok(bytes)
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
    /// Comments on the node itself and its properties; child-entry comments
    /// travel with the children.
    comments: Option<&'a NodeComments>,
    discharge: Discharge<'a>,
}

/// One child entry as its parent's local view sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChildContract<'a> {
    /// The child's contract text.
    pub description: &'a str,
    /// The comment on the parent's entry for this child; empty when none.
    pub comment: &'a str,
}

impl<'a> From<&'a str> for ChildContract<'a> {
    /// A child entry without a comment.
    fn from(description: &'a str) -> Self {
        Self {
            description,
            comment: "",
        }
    }
}

/// How the node is discharged, and the extra content that carries.
///
/// Pairing the payload with the mode in one value rather than leaving a bag of
/// optionals is what lets hashing read the shape it needs without a fallible
/// unwrap in the middle of a frozen layout.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Discharge<'a> {
    /// The direct children's entries, in any order: hashing sorts them.
    Decomposition(Vec<ChildContract<'a>>),
    /// The evidence interface and the contract text of every node the
    /// evidence assumes, in any order: hashing sorts them.
    Evidence {
        interface: InterfaceHash,
        assumptions: Vec<&'a str>,
    },
    Assertion,
}

impl<'a> LocalView<'a> {
    /// The local view of an objective, which is discharged by decomposition.
    #[must_use]
    pub fn objective<C: Into<ChildContract<'a>>>(
        description: &'a str,
        deadline: Option<&'a Deadline>,
        children: impl IntoIterator<Item = C>,
    ) -> Self {
        Self {
            mode: Mode::Objective,
            description,
            deadline,
            comments: None,
            discharge: Discharge::Decomposition(children.into_iter().map(Into::into).collect()),
        }
    }

    /// The local view of a structure, which is discharged by decomposition.
    #[must_use]
    pub fn structure<C: Into<ChildContract<'a>>>(
        description: &'a str,
        deadline: Option<&'a Deadline>,
        children: impl IntoIterator<Item = C>,
    ) -> Self {
        Self {
            mode: Mode::Structure,
            description,
            deadline,
            comments: None,
            discharge: Discharge::Decomposition(children.into_iter().map(Into::into).collect()),
        }
    }

    /// The local view of a bind, which is discharged against evidence.
    ///
    /// `assumptions` are the rendered descriptions of the nodes the evidence
    /// assumes, deduplicated by node upstream; test evidence assumes nothing
    /// and passes an empty iterator. The approver judges the evidence against
    /// what it takes for granted, so that text is contract content — the
    /// assumed nodes' IDs are not.
    #[must_use]
    pub fn bind(
        description: &'a str,
        deadline: Option<&'a Deadline>,
        interface: InterfaceHash,
        assumptions: impl IntoIterator<Item = &'a str>,
    ) -> Self {
        Self {
            mode: Mode::Bind,
            description,
            deadline,
            comments: None,
            discharge: Discharge::Evidence {
                interface,
                assumptions: assumptions.into_iter().collect(),
            },
        }
    }

    /// The local view of a fiat, which is accepted without further argument.
    #[must_use]
    pub fn fiat(description: &'a str, deadline: Option<&'a Deadline>) -> Self {
        Self {
            mode: Mode::Fiat,
            description,
            deadline,
            comments: None,
            discharge: Discharge::Assertion,
        }
    }

    /// Adds the comments on the node itself and on its properties.
    ///
    /// Comments on child entries are not read from here: they are part of
    /// each [`ChildContract`].
    #[must_use]
    pub fn with_comments(self, comments: &'a NodeComments) -> Self {
        Self {
            comments: Some(comments),
            ..self
        }
    }

    /// Assembles the local view of one projected node.
    ///
    /// `interface` is the evidence interface hash and is consulted for bind
    /// nodes only; every other mode ignores it. The assumption slot of a bind
    /// assembled here is empty: the evidence that fills it is not part of the
    /// projected view.
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
        let local = match node.mode {
            Mode::Objective => {
                Self::objective(&node.description, deadline, child_contracts(view, node)?)
            }
            Mode::Structure => {
                Self::structure(&node.description, deadline, child_contracts(view, node)?)
            }
            Mode::Fiat => Self::fiat(&node.description, deadline),
            Mode::Bind => {
                let interface = interface.ok_or_else(|| LocalViewError::MissingInterfaceHash {
                    id: node.id.clone(),
                })?;
                Self::bind(&node.description, deadline, interface, [])
            }
        };
        Ok(local.with_comments(&node.comments))
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
        preimage.list(&canonical_comments(self.comments));

        match &self.discharge {
            Discharge::Decomposition(children) => {
                let mut entries: Vec<(String, &str)> = children
                    .iter()
                    .map(|child| (normalize::description(child.description), child.comment))
                    .collect();
                // Bytewise, on the normalized text and then the comment:
                // reordering a `children:` list is a filing change, not a
                // contract change.
                entries.sort_unstable();
                preimage.entries(&entries);
            }
            Discharge::Evidence {
                interface,
                assumptions,
            } => {
                preimage.field(interface.as_bytes());
                // Bytewise on the normalized text: which node an assumption
                // came from, and in what order the producer listed it, is
                // filing, not contract.
                let mut assumed: Vec<String> = assumptions
                    .iter()
                    .map(|assumption| normalize::description(assumption))
                    .collect();
                assumed.sort_unstable();
                preimage.list(&assumed);
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
/// Only the deadline group is annotation content today.
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
    if let Some(promote) = promote {
        pairs.push(("promote", promote));
    }
    pairs.sort_unstable_by_key(|&(key, _)| key);
    pairs
        .into_iter()
        .map(|(key, value)| format!("{key}:{value}"))
        .collect()
}

/// The canonical form of the comments on a node and its properties:
/// `anchor:text`, sorted bytewise.
///
/// The text is taken as attached: it is already NFC, with each line's `//`,
/// leading space, and trailing whitespace removed.
fn canonical_comments(comments: Option<&NodeComments>) -> Vec<String> {
    let Some(comments) = comments else {
        return Vec::new();
    };
    let mut canonical: Vec<String> = comments
        .block
        .iter()
        .map(|text| format!("block:{text}"))
        .chain(
            comments
                .properties
                .iter()
                .map(|(property, text)| format!("{}:{text}", property.name())),
        )
        .collect();
    canonical.sort_unstable();
    canonical
}

/// A node's direct child entries, in declaration order.
fn child_contracts<'a>(
    view: &'a ArgumentView,
    node: &'a ViewNode,
) -> Result<Vec<ChildContract<'a>>, LocalViewError> {
    node.children
        .iter()
        .map(|child| {
            let description = view
                .node(child)
                .map(|child| child.description.as_str())
                .ok_or_else(|| LocalViewError::UndefinedChild {
                    parent: node.id.clone(),
                    child: child.clone(),
                })?;
            // Keyed by child id, which is sound because a graph never holds the
            // same child twice under one parent: building it rejects that.
            let comment = node.comments.children.get(child).map_or("", String::as_str);
            Ok(ChildContract {
                description,
                comment,
            })
        })
        .collect()
}

/// The length-prefixed byte string a view hash is taken over.
#[derive(Default)]
pub(crate) struct Preimage(Vec<u8>);

impl Preimage {
    /// Appends `u64-LE(len) ‖ bytes`.
    pub(crate) fn field(&mut self, bytes: &[u8]) {
        self.0.extend_from_slice(&length(bytes.len()));
        self.0.extend_from_slice(bytes);
    }

    /// Appends `u64-LE(count)` followed by each element as a field.
    pub(crate) fn list<T: AsRef<[u8]>>(&mut self, items: &[T]) {
        self.0.extend_from_slice(&length(items.len()));
        for item in items {
            self.field(item.as_ref());
        }
    }

    /// Appends `u64-LE(count)` followed by each entry as two fields.
    pub(crate) fn entries<A: AsRef<[u8]>, B: AsRef<[u8]>>(&mut self, entries: &[(A, B)]) {
        self.0.extend_from_slice(&length(entries.len()));
        for (first, second) in entries {
            self.field(first.as_ref());
            self.field(second.as_ref());
        }
    }

    pub(crate) fn finish(self) -> Vec<u8> {
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

    use std::collections::BTreeMap;

    use crate::view::{Property, ViewEdge, ViewNode};

    const INTERFACE: InterfaceHash = InterfaceHash::from_bytes([0x11; DIGEST_LEN]);

    const NO_CHILDREN: [&str; 0] = [];

    fn deadline(until: &str, promote: Option<Mode>) -> Deadline {
        Deadline {
            until: until.to_owned(),
            promote,
        }
    }

    fn block_comment(text: &str) -> NodeComments {
        NodeComments {
            block: Some(text.to_owned()),
            ..NodeComments::default()
        }
    }

    fn property_comment(property: Property, text: &str) -> NodeComments {
        NodeComments {
            properties: BTreeMap::from([(property, text.to_owned())]),
            ..NodeComments::default()
        }
    }

    fn commented<'a>(description: &'a str, comment: &'a str) -> ChildContract<'a> {
        ChildContract {
            description,
            comment,
        }
    }

    #[test]
    fn the_four_kinds_never_share_a_hash_for_the_same_text() {
        // Arrange & Act: identical text, identical (absent) annotations.
        let objective = LocalView::objective("the claim", None, NO_CHILDREN).view_hash();
        let structure = LocalView::structure("the claim", None, NO_CHILDREN).view_hash();
        let fiat = LocalView::fiat("the claim", None).view_hash();
        let bind = LocalView::bind("the claim", None, INTERFACE, []).view_hash();

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
        let dated = deadline("M3", None);
        let dated = LocalView::fiat("taken as given", Some(&dated)).view_hash();

        assert_ne!(bare, dated);
    }

    #[test]
    fn every_part_of_a_deadline_changes_the_hash() {
        let base = deadline("M3", Some(Mode::Bind));
        let later = deadline("M4", Some(Mode::Bind));
        let repromoted = deadline("M3", Some(Mode::Structure));

        let hash = |d: &Deadline| LocalView::fiat("taken as given", Some(d)).view_hash();
        assert_ne!(hash(&base), hash(&later));
        assert_ne!(hash(&base), hash(&repromoted));
    }

    #[test]
    fn annotations_are_key_value_strings_sorted_by_key() {
        // Arrange
        let deadline = deadline("M3", Some(Mode::Bind));

        // Act
        let canonical = annotations(Some(&deadline));

        // Assert: the spelling `spec.md` fixes.
        assert_eq!(canonical, ["promote:bind", "until:M3"]);
    }

    #[test]
    fn comments_are_anchor_text_strings_sorted_bytewise() {
        // Arrange
        let comments = NodeComments {
            block: Some("shared with momentum_ok".to_owned()),
            properties: BTreeMap::from([
                (
                    Property::Until,
                    "awaiting the mass-properties test".to_owned(),
                ),
                (Property::Children, "grouped by axis".to_owned()),
            ]),
            children: BTreeMap::from([("a".to_owned(), "not listed here".to_owned())]),
        };

        // Act & Assert: the spelling `spec.md` fixes; child-entry comments
        // travel with the children instead.
        assert_eq!(
            canonical_comments(Some(&comments)),
            [
                "block:shared with momentum_ok",
                "children:grouped by axis",
                "until:awaiting the mass-properties test",
            ]
        );
    }

    #[test]
    fn a_comment_is_contract_content() {
        let bare = LocalView::fiat("taken as given", None).view_hash();
        let noted = block_comment("measured later");
        let noted = LocalView::fiat("taken as given", None)
            .with_comments(&noted)
            .view_hash();

        assert_ne!(bare, noted);
    }

    #[test]
    fn the_same_text_on_different_anchors_is_a_different_contract() {
        let on_block = block_comment("why");
        let on_until = property_comment(Property::Until, "why");
        let dated = deadline("M3", None);
        let hash = |comments: &NodeComments| {
            LocalView::fiat("taken as given", Some(&dated))
                .with_comments(comments)
                .view_hash()
        };

        assert_ne!(hash(&on_block), hash(&on_until));
    }

    #[test]
    fn a_comment_on_a_child_entry_changes_the_parents_hash() {
        let plain = LocalView::structure("parent", None, ["alpha"]).view_hash();
        let noted =
            LocalView::structure("parent", None, [commented("alpha", "why alpha")]).view_hash();

        assert_ne!(plain, noted);
    }

    #[test]
    fn reordering_commented_children_leaves_the_hash_alone() {
        let declared = LocalView::structure(
            "parent",
            None,
            [commented("alpha", "first"), commented("beta", "second")],
        )
        .view_hash();
        let reordered = LocalView::structure(
            "parent",
            None,
            [commented("beta", "second"), commented("alpha", "first")],
        )
        .view_hash();

        assert_eq!(declared, reordered);
    }

    #[test]
    fn a_child_comment_stays_with_its_child() {
        // Arrange & Act: the same two comments, swapped between the entries.
        let one = LocalView::structure(
            "parent",
            None,
            [commented("alpha", "x"), commented("beta", "y")],
        )
        .view_hash();
        let other = LocalView::structure(
            "parent",
            None,
            [commented("alpha", "y"), commented("beta", "x")],
        )
        .view_hash();

        // Assert
        assert_ne!(one, other);
    }

    #[test]
    fn a_node_without_a_deadline_has_no_annotations() {
        assert_eq!(annotations(None), [] as [String; 0]);
    }

    #[test]
    fn the_evidence_interface_hash_is_part_of_a_bind_hash() {
        // Arrange & Act: same contract, a different test behind it.
        let one = LocalView::bind("checked in CI", None, INTERFACE, []).view_hash();
        let other = LocalView::bind(
            "checked in CI",
            None,
            InterfaceHash::from_bytes([0x22; DIGEST_LEN]),
            [],
        )
        .view_hash();

        assert_ne!(one, other);
    }

    #[test]
    fn an_assumption_is_part_of_a_bind_hash() {
        // Arrange & Act: same contract and evidence, assuming more.
        let bare = LocalView::bind("checked in CI", None, INTERFACE, []).view_hash();
        let assuming =
            LocalView::bind("checked in CI", None, INTERFACE, ["the model holds"]).view_hash();

        // Assert
        assert_ne!(bare, assuming);
    }

    #[test]
    fn assumption_order_and_formatting_are_not_contract() {
        // Arrange & Act: the same two assumptions, listed differently.
        let declared = LocalView::bind(
            "checked in CI",
            None,
            INTERFACE,
            ["the model holds", "  the mass is known  "],
        )
        .view_hash();
        let reordered = LocalView::bind(
            "checked in CI",
            None,
            INTERFACE,
            ["the mass is known", "the model holds"],
        )
        .view_hash();

        // Assert: normalized, then sorted bytewise.
        assert_eq!(declared, reordered);
    }

    #[test]
    fn rewording_an_assumption_changes_a_bind_hash() {
        // Arrange & Act
        let before =
            LocalView::bind("checked in CI", None, INTERFACE, ["the model holds"]).view_hash();
        let after = LocalView::bind(
            "checked in CI",
            None,
            INTERFACE,
            ["the model holds in orbit"],
        )
        .view_hash();

        // Assert
        assert_ne!(before, after);
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
                    comments: NodeComments::default(),
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
                    comments: NodeComments {
                        children: BTreeMap::from([("leaf".to_owned(), "why".to_owned())]),
                        ..NodeComments::default()
                    },
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

        // Assert: the parent's own text plus its children's entries, and
        // nothing else.
        let comments = &view.nodes[1].comments;
        assert_eq!(
            local,
            LocalView::structure("the claim", None, [commented("taken as given", "why")])
                .with_comments(comments)
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
