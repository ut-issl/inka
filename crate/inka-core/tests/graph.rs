//! Builds the argumentation graph for every checked-in fixture project and
//! asserts it validates as a clean DAG — the dogfood closing condition of
//! the graph-representation issue.

use inka_core::graph::{Graph, check};
use inka_core::project::load_project;

mod common;
use common::fixture_dir;

fn assert_clean_dag(name: &str) {
    // Arrange
    let project = load_project(&fixture_dir(name)).expect("fixture project should load");

    // Act
    let graph = Graph::build(&project.blocks)
        .unwrap_or_else(|diagnostics| panic!("{name} should build a graph: {diagnostics:?}"));

    // Assert: one node per block, a single objective, and a valid DAG.
    assert_eq!(graph.len(), project.blocks.len());
    assert_eq!(
        graph.objectives().len(),
        1,
        "{name} should have one objective"
    );
    assert!(graph.cycles().is_empty(), "{name} should be acyclic");
    let order = graph
        .topological_order()
        .unwrap_or_else(|| panic!("{name} should have a topological order"));
    assert_eq!(order.len(), graph.len());
    let diagnostics = graph.validate();
    assert!(
        diagnostics.is_empty(),
        "{name} should validate without diagnostics: {diagnostics:?}"
    );
    assert!(check(&project).is_empty());
}

#[test]
fn dogfood_project_builds_and_validates_as_a_dag() {
    assert_clean_dag("dogfood");
}

#[test]
fn aocs_project_builds_and_validates_as_a_dag() {
    assert_clean_dag("aocs");
}

#[test]
fn dogfood_topological_order_respects_every_edge() {
    // Arrange
    let project = load_project(&fixture_dir("dogfood")).expect("dogfood should load");
    let graph = Graph::build(&project.blocks).expect("dogfood should build a graph");
    let order = graph.topological_order().expect("dogfood should be a DAG");

    // Act
    let position: std::collections::HashMap<_, _> = order
        .iter()
        .enumerate()
        .map(|(position, &node)| (node, position))
        .collect();

    // Assert: every parent sits before each of its children.
    for node in graph.nodes() {
        for child in graph.children(node) {
            assert!(
                position[&node] < position[&child],
                "`{}` should precede its child `{}`",
                graph.id(node),
                graph.id(child)
            );
        }
    }
}

#[test]
fn dogfood_objective_reaches_every_node() {
    // Arrange
    let project = load_project(&fixture_dir("dogfood")).expect("dogfood should load");
    let graph = Graph::build(&project.blocks).expect("dogfood should build a graph");
    let objective = graph.objectives()[0];

    // Act
    let reachable = graph.reachable_from(objective);

    // Assert
    assert_eq!(reachable.len(), graph.len());
}
