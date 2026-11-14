//! Graph representation of a project's argumentation structure.
//!
//! Nodes are blocks and an edge points from a parent block to each child it
//! names, so the graph is the whole argument: objectives discharged through
//! structures down to bind and fiat leaves. A second kind of edge comes from
//! imported calc evidence: a `calc:` bind depends on every argument node its
//! evidence assumes, so an assumption edge points from the bind to the
//! assumed node. Both kinds are dependencies — a node is established only
//! after what it points at — and both take part in cycle detection and
//! reachability; only `children`/`parents` read the declared structure alone.
//! petgraph's [`DiGraph`] is the underlying storage, and the structural
//! features — topological order, cycle enumeration, reachability — lean on
//! its algorithms.
//!
//! Node ids are assigned in sorted block-ID order, not file or declaration
//! order, so every derived output (diagnostics, orders, snapshots) is stable
//! under file renames, splits, and block reordering.
//!
//! Construction is two passes over the loaded blocks: registration, which
//! reports duplicate ids, and resolution, which reports duplicate and
//! undefined references and undefined assumptions. A built graph therefore
//! has exactly one node per id and at most one edge per node pair.
//! Whole-graph judgements — a missing objective, cycles, reachability — live
//! in [`Graph::validate`], which reports them as diagnostics without
//! consuming the graph.

use std::collections::{HashMap, HashSet};

use petgraph::Direction;
use petgraph::algo::{tarjan_scc, toposort};
use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::{Dfs, EdgeFiltered, EdgeRef};

use crate::ast::{Block, SourceRef};
use crate::calc_evidence::CalcEvidenceSet;
use crate::deadline;
use crate::interpolate::RenderedDescription;
use crate::project::Project;
use crate::source::{Diagnostics, Error, Span, Warning};

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

/// Why one node depends on another.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dependency {
    /// The parent names the node in its `children:` list.
    Child,
    /// The bind's calc evidence assumes the node.
    Assumption,
}

impl Dependency {
    /// The arrow a rendered cycle draws for this edge: a child edge is a
    /// plain arrow, an assumption edge a dashed one.
    const fn arrow(self) -> &'static str {
        match self {
            Dependency::Child => "→",
            Dependency::Assumption => "⇢",
        }
    }
}

/// The argumentation graph of one loaded project.
///
/// Borrows the project's blocks rather than cloning them: the graph is a
/// structural view, and the [`Project`] stays the owner of the argument.
#[derive(Debug, Clone)]
pub struct Graph<'p> {
    /// Dependency edges — parent → child and bind → assumed node — tagged
    /// with their kind; node weights are the blocks themselves, added in
    /// sorted block-ID order so [`NodeId`]s are deterministic.
    graph: DiGraph<&'p Block<RenderedDescription>, Dependency>,
    /// Block ID → node, for reference resolution and lookups.
    index: HashMap<&'p str, NodeId>,
}

impl<'p> Graph<'p> {
    /// Builds the graph of the declared structure alone, as if no calc
    /// evidence were imported: [`Graph::build_with`] over an empty set.
    ///
    /// # Errors
    ///
    /// As for [`Graph::build_with`].
    pub fn build(blocks: &'p [Block<RenderedDescription>]) -> Result<Self, Vec<Error>> {
        Self::build_with(blocks, &CalcEvidenceSet::default())
    }

    /// Builds the graph of a loaded project: its blocks with the calc
    /// evidence it imports.
    ///
    /// # Errors
    ///
    /// As for [`Graph::build_with`].
    pub fn of_project(project: &'p Project) -> Result<Self, Vec<Error>> {
        Self::build_with(&project.blocks, &project.calc_evidence)
    }

