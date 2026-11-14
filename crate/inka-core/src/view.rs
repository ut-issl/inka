//! The read-only projection every output renders.
//!
//! `inka export`, the Markdown report, and the server all describe the same
//! argument, so none of them reads the [`Graph`] or the AST directly: they
//! read one [`ArgumentView`] built here, and each command is then a thin
//! formatter over it. Two outputs disagreeing about a node's text or its
//! position becomes impossible rather than merely unlikely.
//!
//! The view is a projection, not a second model. It carries no judgement, no
//! approval or evaluation state, and no command-specific ordering: those
//! belong to the passes and formatters on either side of it.
//!
//! Ordering is fixed here so no consumer has to re-derive it. Nodes follow the
//! graph's sorted block-ID order, each node's children follow the order its
//! block declares, and edges are enumerated parent by parent in node order.
//! The same project therefore always projects to the same view, whatever the
//! file layout.
//!
//! Descriptions are post-interpolation by construction: the view is built from
//! [`RenderedDescription`] blocks, so no `{$var}` can reach a consumer. They
//! also arrive in the canonical form [`normalize::description`] defines, once,
//! rather than each formatter dedenting for itself — and that is the same form
//! hashing reads, so what a reviewer sees is what their approval is bound to.
//! Comments travel the same way: every comment attached to a block, a
//! property, or a child entry is on the node, as the text hashing reads.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::ast::{
    Block, Comment, Deadline as AstDeadline, FiatPromoteTarget, ObjectivePromoteTarget, SourceRef,
};
use crate::graph::{Graph, NodeId};
use crate::interpolate::RenderedDescription;
use crate::normalize;
use crate::source::SourceMap;

/// The four ways a proposition is discharged.
///
/// Doubles as the promotion target of a deadline: `promote:` names the mode a
/// node is to become, and the modes it may name are a subset of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Objective,
    Structure,
    Bind,
    Fiat,
}

impl Mode {
    /// The mode's one spelling everywhere it is written down: the DSL
    /// keyword, the `<kind>` of a record target, and the JSON serialization.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Mode::Objective => "objective",
            Mode::Structure => "structure",
            Mode::Bind => "bind",
            Mode::Fiat => "fiat",
        }
    }
}

/// The scheme of a bind block's evidence reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EvidenceKind {
    Calc,
    Test,
}

/// A bind block's machine-verifiable evidence target.
///
/// The target is carried verbatim and stays opaque: matching a reference to an
/// actual result is evaluation's job, not the view's.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Evidence {
    pub kind: EvidenceKind,
    pub target: String,
}

/// An `until:` annotation with its optional promotion target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Deadline {
    /// Phase after which approvals for this node no longer count.
    pub until: String,
    /// Mode the node announces it will be promoted to.
    pub promote: Option<Mode>,
}

/// A block property a comment can be attached to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Property {
    Children,
    Test,
    Calc,
    Until,
    Promote,
}

impl Property {
    /// The property's name as the DSL spells it.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Property::Children => "children",
            Property::Test => "test",
            Property::Calc => "calc",
            Property::Until => "until",
            Property::Promote => "promote",
        }
    }
}

/// Every comment attached to one node, by anchor.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct NodeComments {
    /// The comment on the block itself.
    pub block: Option<String>,
    /// Comments on the block's properties.
    pub properties: BTreeMap<Property, String>,
    /// Comments on the block's child entries, keyed by child id.
    pub children: BTreeMap<String, String>,
}

/// One block, projected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ViewNode {
    pub id: String,
    pub mode: Mode,
    /// Namespace the node belongs to.
    ///
    /// Until namespace resolution lands this is the stem of the argument file
    /// the block was parsed from, which is what the Web UI invariants specify.
    pub namespace: String,
    /// Contract text with every `{$var}` already replaced.
    pub description: String,
    /// Tags without their leading `#`, in declaration order.
    pub tags: Vec<String>,
    /// Present on bind blocks only, which are the blocks that carry evidence.
    pub evidence: Option<Evidence>,
    pub deadline: Option<Deadline>,
    /// Children in the order this block declares them.
    pub children: Vec<String>,
    pub comments: NodeComments,
}

