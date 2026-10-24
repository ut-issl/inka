//! Graph representation of a project's argumentation structure.
//!
//! Nodes are blocks and an edge points from a parent block to each child it
//! names, so the graph is the whole argument: objectives discharged through
//! structures down to bind and fiat leaves. petgraph's [`DiGraph`] is the
//! underlying storage, and the structural features — topological order,
//! cycle enumeration, reachability — lean on its algorithms.
//!
//! Node ids are assigned in sorted block-ID order, not file or declaration
//! order, so every derived output (diagnostics, orders, snapshots) is stable
//! under file renames, splits, and block reordering.
//!
//! Construction is two passes over the loaded blocks: registration, which
//! reports duplicate ids, and resolution, which reports duplicate and
//! undefined references. A built graph therefore has exactly one node per
//! id and at most one edge per parent–child pair. Whole-graph judgements —
//! a missing objective, cycles, reachability — live in [`Graph::validate`],
//! which reports them as diagnostics without consuming the graph.

use std::collections::{HashMap, HashSet};

use petgraph::Direction;
use petgraph::algo::{tarjan_scc, toposort};
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::Dfs;

use crate::ast::Block;
use crate::interpolate::RenderedDescription;
use crate::project::Project;
use crate::source::{Diagnostics, Error, Warning};

/// Identity of a node in the argument graph.
///
/// Ids are dense indices assigned in sorted block-ID order and minted only
/// by [`Graph::build`], mirroring the [`crate::source::FileId`] pattern.
/// `Ord` therefore follows block-ID order: sorting by `NodeId` sorts by
/// block ID. The inner index is private, so petgraph never leaks into the
/// public API.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId(NodeIndex);

impl NodeId {
    fn from_index(index: NodeIndex) -> Self {
        NodeId(index)
    }

    fn index(self) -> NodeIndex {
        self.0
    }
}

/// The argumentation graph of one loaded project.
///
/// Borrows the project's blocks rather than cloning them: the graph is a
/// structural view, and the [`Project`] stays the owner of the argument.
#[derive(Debug, Clone)]
pub struct Graph<'p> {
    /// Parent → child edges; node weights are the blocks themselves, added
    /// in sorted block-ID order so [`NodeId`]s are deterministic.
    graph: DiGraph<&'p Block<RenderedDescription>, ()>,
    /// Block ID → node, for reference resolution and lookups.
    index: HashMap<&'p str, NodeId>,
}

impl<'p> Graph<'p> {
    /// Builds the graph in two passes: register every block, then resolve
    /// every child reference to an edge.
    ///
    /// # Errors
    ///
    /// Returns every duplicate-id, duplicate-reference, and
    /// undefined-reference diagnostic, with spans on the offending id or
    /// reference. Both passes run to completion before returning, so one
    /// bad id does not hide another.
    pub fn build(blocks: &'p [Block<RenderedDescription>]) -> Result<Self, Vec<Error>> {
        let (graph, errors) = Self::build_collecting(blocks);
        if errors.is_empty() {
            Ok(graph)
        } else {
            Err(errors)
        }
    }

    /// Builds the usable portion of the graph while retaining construction
    /// errors, so `inka check` can continue with shape validation.
    fn build_collecting(blocks: &'p [Block<RenderedDescription>]) -> (Self, Vec<Error>) {
        let mut errors = Vec::new();

        // Registration: the first definition of an id wins its slot; every
        // later definition is an error anchored on the duplicate.
        let mut definitions: HashMap<&str, &Block<RenderedDescription>> = HashMap::new();
        for block in blocks {
            let id = &block.common().id;
            if definitions.contains_key(id.value.as_str()) {
                errors.push(Error::new(id.span, format!("duplicate id `{}`", id.value)));
            } else {
                definitions.insert(&id.value, block);
            }
        }

        // Interning in sorted block-ID order keeps NodeIds independent of
        // file layout and declaration order.
        let mut ids: Vec<&str> = definitions.keys().copied().collect();
        ids.sort_unstable();
        let mut graph = DiGraph::with_capacity(ids.len(), ids.len());
        let mut index = HashMap::with_capacity(ids.len());
        for id in ids {
            let node = graph.add_node(definitions[id]);
            index.insert(id, NodeId::from_index(node));
        }

        // Resolution: each child reference becomes an edge or a diagnostic.
        // The node set is fixed by now, so the index range enumerates every
        // parent without holding a borrow on the graph.
        for parent in (0..graph.node_count()).map(NodeIndex::new) {
            let mut seen: HashSet<&str> = HashSet::new();
            for child in graph[parent].children() {
                if !seen.insert(child.value.as_str()) {
                    errors.push(Error::new(
                        child.span,
                        format!("duplicate reference `{}`", child.value),
                    ));
                    continue;
                }
                match index.get(child.value.as_str()) {
                    Some(child_node) => {
                        graph.add_edge(parent, child_node.index(), ());
                    }
                    None => errors.push(Error::new(
                        child.span,
                        format!("undefined reference `{}`", child.value),
                    )),
                }
            }
        }

        (Self { graph, index }, errors)
    }