    /// Builds the graph in two passes: register every block, then resolve
    /// every child reference and every assumption of a `calc:` bind's
    /// imported evidence to an edge.
    ///
    /// A `calc:` reference no loaded target answers adds no edge and is not
    /// an error here: that is evaluation's missing evidence.
    ///
    /// # Errors
    ///
    /// Returns every duplicate-id, duplicate-reference, undefined-reference
    /// and undefined-assumption diagnostic, with spans on the offending id,
    /// reference or `calc:` property. Both passes run to completion before
    /// returning, so one bad id does not hide another.
    pub fn build_with(
        blocks: &'p [Block<RenderedDescription>],
        calc_evidence: &CalcEvidenceSet,
    ) -> Result<Self, Vec<Error>> {
        let (graph, errors) = Self::build_collecting(blocks, calc_evidence);
        if errors.is_empty() {
            Ok(graph)
        } else {
            Err(errors)
        }
    }

    /// Builds the usable portion of the graph while retaining construction
    /// errors, so `inka check` can continue with shape validation.
    fn build_collecting(
        blocks: &'p [Block<RenderedDescription>],
        calc_evidence: &CalcEvidenceSet,
    ) -> (Self, Vec<Error>) {
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
            for child in graph[parent].children().iter().map(|entry| &entry.id) {
                if !seen.insert(child.value.as_str()) {
                    errors.push(Error::new(
                        child.span,
                        format!("duplicate reference `{}`", child.value),
                    ));
                    continue;
                }
                match index.get(child.value.as_str()) {
                    Some(child_node) => {
                        graph.add_edge(parent, child_node.index(), Dependency::Child);
                    }
                    None => errors.push(Error::new(
                        child.span,
                        format!("undefined reference `{}`", child.value),
                    )),
                }
            }
        }

