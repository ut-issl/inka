//! What a reviewer is shown of one node: the content of its local view, in
//! the order it is presented.
//!
//! An approval binds to a node's local view — its own description, its
//! deadline annotations, the comments attached to it and to its child
//! entries, and its direct children's descriptions; nothing deeper.
//! [`LocalView`](crate::hash::LocalView) is that content arranged for
//! hashing. [`LocalContent`] is the same content arranged for reading, plus
//! the two things a reader needs that the hash leaves out on purpose: the ids
//! and the evidence reference.
//!
//! Every surface that shows a target — `inka review` in a terminal, the Web
//! UI — renders this one structure, so they cannot disagree about what a
//! target shows or in what order: the comment on the block, the description,
//! the evidence reference, the children, the deadline, each property under
//! the comment attached to it.

use serde::Serialize;

use crate::view::{ArgumentView, EvidenceKind, Mode, Property, ViewNode};

/// One node's local view, as content to present.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct LocalContent {
    pub id: String,
    pub mode: Mode,
    /// The comment on the block itself.
    pub comment: Option<String>,
    /// The description the decision judges.
    pub description: String,
    /// Present on bind nodes: the evidence the claim is bound to.
    pub evidence: Option<EvidenceContent>,
    /// Present when the node has children.
    pub children: Option<ChildrenContent>,
    pub deadline: Option<DeadlineContent>,
}

/// A bind's evidence reference with the comment on its property.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct EvidenceContent {
    pub kind: EvidenceKind,
    /// The opaque evidence target, verbatim.
    pub target: String,
    pub comment: Option<String>,
}

/// The `children:` property: its comment, then each direct child.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ChildrenContent {
    pub comment: Option<String>,
    /// The direct children, in the order the block declares them.
    pub entries: Vec<ChildContent>,
}

/// One direct child as its parent's local view shows it: the comment on the
/// entry and the child's description, nothing below.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ChildContent {
    pub id: String,
    /// The comment on the parent's entry for this child.
    pub comment: Option<String>,
    /// The child's description; `None` when the view does not define the
    /// child, which a checked project never produces.
    pub description: Option<String>,
}

/// The deadline annotations, each with the comment on its property.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DeadlineContent {
    pub until: String,
    pub until_comment: Option<String>,
    pub promote: Option<PromoteContent>,
}

/// A `promote:` annotation with the comment on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct PromoteContent {
    pub mode: Mode,
    pub comment: Option<String>,
}

impl EvidenceContent {
    /// The property the reference is written under.
    #[must_use]
    pub const fn property(&self) -> Property {
        match self.kind {
            EvidenceKind::Test => Property::Test,
            EvidenceKind::Veriq => Property::Veriq,
        }
    }
}

