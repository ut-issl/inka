//! What an edit does to a view hash, end to end from source text.
//!
//! The unit tests in `hash.rs` fix the layout; these fix the promise the
//! layout exists for. An approval survives exactly the edits that leave the
//! contract alone — renaming, retagging, editing comments, reordering — and
//! nothing else, so the cost of re-approval stays proportional to what
//! actually changed.

use std::collections::HashMap;

use inka_core::ast::Block;
use inka_core::graph::Graph;
use inka_core::hash::{InterfaceHash, LocalView};
use inka_core::interpolate::{RenderedDescription, interpolate};
use inka_core::lexer::Lexer;
use inka_core::parser::parse;
use inka_core::project::load_project;
use inka_core::source::SourceMap;
use inka_core::view::ArgumentView;

mod common;
use common::fixture_dir;

/// Stands in for a real evidence provider, which lands with the CI provider.
const INTERFACE: InterfaceHash = InterfaceHash::from_bytes([0x11; 32]);

/// Runs one synthetic argument file through the whole load pipeline.
fn view_of(source: &str, values: &[(&str, &str)]) -> ArgumentView {
    let values: HashMap<String, String> = values
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect();

    let mut source_map = SourceMap::new();
    let file_id = source_map.add_file("safety.inka".into(), source.to_owned());
    let tokens = Lexer::new(file_id, source)
        .lex()
        .expect("source should lex");
    let parsed = parse(file_id, &tokens);
    assert!(
        parsed.errors.is_empty(),
        "source should parse cleanly: {:?}",
        parsed.errors
    );
    let blocks: Vec<Block<RenderedDescription>> = parsed
        .blocks
        .into_iter()
        .map(|block| {
            let rendered = interpolate(&block.common().description, &values)
                .expect("source should interpolate");
            block.with_description(rendered)
        })
        .collect();
    let graph = Graph::build(&blocks).expect("source should build a graph");
    ArgumentView::build(&graph, &source_map)
}

/// The view hash of one node, with a stand-in interface hash for binds.
fn hash_of(view: &ArgumentView, id: &str) -> String {
    let node = view
        .node(id)
        .unwrap_or_else(|| panic!("the argument should define `{id}`"));
    LocalView::of_node(view, node, Some(INTERFACE))
        .unwrap_or_else(|error| panic!("`{id}` should assemble a local view: {error}"))
        .view_hash()
        .to_string()
}

/// Every node's hash, sorted, so two arguments can be compared without their
/// ids lining up.
fn hashes(view: &ArgumentView) -> Vec<String> {
    let mut hashes: Vec<String> = view
        .nodes
        .iter()
        .map(|node| hash_of(view, &node.id))
        .collect();
    hashes.sort();
    hashes
}

const ARGUMENT: &str = "\
objective root #safety {
  ```
  the satellite survives its first orbit
  ```
  children: branch, leaf
}
structure branch {
  ```
  power holds through eclipse
  ```
  children: leaf
}
fiat leaf {
  ```
  the battery is charged at separation
  ```
}
";

#[test]
fn renaming_ids_retagging_and_adding_a_file_comment_leaves_every_hash_alone() {
    // Arrange: the same three contracts under different names and tags, in a
    // file that carries a file comment.
    let renamed = "\
// A file comment, which belongs to no node.

objective survival #mission #reviewed {
  ```
  the satellite survives its first orbit
  ```
  children: power, battery
}
structure power {
  ```
  power holds through eclipse
  ```
  children: battery
}
fiat battery {
  ```
  the battery is charged at separation
  ```
}
";

    // Act & Assert: what an approver judged is the content, not its name.
    assert_eq!(
        hashes(&view_of(ARGUMENT, &[])),
        hashes(&view_of(renamed, &[]))
    );
}

#[test]
fn a_comment_changes_only_the_hash_of_the_node_it_is_attached_to() {
    // Arrange: one comment on the branch block, one on the root's entry for
    // the leaf.
    let on_block = ARGUMENT.replace(
        "structure branch {",
        "// Shared with the thermal argument.\nstructure branch {",
    );
    let on_entry = ARGUMENT.replace(
        "children: branch, leaf",
        "children: branch, leaf // Listed directly as well.",
    );
    let base = view_of(ARGUMENT, &[]);

    // Act
    let with_block_comment = view_of(&on_block, &[]);
    let with_entry_comment = view_of(&on_entry, &[]);

    // Assert: a block comment is the block's contract; an entry comment is
    // the parent's, and the child itself is untouched.
    assert_ne!(
        hash_of(&base, "branch"),
        hash_of(&with_block_comment, "branch")
    );
    assert_eq!(hash_of(&base, "root"), hash_of(&with_block_comment, "root"));
    assert_eq!(hash_of(&base, "leaf"), hash_of(&with_block_comment, "leaf"));
    assert_ne!(hash_of(&base, "root"), hash_of(&with_entry_comment, "root"));
    assert_eq!(hash_of(&base, "leaf"), hash_of(&with_entry_comment, "leaf"));
    assert_eq!(
        hash_of(&base, "branch"),
        hash_of(&with_entry_comment, "branch")
    );
}