        // Assumptions: a calc bind whose evidence is loaded depends on every
        // node that evidence assumes. The wire format already rules out a
        // repeated id within one target, so each pair gets one edge.
        for bind in (0..graph.node_count()).map(NodeIndex::new) {
            let Block::Bind(block) = graph[bind] else {
                continue;
            };
            let SourceRef::Calc(target) = &block.source.value else {
                continue;
            };
            let Some(evidence) = calc_evidence.get(target) else {
                continue;
            };
            for assumed in &evidence.assumes {
                match index.get(assumed.as_str()) {
                    Some(node) => {
                        graph.add_edge(bind, node.index(), Dependency::Assumption);
                    }
                    None => errors.push(Error::new(
                        block.source.span,
                        format!(
                            "undefined assumption `{assumed}`: the calc evidence for `{target}` assumes it, but no block defines it"
                        ),
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
            .map(|child| self.index[child.id.value.as_str()])
            .collect()
    }

    /// Parents of a node — the blocks naming it in `children:` — in sorted
    /// block-ID order. A bind assuming the node is not a parent.
    ///
    /// The reverse direction is what later passes (blame, stale propagation)
    /// walk when a change must be traced back toward the objective.
    #[must_use]
    pub fn parents(&self, node: NodeId) -> Vec<NodeId> {
        self.neighbors_of_kind(node, Direction::Incoming, Dependency::Child)
    }

    /// The nodes a bind's imported calc evidence assumes, in sorted block-ID
    /// order; empty for every other node and for a bind without loaded
    /// evidence.
    #[must_use]
    pub fn assumptions(&self, node: NodeId) -> Vec<NodeId> {
        self.neighbors_of_kind(node, Direction::Outgoing, Dependency::Assumption)
    }

    /// The binds whose imported calc evidence assumes `node`, in sorted
    /// block-ID order.
    #[must_use]
    pub fn assumed_by(&self, node: NodeId) -> Vec<NodeId> {
        self.neighbors_of_kind(node, Direction::Incoming, Dependency::Assumption)
    }

    fn neighbors_of_kind(
        &self,
        node: NodeId,
        direction: Direction,
        kind: Dependency,
    ) -> Vec<NodeId> {
        let mut neighbors: Vec<NodeId> = self
            .graph
            .edges_directed(node.index(), direction)
            .filter(|edge| *edge.weight() == kind)
            .map(|edge| match direction {
                Direction::Outgoing => edge.target(),
                Direction::Incoming => edge.source(),
            })
            .map(NodeId::from_index)
            .collect();
        neighbors.sort_unstable();
        neighbors
    }

    /// The kind of the edge from `from` to `to`, or `None` when there is
    /// none. At most one edge joins two nodes: a bind has no children, and
    /// only a bind has assumptions.
    fn dependency(&self, from: NodeId, to: NodeId) -> Option<Dependency> {
        self.graph
            .find_edge(from.index(), to.index())
            .map(|edge| self.graph[edge])
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
    /// A component that holds a cycle of `children:` references alone is
    /// represented by one (its smallest): that cycle is in the argument as
    /// written and stands whatever the evidence says, so it is not to be
    /// reported as the doing of an assumption that merely shares its nodes.
    ///
    /// Each cycle lists its nodes in edge order starting from its smallest
    /// node; the closing edge back to the start is implied.
    #[must_use]
    pub fn cycles(&self) -> Vec<Vec<NodeId>> {
        let declared =
            EdgeFiltered::from_fn(&self.graph, |edge| *edge.weight() == Dependency::Child);
        let mut declared_cycles: Vec<Vec<NodeId>> = tarjan_scc(&declared)
            .into_iter()
            .filter_map(|component| self.cycle_within(&component, Some(Dependency::Child)))
            .collect();
        declared_cycles.sort_unstable();

        let mut cycles: Vec<Vec<NodeId>> = tarjan_scc(&self.graph)
            .into_iter()
            .filter_map(|component| {
                // A cycle lies inside one component, so its first node
                // decides which component it belongs to.
                declared_cycles
                    .iter()
                    .find(|cycle| component.contains(&cycle[0].index()))
                    .cloned()
                    .or_else(|| self.cycle_within(&component, None))
            })
            .collect();
        cycles.sort_unstable();
        cycles
    }

    /// Every node reachable from `start` along child and assumption edges,
    /// including `start` itself. A node only a bind's evidence assumes is
    /// part of the argument the bind rests on, so it counts as reached.
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
    /// - a dependency cycle → error naming the full path, one per group of
    ///   mutually dependent nodes, with each assumption edge drawn as `⇢` and
    ///   the error anchored on the `calc:` reference that brought the first
    ///   such edge in; a cycle of `children:` alone is the one named when the
    ///   group has both kinds
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
            let (span, message) = self.describe_cycle(&cycle);
            diagnostics.errors.push(Error::new(span, message));
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

    /// Renders one cycle as `a → b ⇢ a`, with the edge kinds drawn, and
    /// chooses where to anchor it: the `calc:` reference of the first bind
    /// whose assumption closes an edge of the cycle, or — for a cycle of
    /// `children:` references alone — the id of the cycle's smallest node.
    fn describe_cycle(&self, cycle: &[NodeId]) -> (Span, String) {
        let mut path = String::new();
        let mut anchor = None;
        let mut through_assumption = false;
        for (position, &from) in cycle.iter().enumerate() {
            let to = cycle[(position + 1) % cycle.len()];
            let kind = self
                .dependency(from, to)
                .expect("consecutive cycle nodes are joined by an edge");
            if kind == Dependency::Assumption {
                through_assumption = true;
                if anchor.is_none()
                    && let Block::Bind(bind) = self.block(from)
                {
                    anchor = Some(bind.source.span);
                }
            }
            path.push_str(self.id(from));
            path.push(' ');
            path.push_str(kind.arrow());
            path.push(' ');
        }
        path.push_str(self.id(cycle[0]));
        let message = if through_assumption {
            format!("dependency cycle through an assumption: {path}")
        } else {
            format!("dependency cycle: {path}")
        };
        (
            anchor.unwrap_or(self.block(cycle[0]).common().id.span),
            message,
        )
    }

    /// Finds a cycle inside one strongly connected component, or `None` when
    /// the component is a single node without a self-loop.
    ///
    /// `only` narrows the edges walked to one kind; the component must then
    /// be strongly connected by edges of that kind alone.
    fn cycle_within(
        &self,
        component: &[NodeIndex],
        only: Option<Dependency>,
    ) -> Option<Vec<NodeId>> {
        let start = *component.iter().min()?;
        if component.len() == 1 {
            // At most one edge joins two nodes, so this is the self-loop.
            return self
                .graph
                .find_edge(start, start)
                .filter(|&edge| only.is_none_or(|kind| self.graph[edge] == kind))
                .map(|_| vec![NodeId::from_index(start)]);
        }

        let members: HashSet<NodeIndex> = component.iter().copied().collect();
        // Iterative DFS from `start`, restricted to the component. Every
        // component node reaches `start`, and each neighbor of a stacked node
        // is examined while that node is still on the stack, so the search
        // always closes the cycle before the stack empties.
        let mut visited = HashSet::from([start]);
        let mut stack = vec![(start, self.component_neighbors(start, &members, only))];
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
                        let next_neighbors = self.component_neighbors(next, &members, only);
                        stack.push((next, next_neighbors));
                    }
                }
            }
        }
    }

    /// Out-neighbors of `node` inside the component, along edges of kind
    /// `only` when one is given, sorted descending so that popping from the
    /// back visits the smallest node first.
    fn component_neighbors(
        &self,
        node: NodeIndex,
        members: &HashSet<NodeIndex>,
        only: Option<Dependency>,
    ) -> Vec<NodeIndex> {
        let mut neighbors: Vec<NodeIndex> = self
            .graph
            .edges_directed(node, Direction::Outgoing)
            .filter(|edge| only.is_none_or(|kind| *edge.weight() == kind))
            .map(|edge| edge.target())
            .filter(|neighbor| members.contains(neighbor))
            .collect();
        neighbors.sort_unstable_by(|a, b| b.cmp(a));
        neighbors
    }
}

/// Builds the graph for a loaded project and runs every structural check.
///
/// This is the graph half of `inka check`: construction problems (duplicate
/// ids, duplicate or undefined references, an assumption naming no node) and
/// shape problems (a missing objective, cycles — through `children:` or
/// through an assumption — and unreachable nodes) come back as one
/// [`Diagnostics`] either way, together with every `until:` naming a phase
/// the manifest does not declare. A project with no blocks has no objective,
/// so an empty argument is rejected here too.
#[must_use]
pub fn check(project: &Project) -> Diagnostics {
    let Diagnostics {
        mut errors,
        warnings,
    } = check_structure(&project.blocks, &project.calc_evidence);
    errors.extend(deadline::check(&project.blocks, &project.manifest.phases));
    Diagnostics { errors, warnings }
}

/// Checks graph construction and shape in one pass. A partially resolved
/// graph is sufficient for cycle and reachability analysis, so construction
/// errors do not hide independent shape findings.
fn check_structure(
    blocks: &[Block<RenderedDescription>],
    calc_evidence: &CalcEvidenceSet,
) -> Diagnostics {
    let (graph, mut errors) = Graph::build_collecting(blocks, calc_evidence);
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
        let diagnostics = check_structure(&blocks, &CalcEvidenceSet::default());

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

    use inka_evidence::calc::{Definition, DefinitionKind, Leaf, Target};

    /// Calc evidence for one target whose upstream assumes `assumes`.
    fn calc_evidence(target: &str, assumes: &[&str]) -> CalcEvidenceSet {
        CalcEvidenceSet::from_targets([Target {
            path: target.to_owned(),
            definitions: vec![Definition::new(
                target,
                DefinitionKind::Verification,
                "def verify(): ...",
            )],
            assumes: assumes.iter().map(|&id| id.to_owned()).collect(),
            leaves: vec![Leaf::new(target, Some(true))],
            valid: true,
            xfail: false,
        }])
        .expect("one target is a valid set")
    }

    const TARGET: &str = "AOCS::?verify_detumble";

    /// An objective over a calc bind and a fiat the bind's evidence may assume.
    const BIND_AND_FIAT: &str = "\
objective root {
  ```
  root
  ```
  children: modelled, assumed
}
bind modelled {
  ```
  the controller detumbles
  ```
  calc: \"AOCS::?verify_detumble\"
}
fiat assumed {
  ```
  the disturbance model envelopes reality
  ```
}
";