impl LocalContent {
    /// The local view of `node`, read from `view`.
    #[must_use]
    pub fn of(view: &ArgumentView, node: &ViewNode) -> Self {
        let comments = &node.comments;
        let on = |property: Property| comments.properties.get(&property).cloned();

        let evidence = node.evidence.as_ref().map(|reference| {
            let mut content = EvidenceContent {
                kind: reference.kind,
                target: reference.target.clone(),
                comment: None,
            };
            content.comment = on(content.property());
            content
        });
        let children = (!node.children.is_empty()).then(|| ChildrenContent {
            comment: on(Property::Children),
            entries: node
                .children
                .iter()
                .map(|child| ChildContent {
                    id: child.clone(),
                    comment: comments.children.get(child).cloned(),
                    description: view.node(child).map(|node| node.description.clone()),
                })
                .collect(),
        });
        let deadline = node.deadline.as_ref().map(|deadline| DeadlineContent {
            until: deadline.until.clone(),
            until_comment: on(Property::Until),
            promote: deadline.promote.map(|mode| PromoteContent {
                mode,
                comment: on(Property::Promote),
            }),
        });

        Self {
            id: node.id.clone(),
            mode: node.mode,
            comment: comments.block.clone(),
            description: node.description.clone(),
            evidence,
            children,
            deadline,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::BTreeMap;

    use crate::view::{Deadline, Evidence, NodeComments, ViewEdge};

    fn node(id: &str, mode: Mode, children: &[&str]) -> ViewNode {
        ViewNode {
            id: id.to_owned(),
            mode,
            namespace: "test".to_owned(),
            description: format!("{id} holds"),
            tags: Vec::new(),
            evidence: None,
            deadline: None,
            children: children.iter().map(|&child| child.to_owned()).collect(),
            comments: NodeComments::default(),
        }
    }

    fn view(nodes: Vec<ViewNode>) -> ArgumentView {
        let edges = nodes
            .iter()
            .flat_map(|parent| {
                parent.children.iter().map(|child| ViewEdge {
                    parent: parent.id.clone(),
                    child: child.clone(),
                })
            })
            .collect();
        ArgumentView {
            nodes,
            edges,
            roots: Vec::new(),
        }
    }

    #[test]
    fn a_structure_shows_its_children_in_declaration_order_with_their_entry_comments() {
        // Arrange
        let mut parent = node("parent", Mode::Structure, &["zeta", "alpha"]);
        parent.comments = NodeComments {
            block: Some("on the block".to_owned()),
            properties: BTreeMap::from([(Property::Children, "on the list".to_owned())]),
            children: BTreeMap::from([("alpha".to_owned(), "on the entry".to_owned())]),
        };
        let view = view(vec![
            node("alpha", Mode::Fiat, &[]),
            parent,
            node("zeta", Mode::Structure, &["deep"]),
            node("deep", Mode::Fiat, &[]),
        ]);

        // Act
        let content = LocalContent::of(&view, view.node("parent").expect("the parent"));

        // Assert
        assert_eq!(content.comment.as_deref(), Some("on the block"));
        assert_eq!(content.description, "parent holds");
        let children = content.children.expect("a structure has children");
        assert_eq!(children.comment.as_deref(), Some("on the list"));
        assert_eq!(
            children.entries,
            [
                ChildContent {
                    id: "zeta".to_owned(),
                    comment: None,
                    description: Some("zeta holds".to_owned()),
                },
                ChildContent {
                    id: "alpha".to_owned(),
                    comment: Some("on the entry".to_owned()),
                    description: Some("alpha holds".to_owned()),
                },
            ]
        );
    }

    #[test]
    fn a_leaf_has_no_children_section() {
        // Arrange
        let view = view(vec![node("leaf", Mode::Fiat, &[])]);

        // Act
        let content = LocalContent::of(&view, &view.nodes[0]);

        // Assert
        assert_eq!(content.children, None);
        assert_eq!(content.evidence, None);
        assert_eq!(content.deadline, None);
    }

    #[test]
    fn a_bind_shows_its_reference_under_the_comment_on_its_own_property() {
        // Arrange
        let mut bind = node("bound", Mode::Bind, &[]);
        bind.evidence = Some(Evidence {
            kind: EvidenceKind::Veriq,
            target: "aocs.verify".to_owned(),
        });
        bind.comments.properties = BTreeMap::from([
            (Property::Veriq, "why this evidence".to_owned()),
            (Property::Test, "not this one".to_owned()),
        ]);
        let view = view(vec![bind]);

        // Act
        let content = LocalContent::of(&view, &view.nodes[0]);

        // Assert
        assert_eq!(
            content.evidence,
            Some(EvidenceContent {
                kind: EvidenceKind::Veriq,
                target: "aocs.verify".to_owned(),
                comment: Some("why this evidence".to_owned()),
            })
        );
    }

    #[test]
    fn a_deadline_carries_the_comment_on_each_annotation() {
        // Arrange
        let mut fiat = node("debt", Mode::Fiat, &[]);
        fiat.deadline = Some(Deadline {
            until: "M3".to_owned(),
            promote: Some(Mode::Bind),
        });
        fiat.comments.properties = BTreeMap::from([
            (Property::Until, "awaiting the test".to_owned()),
            (Property::Promote, "to the scenario test".to_owned()),
        ]);
        let view = view(vec![fiat]);

        // Act
        let content = LocalContent::of(&view, &view.nodes[0]);

        // Assert
        assert_eq!(
            content.deadline,
            Some(DeadlineContent {
                until: "M3".to_owned(),
                until_comment: Some("awaiting the test".to_owned()),
                promote: Some(PromoteContent {
                    mode: Mode::Bind,
                    comment: Some("to the scenario test".to_owned()),
                }),
            })
        );
    }

    #[test]
    fn a_child_the_view_does_not_define_has_no_description() {
        // Arrange
        let view = view(vec![node("parent", Mode::Structure, &["absent"])]);

        // Act
        let content = LocalContent::of(&view, &view.nodes[0]);

        // Assert
        let children = content.children.expect("the entry is still listed");
        assert_eq!(children.entries[0].description, None);
    }
}
