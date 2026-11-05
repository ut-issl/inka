//! The review queue: which approval targets need a decision right now.
//!
//! `inka review` walks every approval target — objective, structure, fiat and
//! bind nodes alike — and asks the same question evaluation asks: does a valid
//! approval cover the current content? The judgement is [`approval_status`],
//! the validity function shared with `inka eval`, so the queue and the
//! evaluation can never disagree about which approvals are live.
//!
//! A target enters the queue when it is missing an approval, when its
//! approvals cover only earlier content, or when a decline vetoes the current
//! content — the last shown for information, since the way out of a veto is an
//! edit or a retraction, not another approval. Satisfied targets stay out.
//!
//! Some unestablished targets cannot be acted on from the queue at all: a bind
//! whose evidence has not been delivered has no interface hash and therefore
//! no current view hash to approve, a veriq bind is not evaluated yet, and a
//! node whose `until:` has expired would not be helped by a new approval.
//! These are reported separately as blocked, never silently dropped.

use crate::eval::{ApprovalStatus, EvalError, PhaseOrder, approval_status};
use crate::evidence::EvidenceSet;
use crate::hash::{InterfaceHash, LocalView, LocalViewError, ViewHash};
use crate::manifest::PhasesSection;
use crate::record::Record;
use crate::view::{ArgumentView, EvidenceKind, Mode, ViewNode};

/// Why a target needs a decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueueState {
    /// No record targets this node at all.
    Missing,
    /// Approvals exist, but only for earlier content.
    Stale {
        /// Who approved an earlier version, sorted and deduplicated.
        approvers: Vec<String>,
    },
    /// A decline vetoes the current content. Informational: approving over a
    /// veto changes nothing, the way out is an edit or a retraction.
    Declined {
        /// Who declined the current content, sorted and deduplicated.
        by: Vec<String>,
    },
}

/// One target needing a decision, with the hash a decision would bind to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueItem {
    /// The node's id.
    pub id: String,
    /// The node's mode.
    pub mode: Mode,
    /// The view hash a decision recorded now would name.
    pub current_hash: ViewHash,
    /// Why the target is queued.
    pub state: QueueState,
}

/// Why review cannot act on an unestablished target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockedReason {
    /// No imported evidence provides the bind's `test:` reference, so there
    /// is no interface hash and no current view hash to decide on.
    MissingEvidence {
        /// The `test:` reference no loaded evidence provides.
        target: String,
    },
    /// veriq evidence is not evaluated yet.
    VeriqNotEvaluated,
    /// The node's `until:` has expired; no new approval would count.
    UntilExpired {
        /// The phase the node was approved toward.
        until: String,
    },
}

/// One unestablished target the queue cannot offer a decision on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockedItem {
    /// The node's id.
    pub id: String,
    /// The node's mode.
    pub mode: Mode,
    /// Why no decision can be recorded.
    pub reason: BlockedReason,
}

/// Everything review derives: actionable items and blocked information, both
/// in the view's sorted block-ID order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReviewQueue {
    /// Targets a decision can be recorded on (`Declined` included, as
    /// information).
    pub items: Vec<QueueItem>,
    /// Unestablished targets no decision can reach.
    pub blocked: Vec<BlockedItem>,
}

