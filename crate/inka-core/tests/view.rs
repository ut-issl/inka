//! Projects every checked-in fixture project and freezes the result.
//!
//! The snapshots are the point: the view is what `inka export`, the Markdown
//! report, and the server all render, so any drift in a node's text, its
//! namespace, or the order of anything shows up here first, as a reviewable
//! diff rather than as three outputs quietly disagreeing.

use std::collections::HashSet;

use inka_core::graph::Graph;
use inka_core::project::load_project;
use inka_core::view::{ArgumentView, Mode};

mod common;
use common::fixture_dir;

/// Loads a fixture project and projects it.
fn view_of(name: &str) -> ArgumentView {
    let project = load_project(&fixture_dir(name))
        .result
        .unwrap_or_else(|error| panic!("{name} should load: {error}"));
    let graph = Graph::build(&project.blocks)
        .unwrap_or_else(|errors| panic!("{name} should build a graph: {errors:?}"));
    ArgumentView::build(&graph, &project.source_map)
}

/// The invariants every projection holds, whatever the project.
fn assert_well_formed(name: &str, view: &ArgumentView) {
    let ids: Vec<&str> = view.nodes.iter().map(|node| node.id.as_str()).collect();

    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(
        ids, sorted,
        "{name}: nodes should be in sorted block-ID order"
    );

    let known: HashSet<&str> = ids.iter().copied().collect();
    assert_eq!(known.len(), ids.len(), "{name}: ids should be unique");

    for edge in &view.edges {
        assert!(
            known.contains(edge.parent.as_str()) && known.contains(edge.child.as_str()),
            "{name}: edge {} → {} should name projected nodes",
            edge.parent,
            edge.child
        );
    }

    for node in &view.nodes {
        assert!(
            !node.description.contains("{$"),
            "{name}: `{}` should be interpolated",
            node.id
        );
        assert!(
            !node.namespace.is_empty(),
            "{name}: `{}` should carry a namespace",
            node.id
        );
        assert_eq!(
            node.evidence.is_some(),
            node.mode == Mode::Bind,
            "{name}: `{}` carries evidence exactly when it is a bind",
            node.id
        );
        assert_eq!(
            node.children.is_empty(),
            matches!(node.mode, Mode::Bind | Mode::Fiat),
            "{name}: `{}` has children exactly when it decomposes",
            node.id
        );
    }

    for root in &view.roots {
        let node = view.node(root).expect("a root should be a projected node");
        assert_eq!(node.mode, Mode::Objective, "{name}: a root is an objective");
        assert!(
            !view.edges.iter().any(|edge| &edge.child == root),
            "{name}: nothing references the root `{root}`"
        );
    }
}

#[test]
fn aocs_project_projects_into_a_well_formed_view() {
    assert_well_formed("aocs", &view_of("aocs"));
}

#[test]
fn dogfood_project_projects_into_a_well_formed_view() {
    assert_well_formed("dogfood", &view_of("dogfood"));
}

#[test]
fn projection_is_deterministic() {
    // Arrange & Act: two independent loads of the same project.
    let first = view_of("aocs");
    let second = view_of("aocs");

    // Assert: the same project always projects to the same view.
    assert_eq!(first, second);
}

#[test]
fn aocs_view_matches_its_snapshot() {
    insta::assert_debug_snapshot!(view_of("aocs"));
}

#[test]
fn dogfood_view_matches_its_snapshot() {
    insta::assert_debug_snapshot!(view_of("dogfood"));
}