    #[test]
    fn resolves_the_assumptions_of_loaded_calc_evidence_to_edges() {
        // Arrange
        let blocks = blocks(BIND_AND_FIAT);
        let evidence = calc_evidence(TARGET, &["assumed"]);

        // Act
        let graph = Graph::build_with(&blocks, &evidence).expect("graph should build");

        // Assert: the bind depends on the assumed node, the assumed node is
        // not thereby a child of the bind, and the order respects the edge.
        let modelled = graph.node("modelled").expect("modelled should exist");
        let assumed = graph.node("assumed").expect("assumed should exist");
        assert_eq!(ids(&graph, &graph.assumptions(modelled)), ["assumed"]);
        assert_eq!(ids(&graph, &graph.assumed_by(assumed)), ["modelled"]);
        assert_eq!(ids(&graph, &graph.parents(assumed)), ["root"]);
        assert_eq!(graph.children(modelled), []);
        let order = graph
            .topological_order()
            .expect("an assumption edge keeps the graph a DAG");
        let position = |node| order.iter().position(|&entry| entry == node);
        assert!(position(modelled) < position(assumed));
        assert!(graph.validate().is_empty());
    }

    #[test]
    fn a_calc_reference_no_loaded_target_answers_adds_no_edge_and_no_error() {
        // Arrange: evidence for a different verification.
        let blocks = blocks(BIND_AND_FIAT);
        let evidence = calc_evidence("AOCS::?verify_other", &["assumed"]);

        // Act
        let graph = Graph::build_with(&blocks, &evidence).expect("missing evidence is eval's");

        // Assert
        let modelled = graph.node("modelled").expect("modelled should exist");
        assert_eq!(graph.assumptions(modelled), []);
        assert_eq!(
            Graph::build(&blocks)
                .expect("builds without evidence")
                .len(),
            graph.len()
        );
    }