#[test]
fn reordering_a_children_list_leaves_every_hash_alone() {
    // Arrange
    let reordered = ARGUMENT.replace("children: branch, leaf", "children: leaf, branch");

    // Act & Assert
    assert_eq!(
        hashes(&view_of(ARGUMENT, &[])),
        hashes(&view_of(&reordered, &[]))
    );
}

#[test]
fn editing_a_leaf_invalidates_that_leaf_and_its_parents_only() {
    // Arrange
    let before = view_of(ARGUMENT, &[]);
    let edited = ARGUMENT.replace(
        "the battery is charged at separation",
        "the battery is charged to 80% at separation",
    );
    let after = view_of(&edited, &[]);

    // Act & Assert: the leaf and both of its direct parents move.
    assert_ne!(hash_of(&before, "leaf"), hash_of(&after, "leaf"));
    assert_ne!(hash_of(&before, "branch"), hash_of(&after, "branch"));
    assert_ne!(hash_of(&before, "root"), hash_of(&after, "root"));
}

#[test]
fn editing_a_grandchild_leaves_the_grandparent_alone() {
    // Arrange: a three-deep chain, so `root` has a grandchild to edit.
    let chain = "\
objective root {
  ```
  the satellite survives its first orbit
  ```
  children: branch
}
structure branch {
  ```
  power holds through eclipse
  ```
  children: leaf
}
fiat leaf {
  ```
  the battery is charged at separation
  ```
}
";
    let before = view_of(chain, &[]);
    let after = view_of(
        &chain.replace(
            "the battery is charged at separation",
            "the battery is charged to 80% at separation",
        ),
        &[],
    );

    // Act & Assert: a hash is local, so the edit stops at `branch`.
    assert_ne!(hash_of(&before, "branch"), hash_of(&after, "branch"));
    assert_eq!(hash_of(&before, "root"), hash_of(&after, "root"));
}

#[test]
fn changing_an_interpolated_value_changes_the_hash() {
    // Arrange: interpolation runs before hashing, so a value is contract
    // content wherever it is referenced.
    let source = "\
objective root {
  ```
  detumbling completes within {$limit}
  ```
  children: leaf
}
fiat leaf {
  ```
  taken as given
  ```
}
";

    // Act
    let before = view_of(source, &[("limit", "2 orbits")]);
    let after = view_of(source, &[("limit", "3 orbits")]);

    // Assert: only the node that names the value moves.
    assert_ne!(hash_of(&before, "root"), hash_of(&after, "root"));
    assert_eq!(hash_of(&before, "leaf"), hash_of(&after, "leaf"));
}

#[test]
fn the_evidence_reference_string_is_excluded() {
    // Arrange: the same contract checked by two differently-named targets.
    let source = "\
objective root {
  ```
  the claim
  ```
  children: checked
}
bind checked {
  ```
  established by the detumble test
  ```
  test: \"tests/detumble.rs\"
}
";
    let renamed = source.replace(
        "test: \"tests/detumble.rs\"",
        "veriq: \"aocs.verify_detumble\"",
    );

    // Act & Assert: where the evidence lives is not what was approved — the
    // interface hash is what binds a bind to its evidence.
    assert_eq!(
        hash_of(&view_of(source, &[]), "checked"),
        hash_of(&view_of(&renamed, &[]), "checked")
    );
}

#[test]
fn moving_a_deadline_changes_the_hash() {
    // Arrange
    let source = "\
objective root {
  ```
  the claim
  ```
  children: leaf
}
fiat leaf {
  ```
  taken as given
  ```
  // measured once the vehicle exists
  until: M3
}
";
    let later = source.replace("until: M3", "until: M7");

    // Act & Assert: an `until:` is a jointly-approved decline placed toward
    // the future, so moving it is an edit to the contract.
    assert_ne!(
        hash_of(&view_of(source, &[]), "leaf"),
        hash_of(&view_of(&later, &[]), "leaf")
    );
}

#[test]
fn every_node_of_every_fixture_project_hashes_reproducibly() {
    for name in ["dogfood", "aocs"] {
        // Arrange
        let project = load_project(&fixture_dir(name))
            .result
            .unwrap_or_else(|error| panic!("{name} should load: {error}"));
        let graph = Graph::build(&project.blocks)
            .unwrap_or_else(|errors| panic!("{name} should build a graph: {errors:?}"));
        let view = ArgumentView::build(&graph, &project.source_map);
        assert!(!view.nodes.is_empty(), "{name} should project some nodes");

        // Act & Assert: eval's input is repository content only, so the same
        // commit hashes the same however often it is read.
        assert_eq!(hashes(&view), hashes(&view), "{name} should hash stably");
    }
}