/// Derives the review queue from the same inputs evaluation reads.
///
/// # Errors
///
/// Returns an [`EvalError`] when the current phase or a node's `until:` phase
/// is not in the declared order, or when a hand-built view names a bind
/// without an evidence reference. A view projected from a checked project
/// only trips the phase errors.
pub fn derive_queue(
    view: &ArgumentView,
    phases: &PhasesSection,
    records: &[Record],
    evidence: &EvidenceSet,
) -> Result<ReviewQueue, EvalError> {
    let order = PhaseOrder::new(phases)?;
    let mut queue = ReviewQueue::default();
    for node in &view.nodes {
        match interface_hash(node, evidence)? {
            Resolution::Blocked(reason) => queue.blocked.push(BlockedItem {
                id: node.id.clone(),
                mode: node.mode,
                reason,
            }),
            Resolution::Hashable(interface) => {
                let current_hash = LocalView::of_node(view, node, interface)?.view_hash();
                match approval_status(records, node, current_hash, &order)? {
                    ApprovalStatus::Satisfied { .. } => {}
                    ApprovalStatus::Missing => {
                        queue.push_item(node, current_hash, QueueState::Missing);
                    }
                    ApprovalStatus::Stale { approvers } => {
                        queue.push_item(node, current_hash, QueueState::Stale { approvers });
                    }
                    ApprovalStatus::Declined { declines } => {
                        let mut by: Vec<String> = declines
                            .iter()
                            .map(|decline| decline.approver().to_owned())
                            .collect();
                        by.sort_unstable();
                        by.dedup();
                        queue.push_item(node, current_hash, QueueState::Declined { by });
                    }
                    ApprovalStatus::Expired { until } => queue.blocked.push(BlockedItem {
                        id: node.id.clone(),
                        mode: node.mode,
                        reason: BlockedReason::UntilExpired { until },
                    }),
                }
            }
        }
    }
    Ok(queue)
}

impl ReviewQueue {
    fn push_item(&mut self, node: &ViewNode, current_hash: ViewHash, state: QueueState) {
        self.items.push(QueueItem {
            id: node.id.clone(),
            mode: node.mode,
            current_hash,
            state,
        });
    }
}

/// Whether a node's current hash can be computed, and with which interface.
enum Resolution {
    Hashable(Option<InterfaceHash>),
    Blocked(BlockedReason),
}