    #[test]
    fn reports_an_undefined_assumption_at_the_calc_reference() {
        // Arrange
        let blocks = blocks(BIND_AND_FIAT);
        let evidence = calc_evidence(TARGET, &["assumed", "ghost"]);

        // Act
        let errors = Graph::build_with(&blocks, &evidence).expect_err("ghost is not a block");

        // Assert: anchored on the `calc:` property that brought the edge in.
        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors[0].message,
            "undefined assumption `ghost`: the calc evidence for `AOCS::?verify_detumble` assumes it, but no block defines it"
        );
        let Block::Bind(bind) = &blocks[1] else {
            panic!("the second block is the bind");
        };
        assert_eq!(errors[0].span, Some(bind.source.span));
    }

    #[test]
    fn a_bind_assuming_itself_is_a_cycle_through_an_assumption() {
        // Arrange
        let blocks = blocks(BIND_AND_FIAT);
        let evidence = calc_evidence(TARGET, &["modelled"]);
        let graph = Graph::build_with(&blocks, &evidence).expect("a self-loop still builds");

        // Act
        let diagnostics = graph.validate();

        // Assert
        assert_eq!(graph.topological_order(), None);
        assert_eq!(diagnostics.errors.len(), 1);
        assert_eq!(
            diagnostics.errors[0].message,
            "dependency cycle through an assumption: modelled ⇢ modelled"
        );
        let Block::Bind(bind) = &blocks[1] else {
            panic!("the second block is the bind");
        };
        assert_eq!(diagnostics.errors[0].span, Some(bind.source.span));
    }

    #[test]
    fn a_bind_assuming_an_ancestor_is_a_cycle_through_an_assumption() {
        // Arrange: root → branch → modelled, and modelled's evidence assumes root.
        let blocks = blocks(
            "\
objective root {
  ```
  root
  ```
  children: branch
}
structure branch {
  ```
  branch
  ```
  children: modelled
}
bind modelled {
  ```
  modelled
  ```
  calc: \"AOCS::?verify_detumble\"
}
",
        );
        let evidence = calc_evidence(TARGET, &["root"]);

        // Act
        let diagnostics = check_structure(&blocks, &evidence);

        // Assert: child edges draw as `→`, the assumption edge as `⇢`.
        let messages: Vec<&str> = diagnostics
            .errors
            .iter()
            .map(|error| error.message.as_str())
            .collect();
        assert_eq!(
            messages,
            ["dependency cycle through an assumption: branch → modelled ⇢ root → branch"]
        );
    }

    #[test]
    fn a_cycle_of_children_alone_is_named_even_when_an_assumption_closes_another() {
        // Arrange: `x ↔ y` is a cycle of the argument as written. The
        // evidence adds a second way round (`m ⇢ a`) through the same nodes,
        // and a search from the smallest node meets that one first.
        let blocks = blocks(
            "\
objective a {
  ```
  a
  ```
  children: x
}
structure x {
  ```
  x
  ```
  children: y
}
structure y {
  ```
  y
  ```
  children: x, m
}
bind m {
  ```
  m
  ```
  calc: \"AOCS::?verify_detumble\"
}
",
        );
        let evidence = calc_evidence(TARGET, &["a"]);

        // Act
        let diagnostics = check_structure(&blocks, &evidence);

        // Assert: one error for the component, and it is the cycle no
        // evidence can take away, anchored in the argument and not on `calc:`.
        let messages: Vec<&str> = diagnostics
            .errors
            .iter()
            .map(|error| error.message.as_str())
            .collect();
        assert_eq!(messages, ["dependency cycle: x → y → x"]);
        assert_eq!(diagnostics.errors[0].span, Some(blocks[1].common().id.span));
    }

    #[test]
    fn a_self_referencing_child_is_named_before_a_cycle_through_an_assumption() {
        // Arrange: `s` lists itself, and sits on a cycle the evidence closes.
        let blocks = blocks(
            "\
objective a {
  ```
  a
  ```
  children: s
}
structure s {
  ```
  s
  ```
  children: s, m
}
bind m {
  ```
  m
  ```
  calc: \"AOCS::?verify_detumble\"
}
",
        );
        let evidence = calc_evidence(TARGET, &["a"]);

        // Act
        let diagnostics = check_structure(&blocks, &evidence);

        // Assert
        let messages: Vec<&str> = diagnostics
            .errors
            .iter()
            .map(|error| error.message.as_str())
            .collect();
        assert_eq!(messages, ["dependency cycle: s → s"]);
    }

    #[test]
    fn a_node_only_an_assumption_reaches_is_not_an_orphan() {
        // Arrange: `assumed` is no block's child; only the bind's evidence
        // depends on it.
        let blocks = blocks(
            "\
objective root {
  ```
  root
  ```
  children: modelled
}
bind modelled {
  ```
  modelled
  ```
  calc: \"AOCS::?verify_detumble\"
}
fiat assumed {
  ```
  assumed
  ```
}
",
        );

        // Act
        let without = check_structure(&blocks, &CalcEvidenceSet::default());
        let with = check_structure(&blocks, &calc_evidence(TARGET, &["assumed"]));

        // Assert: the warning exists exactly while nothing depends on the node.
        assert_eq!(without.warnings.len(), 1);
        assert!(with.is_empty(), "got {with:?}");
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