/// A parent-to-child edge, named by block id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ViewEdge {
    pub parent: String,
    pub child: String,
}

/// One argument, as every output sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ArgumentView {
    /// Every node, in sorted block-ID order.
    pub nodes: Vec<ViewNode>,
    /// Every edge, enumerated parent by parent in node order and, within a
    /// parent, in the order the block declares its children.
    pub edges: Vec<ViewEdge>,
    /// The objectives the argument hangs from: objective blocks nothing
    /// references, in sorted block-ID order.
    ///
    /// An objective some other block names is not a root — it is a node inside
    /// a larger argument — so this is the set a reader starts from.
    pub roots: Vec<String>,
}

impl ArgumentView {
    /// Projects a built graph.
    ///
    /// `source_map` resolves each block's file, which is where a node's
    /// namespace comes from until namespace resolution lands.
    #[must_use]
    pub fn build(graph: &Graph<'_>, source_map: &SourceMap) -> Self {
        let nodes: Vec<ViewNode> = graph
            .nodes()
            .map(|node| project_node(graph, source_map, node))
            .collect();

        let edges = nodes
            .iter()
            .flat_map(|node| {
                node.children.iter().map(|child| ViewEdge {
                    parent: node.id.clone(),
                    child: child.clone(),
                })
            })
            .collect();

        let roots = graph
            .objectives()
            .into_iter()
            .filter(|&objective| graph.parents(objective).is_empty())
            .map(|objective| graph.id(objective).to_owned())
            .collect();

        Self {
            nodes,
            edges,
            roots,
        }
    }

    /// The node with this id, or `None` when the argument has no such block.
    #[must_use]
    pub fn node(&self, id: &str) -> Option<&ViewNode> {
        self.nodes.iter().find(|node| node.id == id)
    }
}

fn project_node(graph: &Graph<'_>, source_map: &SourceMap, node: NodeId) -> ViewNode {
    let block = graph.block(node);
    let common = block.common();
    ViewNode {
        id: common.id.value.clone(),
        mode: mode_of(block),
        namespace: namespace_of(source_map, block),
        description: normalize::description(common.description.rendered()),
        tags: common.tags.iter().map(|tag| tag.value.clone()).collect(),
        evidence: evidence_of(block),
        deadline: deadline_of(block),
        children: block
            .children()
            .iter()
            .map(|child| child.id.value.clone())
            .collect(),
        comments: comments_of(block),
    }
}

/// Collects the text of every comment attached to the block, by anchor.
fn comments_of(block: &Block<RenderedDescription>) -> NodeComments {
    let mut properties = BTreeMap::new();
    let mut put = |property, comment: Option<&Comment>| {
        if let Some(comment) = comment {
            properties.insert(property, comment.text());
        }
    };
    if let Some(deadline) = block.deadline() {
        put(Property::Until, deadline.comment.as_ref());
    }
    match block {
        Block::Objective(objective) => {
            put(Property::Children, objective.children_comment.as_ref());
            let promote = objective.deadline.as_ref().and_then(|d| d.promote.as_ref());
            put(Property::Promote, promote.and_then(|p| p.comment.as_ref()));
        }
        Block::Structure(structure) => {
            put(Property::Children, structure.children_comment.as_ref());
        }
        Block::Bind(bind) => {
            let property = match bind.source.value {
                SourceRef::Test(_) => Property::Test,
                SourceRef::Calc(_) => Property::Calc,
            };
            put(property, bind.source_comment.as_ref());
        }
        Block::Fiat(fiat) => {
            let promote = fiat.deadline.as_ref().and_then(|d| d.promote.as_ref());
            put(Property::Promote, promote.and_then(|p| p.comment.as_ref()));
        }
    }
    NodeComments {
        block: block.common().comment.as_ref().map(Comment::text),
        properties,
        children: block
            .children()
            .iter()
            .filter_map(|child| {
                let comment = child.comment.as_ref()?;
                Some((child.id.value.clone(), comment.text()))
            })
            .collect(),
    }
}