/// Resolves the interface hash a node's view hash folds in, mirroring how
/// evaluation treats binds: delivered test evidence hashes, anything else is
/// blocked rather than guessed.
fn interface_hash(node: &ViewNode, evidence: &EvidenceSet) -> Result<Resolution, EvalError> {
    if node.mode != Mode::Bind {
        return Ok(Resolution::Hashable(None));
    }
    let Some(reference) = node.evidence.as_ref() else {
        // Projection always carries a bind's evidence reference; a hand-built
        // view without one cannot be hashed, which is already an error.
        return Err(LocalViewError::MissingInterfaceHash {
            id: node.id.clone(),
        }
        .into());
    };
    Ok(match reference.kind {
        EvidenceKind::Veriq => Resolution::Blocked(BlockedReason::VeriqNotEvaluated),
        EvidenceKind::Test => match evidence.get(&reference.target) {
            Some(test) => Resolution::Hashable(Some(test.interface_hash)),
            None => Resolution::Blocked(BlockedReason::MissingEvidence {
                target: reference.target.clone(),
            }),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::record::{RecordFields, Target};
    use crate::view::{Deadline, Evidence, NodeComments, ViewEdge};

    fn phases(current: &str) -> PhasesSection {
        PhasesSection {
            order: vec!["M1".to_owned(), "M2".to_owned(), "M3".to_owned()],
            current: current.to_owned(),
        }
    }

    fn node(id: &str, mode: Mode, description: &str, children: &[&str]) -> ViewNode {
        ViewNode {
            id: id.to_owned(),
            mode,
            namespace: "test".to_owned(),
            description: description.to_owned(),
            tags: Vec::new(),
            evidence: match mode {
                Mode::Bind => Some(Evidence {
                    kind: EvidenceKind::Test,
                    target: "tests/x.rs".to_owned(),
                }),
                _ => None,
            },
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
        let roots = nodes
            .iter()
            .filter(|candidate| {
                candidate.mode == Mode::Objective
                    && !nodes
                        .iter()
                        .any(|other| other.children.contains(&candidate.id))
            })
            .map(|root| root.id.clone())
            .collect();
        ArgumentView {
            nodes,
            edges,
            roots,
        }
    }

    fn hash_of(view: &ArgumentView, id: &str) -> ViewHash {
        let node = view.node(id).expect("the fixture defines the node");
        LocalView::of_node(view, node, None)
            .expect("test nodes are hashable")
            .view_hash()
    }

    fn fields(kind: Mode, id: &str, hash: ViewHash, approver: &str) -> RecordFields {
        RecordFields {
            target: Target::new(kind, id).expect("a valid target"),
            view_hash: hash,
            approver: approver.to_owned(),
            key_id: approver.to_owned(),
            timestamp: "2026-09-10T08:30:00Z".parse().expect("a valid timestamp"),
            phase: "M1".to_owned(),
            until: None,
            policy_hash: None,
        }
    }

    fn approve(kind: Mode, id: &str, hash: ViewHash, approver: &str) -> Record {
        Record::approve(fields(kind, id, hash, approver), None).expect("a valid record")
    }

    fn decline(kind: Mode, id: &str, hash: ViewHash, approver: &str, comment: &str) -> Record {
        Record::decline(fields(kind, id, hash, approver), comment.to_owned())
            .expect("a valid record")
    }

    /// objective root → structure branch → fiat leaf.
    fn chain() -> ArgumentView {
        view(vec![
            node("branch", Mode::Structure, "the branch claim", &["leaf"]),
            node("leaf", Mode::Fiat, "taken as given", &[]),
            node("root", Mode::Objective, "the root claim", &["branch"]),
        ])
    }

    fn approve_all(view: &ArgumentView) -> Vec<Record> {
        view.nodes
            .iter()
            .map(|node| approve(node.mode, &node.id, hash_of(view, &node.id), "iwata"))
            .collect()
    }

    fn queued_ids(queue: &ReviewQueue) -> Vec<&str> {
        queue.items.iter().map(|item| item.id.as_str()).collect()
    }

    #[test]
    fn a_fully_approved_argument_has_an_empty_queue() {
        // Arrange
        let view = chain();
        let records = approve_all(&view);

        // Act
        let queue = derive_queue(&view, &phases("M1"), &records, &EvidenceSet::default())
            .expect("the queue derives");

        // Assert
        assert_eq!(queue, ReviewQueue::default());
    }

    #[test]
    fn every_bare_target_queues_as_missing_in_view_order() {
        // Arrange & Act
        let view = chain();
        let queue = derive_queue(&view, &phases("M1"), &[], &EvidenceSet::default())
            .expect("the queue derives");

        // Assert: view order (sorted block IDs), every state Missing, and the
        // hash a decision would bind to is the node's current hash.
        assert_eq!(queued_ids(&queue), ["branch", "leaf", "root"]);
        assert!(
            queue
                .items
                .iter()
                .all(|item| item.state == QueueState::Missing)
        );
        assert_eq!(queue.items[1].current_hash, hash_of(&view, "leaf"));
        assert_eq!(queue.blocked, []);
    }

    #[test]
    fn editing_a_description_returns_the_target_and_its_direct_parent_as_stale() {
        // Arrange: approvals bound to the original wording, then the leaf's
        // contract changes.
        let before = chain();
        let records = approve_all(&before);
        let after = view(vec![
            node("branch", Mode::Structure, "the branch claim", &["leaf"]),
            node("leaf", Mode::Fiat, "taken as given, but edited", &[]),
            node("root", Mode::Objective, "the root claim", &["branch"]),
        ]);

        // Act
        let queue = derive_queue(&after, &phases("M1"), &records, &EvidenceSet::default())
            .expect("the queue derives");

        // Assert: exactly the leaf and its direct parent come back, as Stale.
        assert_eq!(queued_ids(&queue), ["branch", "leaf"]);
        assert!(queue.items.iter().all(|item| item.state
            == QueueState::Stale {
                approvers: vec!["iwata".to_owned()],
            }));
    }

    #[test]
    fn a_declined_target_is_shown_as_declined() {
        // Arrange: approved everywhere, then one veto on the current leaf.
        let view = chain();
        let mut records = approve_all(&view);
        records.push(decline(
            Mode::Fiat,
            "leaf",
            hash_of(&view, "leaf"),
            "sato",
            "the margin math is wrong",
        ));

        // Act
        let queue = derive_queue(&view, &phases("M1"), &records, &EvidenceSet::default())
            .expect("the queue derives");

        // Assert
        assert_eq!(queued_ids(&queue), ["leaf"]);
        assert_eq!(
            queue.items[0].state,
            QueueState::Declined {
                by: vec!["sato".to_owned()],
            }
        );
    }

    #[test]
    fn an_expired_target_is_blocked_not_queued() {
        // Arrange: a new approval would not count past the deadline, so the
        // queue must not invite one.
        let mut nodes = vec![node("leaf", Mode::Fiat, "holds until the freeze", &[])];
        nodes[0].deadline = Some(Deadline {
            until: "M2".to_owned(),
            promote: None,
        });
        let view = view(nodes);

        // Act
        let queue = derive_queue(&view, &phases("M3"), &[], &EvidenceSet::default())
            .expect("the queue derives");

        // Assert
        assert_eq!(queue.items, []);
        assert_eq!(
            queue.blocked,
            [BlockedItem {
                id: "leaf".to_owned(),
                mode: Mode::Fiat,
                reason: BlockedReason::UntilExpired {
                    until: "M2".to_owned(),
                },
            }]
        );
    }

    #[test]
    fn a_bind_without_imported_evidence_is_blocked() {
        // Arrange & Act
        let view = view(vec![node("tested", Mode::Bind, "checked in CI", &[])]);
        let queue = derive_queue(&view, &phases("M1"), &[], &EvidenceSet::default())
            .expect("the queue derives");

        // Assert: no interface hash, so nothing to decide on.
        assert_eq!(queue.items, []);
        assert_eq!(
            queue.blocked,
            [BlockedItem {
                id: "tested".to_owned(),
                mode: Mode::Bind,
                reason: BlockedReason::MissingEvidence {
                    target: "tests/x.rs".to_owned(),
                },
            }]
        );
    }

    #[test]
    fn a_veriq_bind_is_blocked_as_not_evaluated() {
        // Arrange
        let mut nodes = vec![node("calc", Mode::Bind, "verified by calc", &[])];
        nodes[0].evidence = Some(Evidence {
            kind: EvidenceKind::Veriq,
            target: "calc/margin".to_owned(),
        });
        let view = view(nodes);

        // Act
        let queue = derive_queue(&view, &phases("M1"), &[], &EvidenceSet::default())
            .expect("the queue derives");

        // Assert
        assert_eq!(
            queue.blocked,
            [BlockedItem {
                id: "calc".to_owned(),
                mode: Mode::Bind,
                reason: BlockedReason::VeriqNotEvaluated,
            }]
        );
    }

    #[test]
    fn the_queue_and_eval_agree_on_which_approvals_are_live() {
        // Arrange: one stale approval, one live, one decline — the shared
        // validity function must sort them the same way eval does.
        let view = chain();
        let old_hash = LocalView::fiat("an earlier wording", None).view_hash();
        let records = vec![
            approve(Mode::Fiat, "leaf", old_hash, "iwata"),
            approve(Mode::Structure, "branch", hash_of(&view, "branch"), "iwata"),
            decline(
                Mode::Objective,
                "root",
                hash_of(&view, "root"),
                "sato",
                "not yet",
            ),
        ];

        // Act
        let queue = derive_queue(&view, &phases("M1"), &records, &EvidenceSet::default())
            .expect("the queue derives");
        let evaluation =
            crate::eval::evaluate(&view, &phases("M1"), &records, &EvidenceSet::default())
                .expect("evaluation runs");

        // Assert: branch is satisfied in both; leaf stale, root declined, and
        // eval blames exactly the queued targets.
        assert_eq!(queued_ids(&queue), ["leaf", "root"]);
        let blamed: Vec<&str> = evaluation
            .blame
            .iter()
            .map(|blame| blame.node.as_str())
            .collect();
        assert_eq!(blamed, ["leaf", "root"]);
    }
}
