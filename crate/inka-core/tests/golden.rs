//! Replays the frozen v1 hash vectors.
//!
//! The vectors under `tests/golden/hash/v1/` are what makes "frozen" mean
//! something. Every approval anyone records names a hash; if the layout that
//! produced it ever shifted — a reordered field, a dropped length prefix, a
//! normalization step that composes one character differently — every one of
//! those approvals would silently stop matching content nobody edited, and the
//! audit trail would be worthless. So the vectors are checked in, the
//! directory is append-only, and this test fails the moment a recomputed hash
//! stops equalling the recorded one.
//!
//! A failure here is therefore never fixed by updating the file. Either the
//! change that caused it is wrong, or the layout is genuinely moving on, in
//! which case it moves on as a parallel v2 with its own domain tags and its
//! own vector directory.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use inka_core::hash::{ChildContract, InterfaceHash, LocalView};
use inka_core::normalize;
use inka_core::view::{Deadline, Mode, NodeComments, Property};
use serde::Deserialize;

mod common;
use common::golden_dir;

/// One frozen vector: the inputs, the normal form they reduce to, and the hash.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Vector {
    kind: Kind,
    /// Rendered (post-interpolation) description, before normalization.
    description: String,
    /// What `description` normalizes to, recorded so a reader can see the
    /// pipeline rather than infer it from a digest.
    normalized: String,
    /// The direct children's entries, in declaration order.
    #[serde(default)]
    children: Vec<Child>,
    /// The evidence interface hash; bind vectors only.
    interface_hash: Option<String>,
    /// The rendered descriptions of the nodes the evidence assumes, in
    /// declaration order; bind vectors only.
    #[serde(default)]
    assumptions: Vec<String>,
    #[serde(default)]
    annotations: Annotations,
    #[serde(default)]
    comments: Comments,
    view_hash: String,
}

/// One child entry: the child's contract text and the comment on the entry.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Child {
    description: String,
    comment: Option<String>,
}

/// The merged comment text on the node and on each of its properties.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Comments {
    block: Option<String>,
    children: Option<String>,
    test: Option<String>,
    veriq: Option<String>,
    until: Option<String>,
    promote: Option<String>,
}

impl Comments {
    fn node_comments(&self) -> NodeComments {
        let properties = [
            (Property::Children, &self.children),
            (Property::Test, &self.test),
            (Property::Veriq, &self.veriq),
            (Property::Until, &self.until),
            (Property::Promote, &self.promote),
        ]
        .into_iter()
        .filter_map(|(property, text)| Some((property, text.clone()?)))
        .collect::<BTreeMap<_, _>>();
        NodeComments {
            block: self.block.clone(),
            properties,
            children: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Kind {
    Objective,
    Structure,
    Bind,
    Fiat,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct Annotations {
    until: Option<String>,
    /// The mode `promote:` names, spelled as the DSL spells it.
    promote: Option<String>,
}

impl Annotations {
    /// The deadline these annotations describe, if any.
    ///
    /// `promote` is anchored on `until:`, so a vector naming it without one is
    /// malformed rather than annotation-free.
    fn deadline(&self) -> Option<Deadline> {
        let Some(until) = &self.until else {
            assert!(self.promote.is_none(), "`promote` requires `until`");
            return None;
        };
        Some(Deadline {
            until: until.clone(),
            promote: self.promote.as_deref().map(promotion_target),
        })
    }
}

/// Reads the mode a `promote:` annotation names.
fn promotion_target(mode: &str) -> Mode {
    match mode {
        "objective" => Mode::Objective,
        "structure" => Mode::Structure,
        "bind" => Mode::Bind,
        "fiat" => Mode::Fiat,
        other => panic!("`{other}` is not a mode"),
    }
}

/// Every `*.toml` under the vector directory, in sorted path order.
fn vector_files() -> Vec<PathBuf> {
    let dir = golden_dir().join("hash/v1");
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("{} should be readable: {error}", dir.display()))
        .map(|entry| entry.expect("a directory entry should be readable").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "toml")
        })
        .collect();
    files.sort();
    files
}

fn read_vector(path: &Path) -> Vector {
    let text = fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("{} should be readable: {error}", path.display()));
    toml::from_str(&text)
        .unwrap_or_else(|error| panic!("{} should be a valid vector: {error}", path.display()))
}

/// Rebuilds the local view a vector describes.
fn local_view<'a>(
    vector: &'a Vector,
    deadline: Option<&'a Deadline>,
    comments: &'a NodeComments,
) -> LocalView<'a> {
    let children: Vec<ChildContract<'a>> = vector
        .children
        .iter()
        .map(|child| ChildContract {
            description: &child.description,
            comment: child.comment.as_deref().unwrap_or(""),
        })
        .collect();
    let local = match vector.kind {
        Kind::Objective => LocalView::objective(&vector.description, deadline, children),
        Kind::Structure => LocalView::structure(&vector.description, deadline, children),
        Kind::Fiat => LocalView::fiat(&vector.description, deadline),
        Kind::Bind => {
            let interface: InterfaceHash = vector
                .interface_hash
                .as_deref()
                .expect("a bind vector records an interface hash")
                .parse()
                .expect("a recorded interface hash should be a `sha256:<hex>` digest");
            LocalView::bind(
                &vector.description,
                deadline,
                interface,
                vector.assumptions.iter().map(String::as_str),
            )
        }
    };
    local.with_comments(comments)
}

#[test]
fn every_recorded_vector_still_hashes_to_its_recorded_value() {
    let files = vector_files();
    assert!(
        !files.is_empty(),
        "the v1 layout should be frozen by vectors"
    );

    for path in &files {
        // Arrange
        let vector = read_vector(path);
        let name = path.display();

        // Act
        let deadline = vector.annotations.deadline();
        let comments = vector.comments.node_comments();
        let local = local_view(&vector, deadline.as_ref(), &comments);

        // Assert
        assert_eq!(
            normalize::description(&vector.description),
            vector.normalized,
            "{name}: the recorded normal form should still be the normal form"
        );
        assert_eq!(
            local.view_hash().to_string(),
            vector.view_hash,
            "{name}: the v1 layout is frozen — a parallel v2 is how it changes"
        );
    }
}

#[test]
fn the_vector_set_covers_all_four_kinds() {
    // Arrange & Act: what each recorded vector exercises.
    let mut covered = [false; 4];
    for path in vector_files() {
        let slot = match read_vector(&path).kind {
            Kind::Objective => 0,
            Kind::Structure => 1,
            Kind::Bind => 2,
            Kind::Fiat => 3,
        };
        covered[slot] = true;
    }

    // Assert: a kind with no vector has an unfrozen layout.
    assert_eq!(
        covered, [true; 4],
        "every hash kind needs at least one vector (objective, structure, bind, fiat)"
    );
}