fn mode_of(block: &Block<RenderedDescription>) -> Mode {
    match block {
        Block::Objective(_) => Mode::Objective,
        Block::Structure(_) => Mode::Structure,
        Block::Bind(_) => Mode::Bind,
        Block::Fiat(_) => Mode::Fiat,
    }
}

/// The stem of the argument file the block was parsed from.
///
/// A block always comes from a file the map registered, so the fallbacks below
/// are unreachable in practice; they keep an odd path (no stem, or one that is
/// not valid UTF-8) from costing the whole projection.
fn namespace_of(source_map: &SourceMap, block: &Block<RenderedDescription>) -> String {
    source_map
        .file(block.common().span.file_id)
        .and_then(|file| file.path.file_stem())
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn evidence_of(block: &Block<RenderedDescription>) -> Option<Evidence> {
    let Block::Bind(bind) = block else {
        return None;
    };
    Some(match &bind.source.value {
        SourceRef::Calc(target) => Evidence {
            kind: EvidenceKind::Calc,
            target: target.clone(),
        },
        SourceRef::Test(target) => Evidence {
            kind: EvidenceKind::Test,
            target: target.clone(),
        },
    })
}

/// Flattens the per-mode deadline types into the one shape a consumer reads.
///
/// The AST keeps them apart so an illegal promotion target cannot be built; by
/// this point the parser has already enforced that, and what a renderer needs
/// is the phase and the announced mode. Their comments travel separately, in
/// [`NodeComments`].
fn deadline_of(block: &Block<RenderedDescription>) -> Option<Deadline> {
    match block {
        Block::Objective(objective) => objective.deadline.as_ref().map(|annotation| {
            deadline(
                &annotation.deadline,
                annotation
                    .promote
                    .as_ref()
                    .map(|promote| match promote.target.value {
                        ObjectivePromoteTarget::Structure => Mode::Structure,
                    }),
            )
        }),
        Block::Structure(structure) => structure
            .deadline
            .as_ref()
            .map(|annotation| deadline(annotation, None)),
        Block::Bind(bind) => bind
            .deadline
            .as_ref()
            .map(|annotation| deadline(annotation, None)),
        Block::Fiat(fiat) => fiat.deadline.as_ref().map(|annotation| {
            deadline(
                &annotation.deadline,
                annotation
                    .promote
                    .as_ref()
                    .map(|promote| match promote.target.value {
                        FiatPromoteTarget::Structure => Mode::Structure,
                        FiatPromoteTarget::Bind => Mode::Bind,
                    }),
            )
        }),
    }
}

fn deadline(annotation: &AstDeadline, promote: Option<Mode>) -> Deadline {
    Deadline {
        until: annotation.until.value.clone(),
        promote,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;

    use crate::interpolate::interpolate;
    use crate::lexer::Lexer;
    use crate::parser::parse;

    /// Projects one synthetic argument file, running the same pipeline the
    /// project loader does. `values` resolves any `{$var}` in it.
    fn view_of(file: &str, source: &str, values: &HashMap<String, String>) -> ArgumentView {
        let mut source_map = SourceMap::new();
        let file_id = source_map.add_file(file.into(), source.to_owned());
        let tokens = Lexer::new(file_id, source)
            .lex()
            .expect("test source should lex");
        let parsed = parse(file_id, &tokens);
        assert!(
            parsed.errors.is_empty(),
            "test source should parse cleanly: {:?}",
            parsed.errors
        );
        let blocks: Vec<Block<RenderedDescription>> = parsed
            .blocks
            .into_iter()
            .map(|block| {
                let rendered = interpolate(&block.common().description, values)
                    .expect("test source should interpolate");
                block.with_description(rendered)
            })
            .collect();
        let graph = Graph::build(&blocks).expect("test source should build a graph");
        ArgumentView::build(&graph, &source_map)
    }

    fn view(source: &str) -> ArgumentView {
        view_of("safety.inka", source, &HashMap::new())
    }

    const TREE: &str = "\
objective z_root #top #reviewed {
  ```
  the root claim
  ```
  children: b_branch, a_leaf
}
structure b_branch {
  ```
  a branch
  ```
  children: a_leaf
}
fiat a_leaf {
  ```
  taken as given
  ```
  // measured once the vehicle exists
  until: FM
  promote: bind
}
";

    #[test]
    fn projects_nodes_in_sorted_block_id_order() {
        // Arrange & Act: declaration order differs from sorted order.
        let view = view(TREE);

        // Assert
        let ids: Vec<&str> = view.nodes.iter().map(|node| node.id.as_str()).collect();
        assert_eq!(ids, ["a_leaf", "b_branch", "z_root"]);
    }

    #[test]
    fn enumerates_edges_parent_by_parent_in_declaration_order() {
        let view = view(TREE);

        let edges: Vec<(&str, &str)> = view
            .edges
            .iter()
            .map(|edge| (edge.parent.as_str(), edge.child.as_str()))
            .collect();
        assert_eq!(
            edges,
            [
                ("b_branch", "a_leaf"),
                // z_root declares b_branch before a_leaf, and keeps that order.
                ("z_root", "b_branch"),
                ("z_root", "a_leaf"),
            ]
        );
    }

    #[test]
    fn projects_each_block_once_however_many_parents_it_has() {
        // Arrange & Act: a_leaf is a child of both z_root and b_branch.
        let view = view(TREE);

        // Assert
        assert_eq!(
            view.nodes.iter().filter(|node| node.id == "a_leaf").count(),
            1
        );
        assert_eq!(view.nodes.len(), 3);
    }

    #[test]
    fn carries_the_mode_of_every_block() {
        let view = view(TREE);

        assert_eq!(
            view.node("z_root").map(|node| node.mode),
            Some(Mode::Objective)
        );
        assert_eq!(
            view.node("b_branch").map(|node| node.mode),
            Some(Mode::Structure)
        );
        assert_eq!(view.node("a_leaf").map(|node| node.mode), Some(Mode::Fiat));
    }

    #[test]
    fn names_the_argument_file_stem_as_the_namespace() {
        let view = view_of("argument/pointing.inka", TREE, &HashMap::new());

        assert!(view.nodes.iter().all(|node| node.namespace == "pointing"));
    }

    #[test]
    fn carries_tags_without_their_leading_hash() {
        let view = view(TREE);

        assert_eq!(
            view.node("z_root").map(|node| node.tags.as_slice()),
            Some(["top".to_owned(), "reviewed".to_owned()].as_slice())
        );
        assert!(view.node("a_leaf").is_some_and(|node| node.tags.is_empty()));
    }

    #[test]
    fn projects_descriptions_after_interpolation() {
        // Arrange
        let values = HashMap::from([("limit".to_owned(), "2 orbits".to_owned())]);
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
        let view = view_of("objective.inka", source, &values);

        // Assert: no `{$var}` reaches a consumer.
        assert_eq!(
            view.node("root").map(|node| node.description.as_str()),
            Some("detumbling completes within 2 orbits")
        );
    }

    #[test]
    fn carries_the_evidence_target_of_a_bind_verbatim() {
        let view = view(
            "\
objective root {
  ```
  the claim
  ```
  children: verified, asserted
}
bind verified {
  ```
  checked by a solver
  ```
  calc: \"AOCS::?verify_detumble\"
}
fiat asserted {
  ```
  taken as given
  ```
}
",
        );

        assert_eq!(
            view.node("verified").and_then(|node| node.evidence.clone()),
            Some(Evidence {
                kind: EvidenceKind::Calc,
                target: "AOCS::?verify_detumble".to_owned(),
            })
        );
        // Only a bind block discharges against evidence.
        assert_eq!(
            view.node("asserted").and_then(|node| node.evidence.clone()),
            None
        );
    }

    #[test]
    fn distinguishes_the_two_evidence_schemes() {
        let view = view(
            "\
objective root {
  ```
  the claim
  ```
  children: tested
}
bind tested {
  ```
  checked in CI
  ```
  test: \"tests/hash_props.rs\"
}
",
        );

        assert_eq!(
            view.node("tested").and_then(|node| node.evidence.clone()),
            Some(Evidence {
                kind: EvidenceKind::Test,
                target: "tests/hash_props.rs".to_owned(),
            })
        );
    }

    #[test]
    fn flattens_a_deadline_into_phase_and_promotion() {
        let view = view(TREE);

        assert_eq!(
            view.node("a_leaf").and_then(|node| node.deadline.clone()),
            Some(Deadline {
                until: "FM".to_owned(),
                promote: Some(Mode::Bind),
            })
        );
        assert_eq!(
            view.node("z_root").and_then(|node| node.deadline.clone()),
            None
        );
    }

    #[test]
    fn carries_every_attached_comment_by_anchor() {
        let view = view(
            "\
// on the root
objective root {
  ```
  the root claim
  ```
  children:
    // why a
    a,
    b
  // provisional wording
  until: M3
  // once the branches settle
  promote: structure
}
bind a {
  ```
  checked in CI
  ```
  calc: \"M::?f\" // the shipped check
}
fiat b {
  ```
  taken as given
  ```
}
",
        );

        let root = view.node("root").expect("root is projected");
        assert_eq!(root.comments.block.as_deref(), Some("on the root"));
        assert_eq!(
            root.comments.properties,
            BTreeMap::from([
                (Property::Until, "provisional wording".to_owned()),
                (Property::Promote, "once the branches settle".to_owned()),
            ])
        );
        assert_eq!(
            root.comments.children,
            BTreeMap::from([("a".to_owned(), "why a".to_owned())])
        );
        let bind = view.node("a").expect("a is projected");
        assert_eq!(
            bind.comments.properties,
            BTreeMap::from([(Property::Calc, "the shipped check".to_owned())])
        );
        assert_eq!(
            view.node("b").map(|node| &node.comments),
            Some(&NodeComments::default())
        );
    }

    #[test]
    fn reads_an_objectives_only_promotion_target() {
        let view = view(
            "\
objective root {
  ```
  provisional wording
  ```
  until: M3
  promote: structure
  children: leaf
}
fiat leaf {
  ```
  taken as given
  ```
}
",
        );

        assert_eq!(
            view.node("root").and_then(|node| node.deadline.clone()),
            Some(Deadline {
                until: "M3".to_owned(),
                promote: Some(Mode::Structure),
            })
        );
    }

    #[test]
    fn names_only_the_objectives_nothing_references_as_roots() {
        // Arrange & Act: `sub` is an objective, but the root objective claims it.
        let view = view(
            "\
objective root {
  ```
  the root claim
  ```
  children: sub
}
objective sub {
  ```
  a claim inside the argument
  ```
  children: leaf
}
fiat leaf {
  ```
  taken as given
  ```
}
",
        );

        // Assert
        assert_eq!(view.roots, ["root"]);
    }

    #[test]
    fn strips_the_fence_indent_so_a_multi_line_contract_reads_as_written() {
        // Arrange: a description indented inside its block, with a list whose
        // relative indent is contract content.
        let source = "\
objective root {
  ```
  In safe mode the satellite holds:

  - the sun angle within 30 deg
    - measured from the array normal
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
        let view = view_of("safety.inka", source, &HashMap::new());

        // Assert: the common indent is gone, the relative one is kept.
        assert_eq!(
            view.node("root").map(|node| node.description.as_str()),
            Some(
                "In safe mode the satellite holds:\n\n- the sun angle within 30 deg\n  - measured from the array normal"
            )
        );
    }

    #[test]
    fn has_no_node_for_an_id_the_argument_does_not_define() {
        assert!(view(TREE).node("absent").is_none());
    }
}