    /// Number of nodes in the graph.
    #[must_use]
    pub fn len(&self) -> usize {
        self.graph.node_count()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.graph.node_count() == 0
    }

    /// Every node, in sorted block-ID order.
    pub fn nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.graph.node_indices().map(NodeId::from_index)
    }

    /// Resolves a block ID to its node.
    #[must_use]
    pub fn node(&self, id: &str) -> Option<NodeId> {
        self.index.get(id).copied()
    }

    /// The block behind a node.
    ///
    /// # Panics
    ///
    /// Panics when `node` was minted by a different, larger graph.
    #[must_use]
    pub fn block(&self, node: NodeId) -> &'p Block<RenderedDescription> {
        self.graph[node.index()]
    }

    /// The block ID behind a node.
    #[must_use]
    pub fn id(&self, node: NodeId) -> &'p str {
        &self.block(node).common().id.value
    }

    /// Children of a node, in the order the block declares them.
    ///
    /// Reads the block's own child list rather than petgraph's edge storage:
    /// declaration order is then self-evident, and `build` guarantees every
    /// reference resolves.
    #[must_use]
    pub fn children(&self, node: NodeId) -> Vec<NodeId> {
        self.block(node)
            .children()
            .iter()
            .map(|child| self.index[child.value.as_str()])
            .collect()
    }

    /// Parents of a node, in sorted block-ID order.
    ///
    /// The reverse direction is what later passes (blame, stale propagation)
    /// walk when a change must be traced back toward the objective.
    #[must_use]
    pub fn parents(&self, node: NodeId) -> Vec<NodeId> {
        let mut parents: Vec<NodeId> = self
            .graph
            .neighbors_directed(node.index(), Direction::Incoming)
            .map(NodeId::from_index)
            .collect();
        parents.sort_unstable();
        parents
    }

    /// Every objective node, in sorted block-ID order.
    #[must_use]
    pub fn objectives(&self) -> Vec<NodeId> {
        self.nodes()
            .filter(|&node| matches!(self.block(node), Block::Objective(_)))
            .collect()
    }

    /// A topological order: every parent before all of its children.
    ///
    /// Returns `None` when the graph has a cycle; [`Graph::cycles`] names it.
    #[must_use]
    pub fn topological_order(&self) -> Option<Vec<NodeId>> {
        toposort(&self.graph, None)
            .ok()
            .map(|order| order.into_iter().map(NodeId::from_index).collect())
    }

    /// One representative cycle per strongly connected component, so an
    /// empty result proves the graph is a DAG.
    ///
    /// Each cycle lists its nodes in edge order starting from the component's
    /// smallest node; the closing edge back to the start is implied.
    #[must_use]
    pub fn cycles(&self) -> Vec<Vec<NodeId>> {
        let mut cycles: Vec<Vec<NodeId>> = tarjan_scc(&self.graph)
            .into_iter()
            .filter_map(|component| self.cycle_within(&component))
            .collect();
        cycles.sort_unstable();
        cycles
    }

    /// Every node reachable from `start` along child edges, including
    /// `start` itself.
    #[must_use]
    pub fn reachable_from(&self, start: NodeId) -> HashSet<NodeId> {
        let mut dfs = Dfs::new(&self.graph, start.index());
        let mut reachable = HashSet::new();
        while let Some(node) = dfs.next(&self.graph) {
            reachable.insert(NodeId::from_index(node));
        }
        reachable
    }

    /// Structural diagnostics over the whole graph.
    ///
    /// `build` already rejected duplicate ids and dangling references, so
    /// everything here is a judgement on shape:
    ///
    /// - no objective block, including an empty graph → error, unanchored
    ///   because it concerns the project as a whole
    /// - a dependency cycle → error naming the full path
    /// - a node no objective reaches → warning, in the warnings lane so it
    ///   never fails the check
    ///
    /// Any number of objectives is legal; reachability is judged against
    /// all of them together, so a node is an orphan only when every
    /// objective misses it.
    #[must_use]
    pub fn validate(&self) -> Diagnostics {
        let mut diagnostics = Diagnostics::new();
        let objectives = self.objectives();

        if objectives.is_empty() {
            diagnostics
                .errors
                .push(Error::unanchored("no objective block found"));
        }

        for cycle in self.cycles() {
            let path = cycle
                .iter()
                .chain(cycle.first())
                .map(|&node| self.id(node))
                .collect::<Vec<_>>()
                .join(" → ");
            diagnostics.errors.push(Error::new(
                self.block(cycle[0]).common().id.span,
                format!("dependency cycle: {path}"),
            ));
        }

        if !objectives.is_empty() {
            let reachable: HashSet<NodeId> = objectives
                .iter()
                .flat_map(|&objective| self.reachable_from(objective))
                .collect();
            for node in self.nodes() {
                if !reachable.contains(&node) {
                    diagnostics.warnings.push(Warning::new(
                        self.block(node).common().id.span,
                        format!("`{}` is unreachable from any objective", self.id(node)),
                    ));
                }
            }
        }

        diagnostics
    }

    /// Finds a cycle inside one strongly connected component, or `None` when
    /// the component is a single node without a self-loop.
    fn cycle_within(&self, component: &[NodeIndex]) -> Option<Vec<NodeId>> {
        let start = *component.iter().min()?;
        if component.len() == 1 {
            return self
                .graph
                .find_edge(start, start)
                .map(|_| vec![NodeId::from_index(start)]);
        }

        let members: HashSet<NodeIndex> = component.iter().copied().collect();
        // Iterative DFS from `start`, restricted to the component. Every
        // component node reaches `start`, and each neighbor of a stacked node
        // is examined while that node is still on the stack, so the search
        // always closes the cycle before the stack empties.
        let mut visited = HashSet::from([start]);
        let mut stack = vec![(start, self.component_neighbors(start, &members))];
        loop {
            let Some((_, neighbors)) = stack.last_mut() else {
                unreachable!("a multi-node strongly connected component contains a cycle");
            };
            match neighbors.pop() {
                None => {
                    stack.pop();
                }
                Some(next) if next == start => {
                    return Some(
                        stack
                            .iter()
                            .map(|&(node, _)| NodeId::from_index(node))
                            .collect(),
                    );
                }
                Some(next) => {
                    if visited.insert(next) {
                        let next_neighbors = self.component_neighbors(next, &members);
                        stack.push((next, next_neighbors));
                    }
                }
            }
        }
    }

    /// Out-neighbors of `node` inside the component, sorted descending so
    /// that popping from the back visits the smallest node first.
    fn component_neighbors(&self, node: NodeIndex, members: &HashSet<NodeIndex>) -> Vec<NodeIndex> {
        let mut neighbors: Vec<NodeIndex> = self
            .graph
            .neighbors_directed(node, Direction::Outgoing)
            .filter(|neighbor| members.contains(neighbor))
            .collect();
        neighbors.sort_unstable_by(|a, b| b.cmp(a));
        neighbors
    }
}

/// Builds the graph for a loaded project and runs every structural check.
///
/// This is the graph half of `inka check`: construction problems (duplicate
/// ids, duplicate or undefined references) and shape problems (a missing
/// objective, cycles, unreachable nodes) come back as one [`Diagnostics`]
/// either way. A project with no blocks has no objective, so an empty
/// argument is rejected here too.
#[must_use]
pub fn check(project: &Project) -> Diagnostics {
    check_blocks(&project.blocks)
}

/// Checks graph construction and shape in one pass. A partially resolved
/// graph is sufficient for cycle and reachability analysis, so construction
/// errors do not hide independent shape findings.
fn check_blocks(blocks: &[Block<RenderedDescription>]) -> Diagnostics {
    let (graph, mut errors) = Graph::build_collecting(blocks);
    let Diagnostics {
        errors: shape_errors,
        warnings,
    } = graph.validate();
    errors.extend(shape_errors);
    Diagnostics { errors, warnings }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::interpolate::interpolate;
    use crate::lexer::Lexer;
    use crate::parser::parse;
    use crate::source::SourceMap;

    /// Lexes, parses, and interpolates one synthetic file into blocks, the
    /// same pipeline the project loader runs.
    fn blocks(source: &str) -> Vec<Block<RenderedDescription>> {
        let mut map = SourceMap::new();
        let file_id = map.add_file("test.inka".into(), source.to_owned());
        let tokens = Lexer::new(file_id, source)
            .lex()
            .expect("test source should lex");
        let parsed = parse(file_id, &tokens);
        assert!(
            parsed.errors.is_empty(),
            "test source should parse cleanly: {:?}",
            parsed.errors
        );
        let values = HashMap::new();
        parsed
            .blocks
            .into_iter()
            .map(|block| {
                let rendered = interpolate(&block.common().description, &values)
                    .expect("test source should interpolate");
                block.with_description(rendered)
            })
            .collect()
    }

    /// Builds a graph expected to fail, returning its errors.
    fn build_errors(source: &str) -> Vec<Error> {
        Graph::build(&blocks(source)).expect_err("caller expects build errors")
    }

    fn ids<'g>(graph: &Graph<'g>, nodes: &[NodeId]) -> Vec<&'g str> {
        nodes.iter().map(|&node| graph.id(node)).collect()
    }

    const CLEAN_TREE: &str = "\
objective root {
  ```
  the root claim
  ```
  children: left, right
}
structure left {
  ```
  left branch
  ```
  children: leaf
}
fiat right {
  ```
  right leaf
  ```
}
fiat leaf {
  ```
  shared leaf
  ```
}
";

    #[test]
    fn assigns_node_ids_in_sorted_id_order() {
        // Arrange: declaration order differs from sorted order.
        let blocks = blocks(
            "\
objective z_root {
  ```
  root
  ```
  children: b, a
}
fiat b {
  ```
  b
  ```
}
fiat a {
  ```
  a
  ```
}
",
        );

        // Act
        let graph = Graph::build(&blocks).expect("graph should build");

        // Assert: nodes iterate in sorted block-ID order, not file order.
        let all: Vec<NodeId> = graph.nodes().collect();
        assert_eq!(ids(&graph, &all), ["a", "b", "z_root"]);
        assert!(graph.node("a") < graph.node("b"));
    }

    #[test]
    fn resolves_children_to_edges_in_declaration_order() {
        // Arrange & Act
        let blocks = blocks(CLEAN_TREE);
        let graph = Graph::build(&blocks).expect("graph should build");

        // Assert: children keep declaration order, parents resolve.
        let root = graph.node("root").expect("root should exist");
        assert_eq!(ids(&graph, &graph.children(root)), ["left", "right"]);
        let leaf = graph.node("leaf").expect("leaf should exist");
        assert_eq!(ids(&graph, &graph.parents(leaf)), ["left"]);
    }

    #[test]
    fn reports_duplicate_ids_at_the_redefinition() {
        // Arrange & Act
        let diagnostics = build_errors(
            "\
fiat a {
  ```
  first
  ```
}
fiat a {
  ```
  second
  ```
}
",
        );

        // Assert
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].message, "duplicate id `a`");
    }

    #[test]
    fn reports_undefined_references_at_the_reference() {
        // Arrange & Act
        let diagnostics = build_errors(
            "\
objective root {
  ```
  root
  ```
  children: ghost
}
",
        );

        // Assert
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].message, "undefined reference `ghost`");
    }

    #[test]
    fn collects_every_build_error_before_failing() {
        // Arrange & Act: one duplicate and one undefined reference.
        let diagnostics = build_errors(
            "\
objective root {
  ```
  root
  ```
  children: a, ghost
}
fiat a {
  ```
  a
  ```
}
fiat a {
  ```
  again
  ```
}
",
        );

        // Assert
        let messages: Vec<&str> = diagnostics.iter().map(|d| d.message.as_str()).collect();
        assert_eq!(
            messages,
            ["duplicate id `a`", "undefined reference `ghost`"]
        );
    }

    #[test]
    fn check_collects_build_errors_and_shape_findings() {
        // Arrange: the dangling reference is a construction error, while the
        // cycle and orphan are independent findings on the resolved portion.
        let blocks = blocks(
            "\
fiat orphan {
  ```
  orphan
  ```
}
objective root {
  ```
  root
  ```
  children: a
}
structure a {
  ```
  cycle
  ```
  children: a, missing
}
",
        );

        // Act
        let diagnostics = check_blocks(&blocks);

        // Assert
        let error_messages: Vec<&str> = diagnostics
            .errors
            .iter()
            .map(|error| error.message.as_str())
            .collect();
        assert_eq!(
            error_messages,
            ["undefined reference `missing`", "dependency cycle: a → a"]
        );
        assert_eq!(diagnostics.warnings.len(), 1);
        assert_eq!(
            diagnostics.warnings[0].message,
            "`orphan` is unreachable from any objective"
        );
    }

    #[test]
    fn reports_duplicate_child_references_at_the_reference() {
        // Arrange & Act: the same child is listed twice in one block.
        let diagnostics = build_errors(
            "\
objective root {
  ```
  root
  ```
  children: a, a
}
fiat a {
  ```
  a
  ```
}
",
        );

        // Assert
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].message, "duplicate reference `a`");
    }

    #[test]
    fn topological_order_puts_parents_before_children() {
        // Arrange
        let blocks = blocks(CLEAN_TREE);
        let graph = Graph::build(&blocks).expect("graph should build");

        // Act
        let order = graph
            .topological_order()
            .expect("a tree should have a topological order");

        // Assert
        let position = |id: &str| {
            let node = graph.node(id).expect("node should exist");
            order
                .iter()
                .position(|&entry| entry == node)
                .expect("every node should appear in the order")
        };
        assert_eq!(order.len(), 4);
        assert!(position("root") < position("left"));
        assert!(position("root") < position("right"));
        assert!(position("left") < position("leaf"));
    }

    #[test]
    fn cycle_disables_topological_order_and_is_reported_with_its_path() {
        // Arrange
        let blocks = blocks(
            "\
objective root {
  ```
  root
  ```
  children: a
}
structure a {
  ```
  a
  ```
  children: b
}
structure b {
  ```
  b
  ```
  children: c
}
structure c {
  ```
  c
  ```
  children: a
}
",
        );
        let graph = Graph::build(&blocks).expect("graph should build");

        // Act
        let cycles = graph.cycles();
        let diagnostics = graph.validate();

        // Assert
        assert_eq!(graph.topological_order(), None);
        assert_eq!(cycles.len(), 1);
        assert_eq!(ids(&graph, &cycles[0]), ["a", "b", "c"]);
        assert!(
            diagnostics
                .errors
                .iter()
                .any(|error| error.message == "dependency cycle: a → b → c → a"),
            "expected a cycle error, got {diagnostics:?}"
        );
    }

    #[test]
    fn reports_a_self_loop_as_a_cycle() {
        // Arrange
        let blocks = blocks(
            "\
objective root {
  ```
  root
  ```
  children: a
}
structure a {
  ```
  a
  ```
  children: a
}
",
        );
        let graph = Graph::build(&blocks).expect("graph should build");

        // Act
        let diagnostics = graph.validate();

        // Assert
        assert_eq!(graph.cycles().len(), 1);
        assert!(
            diagnostics
                .errors
                .iter()
                .any(|error| error.message == "dependency cycle: a → a"),
            "expected a self-loop error, got {diagnostics:?}"
        );
    }

    #[test]
    fn validate_requires_an_objective() {
        // Arrange
        let blocks = blocks("fiat a {\n  ```\n  a\n  ```\n}\n");
        let graph = Graph::build(&blocks).expect("graph should build");

        // Act
        let diagnostics = graph.validate();

        // Assert: the judgement concerns the whole project, so no span.
        assert_eq!(diagnostics.errors.len(), 1);
        assert_eq!(diagnostics.errors[0].message, "no objective block found");
        assert_eq!(diagnostics.errors[0].span, None);
        assert_eq!(diagnostics.warnings, []);
    }

    #[test]
    fn validate_reports_missing_objective_for_an_empty_graph() {
        // Arrange
        let graph = Graph::build(&[]).expect("an empty graph should build");

        // Act
        let diagnostics = graph.validate();

        // Assert
        assert_eq!(diagnostics.errors.len(), 1);
        assert_eq!(diagnostics.errors[0].message, "no objective block found");
        assert_eq!(diagnostics.errors[0].span, None);
    }

    #[test]
    fn validate_accepts_multiple_objectives() {
        // Arrange: several objectives are legal as long as together they
        // reach every node.
        let blocks = blocks(
            "\
objective one {
  ```
  one
  ```
  children: shared
}
objective two {
  ```
  two
  ```
  children: shared
}
fiat shared {
  ```
  shared
  ```
}
",
        );
        let graph = Graph::build(&blocks).expect("graph should build");

        // Act & Assert
        assert!(graph.validate().is_empty());
    }

    #[test]
    fn validate_warns_on_nodes_unreachable_from_any_objective() {
        // Arrange
        let blocks = blocks(
            "\
objective root {
  ```
  root
  ```
  children: a
}
fiat a {
  ```
  a
  ```
}
fiat orphan {
  ```
  orphan
  ```
}
",
        );
        let graph = Graph::build(&blocks).expect("graph should build");

        // Act
        let diagnostics = graph.validate();

        // Assert: the finding sits in the warnings lane, so the check
        // still passes.
        assert!(!diagnostics.has_errors());
        assert_eq!(diagnostics.warnings.len(), 1);
        assert_eq!(
            diagnostics.warnings[0].message,
            "`orphan` is unreachable from any objective"
        );
    }

    #[test]
    fn unreachability_is_judged_against_every_objective_together() {
        // Arrange: each objective reaches one leaf; only `orphan` is
        // reached by neither.
        let blocks = blocks(
            "\
objective one {
  ```
  one
  ```
  children: a
}
objective two {
  ```
  two
  ```
  children: b
}
fiat a {
  ```
  a
  ```
}
fiat b {
  ```
  b
  ```
}
fiat orphan {
  ```
  orphan
  ```
}
",
        );
        let graph = Graph::build(&blocks).expect("graph should build");

        // Act
        let diagnostics = graph.validate();

        // Assert: neither `a` nor `b` is flagged, only the true orphan.
        assert!(!diagnostics.has_errors());
        assert_eq!(diagnostics.warnings.len(), 1);
        assert_eq!(
            diagnostics.warnings[0].message,
            "`orphan` is unreachable from any objective"
        );
    }

    #[test]
    fn validate_accepts_a_clean_tree() {
        // Arrange
        let blocks = blocks(CLEAN_TREE);
        let graph = Graph::build(&blocks).expect("graph should build");

        // Act & Assert
        assert!(graph.validate().is_empty());
    }

    #[test]
    fn reachable_from_includes_the_start_and_all_descendants() {
        // Arrange
        let blocks = blocks(CLEAN_TREE);
        let graph = Graph::build(&blocks).expect("graph should build");
        let root = graph.node("root").expect("root should exist");
        let left = graph.node("left").expect("left should exist");

        // Act
        let from_root = graph.reachable_from(root);
        let from_left = graph.reachable_from(left);

        // Assert
        assert_eq!(from_root.len(), 4);
        let mut left_ids: Vec<&str> = from_left.iter().map(|&node| graph.id(node)).collect();
        left_ids.sort_unstable();
        assert_eq!(left_ids, ["leaf", "left"]);
    }

    #[test]
    fn shared_children_have_every_parent() {
        // Arrange: DAG sharing — two structures reference the same leaf.
        let blocks = blocks(
            "\
objective root {
  ```
  root
  ```
  children: a, b
}
structure a {
  ```
  a
  ```
  children: shared
}
structure b {
  ```
  b
  ```
  children: shared
}
fiat shared {
  ```
  shared
  ```
}
",
        );
        let graph = Graph::build(&blocks).expect("graph should build");

        // Act
        let shared = graph.node("shared").expect("shared should exist");

        // Assert
        assert_eq!(ids(&graph, &graph.parents(shared)), ["a", "b"]);
        assert!(graph.validate().is_empty());
    }
}
