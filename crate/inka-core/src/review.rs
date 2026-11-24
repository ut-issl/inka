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
//!
//! The queue runs from the leaves up, one subtree at a time: a node comes
//! after everything below it, so a reviewer who follows the queue finishes a
//! subtree before judging the claim it supports. See [`leaves_first`].

use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::approvals::{self, StoreError};
use crate::eval::{ApprovalStatus, EvalError, PhaseOrder, approval_status};
use crate::evidence::EvidenceSet;
use crate::hash::{InterfaceHash, LocalView, LocalViewError, ViewHash};
use crate::manifest::PhasesSection;
use crate::record::{InvalidTarget, Label, Record, RecordError, RecordFields, Target, Timestamp};
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
/// leaves first, each subtree before the node it hangs from.
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
    for node in leaves_first(view) {
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

/// Every node of the view in the order review visits it: depth first from
/// each root, a node after all of its children, so a whole subtree precedes
/// the node it supports.
///
/// Roots are taken in the view's order and children in the order their block
/// declares them. A node several parents share is placed where the first of
/// them reaches it. Nodes no root reaches follow, walked the same way in the
/// view's node order, so nothing is left out. A child the view does not
/// define is skipped, and a cycle in a hand-built view is cut where the walk
/// meets a node it has already entered.
fn leaves_first(view: &ArgumentView) -> Vec<&ViewNode> {
    let by_id: HashMap<&str, &ViewNode> = view
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node))
        .collect();
    let starts = view
        .roots
        .iter()
        .filter_map(|root| by_id.get(root.as_str()).copied())
        .chain(&view.nodes);

    let mut entered: HashSet<&str> = HashSet::with_capacity(view.nodes.len());
    let mut ordered = Vec::with_capacity(view.nodes.len());
    for start in starts {
        if !entered.insert(start.id.as_str()) {
            continue;
        }
        // Each frame is a node and its children still to walk; the node is
        // emitted when the frame runs dry. A loop rather than recursion, so a
        // deep argument cannot overflow the stack.
        let mut stack = vec![(start, start.children.iter())];
        while let Some((node, children)) = stack.last_mut() {
            if let Some(child) = children.next() {
                if let Some(&child) = by_id.get(child.as_str())
                    && entered.insert(child.id.as_str())
                {
                    stack.push((child, child.children.iter()));
                }
            } else {
                ordered.push(*node);
                stack.pop();
            }
        }
    }
    ordered
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

/// A judgement on one target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// An approval, with an optional note.
    Approve {
        /// The note the record carries.
        comment: Option<String>,
    },
    /// A veto; it must say why.
    Decline {
        /// Why the content is vetoed.
        comment: String,
    },
}

impl Decision {
    /// The label the record of this decision carries.
    #[must_use]
    pub const fn label(&self) -> Label {
        match self {
            Decision::Approve { .. } => Label::Approve,
            Decision::Decline { .. } => Label::Decline,
        }
    }
}

/// Who is deciding, and where their decisions are kept: everything a record
/// needs besides the target, its hash, the decision and the instant.
#[derive(Debug, Clone, Copy)]
pub struct Reviewer<'a> {
    /// The directory records append to, one file per approver.
    pub approvals_dir: &'a Path,
    /// Who is reviewing.
    pub approver: &'a str,
    /// The key id records name; the approver name when not given.
    pub key_id: Option<&'a str>,
    /// The project's current phase, which every record carries.
    pub phase: &'a str,
}

/// Why a decision was not recorded.
#[derive(Debug, thiserror::Error)]
pub enum DecideError {
    #[error("no block named `{target}` in the argument")]
    UnknownTarget { target: String },
    /// The target has no current view hash a decision could name.
    #[error("{}", unhashable(.id, .reason))]
    Unhashable { id: String, reason: BlockedReason },
    /// The target's content is not what the decision was taken on.
    #[error(
        "`{id}` has changed since it was shown: the decision names {expected} but the current \
         content hashes to {current}"
    )]
    Changed {
        id: String,
        expected: ViewHash,
        current: ViewHash,
    },
    #[error(transparent)]
    LocalView(#[from] LocalViewError),
    #[error(transparent)]
    Target(#[from] InvalidTarget),
    #[error(transparent)]
    Record(#[from] RecordError),
    #[error(transparent)]
    Store(#[from] StoreError),
}

fn unhashable(id: &str, reason: &BlockedReason) -> String {
    match reason {
        BlockedReason::VeriqNotEvaluated => {
            format!("`{id}` is a veriq bind; veriq evidence is not evaluated yet")
        }
        BlockedReason::MissingEvidence { target } => format!(
            "bind `{id}` has no imported evidence for `{target}`; deliver the evidence first"
        ),
        BlockedReason::UntilExpired { until } => {
            format!("`{id}` is past until:{until}; no approval of it counts")
        }
    }
}

/// The view hash a decision on `node` recorded now would name.
///
/// # Errors
///
/// Returns [`DecideError::Unhashable`] for a bind whose evidence has not been
/// delivered or is not evaluated, and [`DecideError::LocalView`] when a
/// hand-built view cannot be hashed.
pub fn current_hash(
    view: &ArgumentView,
    node: &ViewNode,
    evidence: &EvidenceSet,
) -> Result<ViewHash, DecideError> {
    let interface = match interface_hash(node, evidence).map_err(local_view_error)? {
        Resolution::Hashable(interface) => interface,
        Resolution::Blocked(reason) => {
            return Err(DecideError::Unhashable {
                id: node.id.clone(),
                reason,
            });
        }
    };
    Ok(LocalView::of_node(view, node, interface)?.view_hash())
}

/// [`interface_hash`] fails only on a view that cannot be hashed.
fn local_view_error(error: EvalError) -> DecideError {
    match error {
        EvalError::LocalView(error) => DecideError::LocalView(error),
        other => unreachable!("resolving an interface hash reads no phase and no graph: {other}"),
    }
}

/// Builds the record of `decision` on `node` at `view_hash` and appends it to
/// the reviewer's file: the one write path a decision has. Nothing existing
/// is rewritten or removed.
///
/// The instant is passed in rather than read here, so the core stays free of
/// a clock.
///
/// # Errors
///
/// Returns [`DecideError::Record`] when the fields do not make a valid record
/// and [`DecideError::Store`] when the append fails.
pub fn record_decision(
    reviewer: &Reviewer<'_>,
    node: &ViewNode,
    view_hash: ViewHash,
    decision: Decision,
    timestamp: Timestamp,
) -> Result<Record, DecideError> {
    let fields = RecordFields {
        target: Target::new(node.mode, &node.id)?,
        view_hash,
        approver: reviewer.approver.to_owned(),
        key_id: reviewer.key_id.unwrap_or(reviewer.approver).to_owned(),
        timestamp,
        phase: reviewer.phase.to_owned(),
        until: None,
        policy_hash: None,
    };
    let record = match decision {
        Decision::Approve { comment } => Record::approve(fields, comment),
        Decision::Decline { comment } => Record::decline(fields, comment),
    }?;
    approvals::append(reviewer.approvals_dir, &record)?;
    Ok(record)
}

/// Records `decision` on the target named `target`, whatever its queue state:
/// an extra approval or a veto is a judgement too, and counting is eval's job.
///
/// With `expected`, the decision is recorded only if that is still the
/// target's current hash — the guard a caller needs when the content was
/// shown at one moment and judged at a later one.
///
/// # Errors
///
/// Returns [`DecideError::UnknownTarget`] when the view has no such node,
/// [`DecideError::Changed`] when `expected` is not the current hash, and
/// otherwise whatever [`current_hash`] or [`record_decision`] return.
pub fn decide(
    reviewer: &Reviewer<'_>,
    view: &ArgumentView,
    evidence: &EvidenceSet,
    target: &str,
    expected: Option<ViewHash>,
    decision: Decision,
    timestamp: Timestamp,
) -> Result<Record, DecideError> {
    let Some(node) = view.node(target) else {
        return Err(DecideError::UnknownTarget {
            target: target.to_owned(),
        });
    };
    let current = current_hash(view, node, evidence)?;
    if let Some(expected) = expected
        && expected != current
    {
        return Err(DecideError::Changed {
            id: node.id.clone(),
            expected,
            current,
        });
    }
    record_decision(reviewer, node, current, decision, timestamp)
}

#[cfg(test)]
mod tests {
    use super::*;

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

    fn reviewer(dir: &Path) -> Reviewer<'_> {
        Reviewer {
            approvals_dir: dir,
            approver: "iwata",
            key_id: None,
            phase: "M1",
        }
    }

    fn now() -> Timestamp {
        "2026-09-10T08:30:00Z".parse().expect("a valid timestamp")
    }

    fn approval() -> Decision {
        Decision::Approve { comment: None }
    }

    #[test]
    fn a_decision_is_appended_at_the_current_hash_under_the_reviewer() {
        // Arrange
        let view = chain();
        let dir = tempfile::tempdir().expect("a temporary directory");

        // Act
        let record = decide(
            &reviewer(dir.path()),
            &view,
            &EvidenceSet::default(),
            "leaf",
            None,
            approval(),
            now(),
        )
        .expect("the decision records");

        // Assert
        assert_eq!(record.view_hash(), hash_of(&view, "leaf"));
        assert_eq!(record.key_id(), "iwata");
        assert_eq!(record.phase(), "M1");
        let stored = approvals::load_dir(dir.path()).expect("the store reads back");
        assert_eq!(stored, [record]);
    }

    #[test]
    fn a_decision_on_content_that_changed_since_it_was_shown_is_refused_and_writes_nothing() {
        // Arrange: the hash on screen was the branch's, the target is the leaf.
        let view = chain();
        let dir = tempfile::tempdir().expect("a temporary directory");
        let shown = hash_of(&view, "branch");

        // Act
        let error = decide(
            &reviewer(dir.path()),
            &view,
            &EvidenceSet::default(),
            "leaf",
            Some(shown),
            approval(),
            now(),
        )
        .expect_err("the hash on screen is not the current one");

        // Assert
        assert!(matches!(
            error,
            DecideError::Changed { expected, current, .. }
                if expected == shown && current == hash_of(&view, "leaf")
        ));
        let stored = approvals::load_dir(dir.path()).expect("the store reads back");
        assert_eq!(stored, []);
    }

    #[test]
    fn a_decision_naming_the_current_hash_is_recorded() {
        // Arrange
        let view = chain();
        let dir = tempfile::tempdir().expect("a temporary directory");

        // Act
        let result = decide(
            &reviewer(dir.path()),
            &view,
            &EvidenceSet::default(),
            "leaf",
            Some(hash_of(&view, "leaf")),
            Decision::Decline {
                comment: "not yet".to_owned(),
            },
            now(),
        );

        // Assert
        let record = result.expect("the hash matches");
        assert_eq!(record.label(), Label::Decline);
        assert_eq!(record.comment(), Some("not yet"));
    }

    #[test]
    fn a_decision_on_an_unknown_target_or_an_undelivered_bind_says_why() {
        // Arrange
        let view = view(vec![
            node("bound", Mode::Bind, "evidence says so", &[]),
            node("root", Mode::Objective, "the root claim", &["bound"]),
        ]);
        let dir = tempfile::tempdir().expect("a temporary directory");
        let decide_on = |target: &str| {
            decide(
                &reviewer(dir.path()),
                &view,
                &EvidenceSet::default(),
                target,
                None,
                approval(),
                now(),
            )
            .expect_err("no decision can be recorded")
            .to_string()
        };

        // Act & Assert
        assert_eq!(
            decide_on("absent"),
            "no block named `absent` in the argument"
        );
        assert_eq!(
            decide_on("bound"),
            "bind `bound` has no imported evidence for `tests/x.rs`; deliver the evidence first"
        );
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
    fn every_bare_target_queues_as_missing_leaves_first() {
        // Arrange & Act
        let view = chain();
        let queue = derive_queue(&view, &phases("M1"), &[], &EvidenceSet::default())
            .expect("the queue derives");

        // Assert: leaves first, every state Missing, and the hash a decision
        // would bind to is the node's current hash.
        assert_eq!(queued_ids(&queue), ["leaf", "branch", "root"]);
        assert!(
            queue
                .items
                .iter()
                .all(|item| item.state == QueueState::Missing)
        );
        assert_eq!(queue.items[0].current_hash, hash_of(&view, "leaf"));
        assert_eq!(queue.blocked, []);
    }

    #[test]
    fn a_subtree_is_queued_whole_before_the_node_it_supports() {
        // Arrange: two subtrees whose ids sort against the tree's shape, and
        // children declared in an order that is not alphabetical.
        let view = view(vec![
            node(
                "a_root",
                Mode::Objective,
                "the root claim",
                &["z_left", "b_right"],
            ),
            node(
                "b_right",
                Mode::Structure,
                "the right claim",
                &["c_right_leaf"],
            ),
            node("c_right_leaf", Mode::Fiat, "right, taken as given", &[]),
            node("y_left_leaf", Mode::Fiat, "left, taken as given", &[]),
            node(
                "z_left",
                Mode::Structure,
                "the left claim",
                &["y_left_leaf", "x_left_leaf"],
            ),
            node("x_left_leaf", Mode::Fiat, "left again, taken as given", &[]),
        ]);

        // Act
        let queue = derive_queue(&view, &phases("M1"), &[], &EvidenceSet::default())
            .expect("the queue derives");

        // Assert: each structure follows its own leaves, siblings keep their
        // declared order, and the root comes last.
        assert_eq!(
            queued_ids(&queue),
            [
                "y_left_leaf",
                "x_left_leaf",
                "z_left",
                "c_right_leaf",
                "b_right",
                "a_root",
            ]
        );
    }

    #[test]
    fn a_shared_node_is_queued_once_under_its_first_parent() {
        // Arrange: both branches cite the same leaf.
        let view = view(vec![
            node("first", Mode::Structure, "the first claim", &["shared"]),
            node(
                "root",
                Mode::Objective,
                "the root claim",
                &["second", "first"],
            ),
            node(
                "second",
                Mode::Structure,
                "the second claim",
                &["own", "shared"],
            ),
            node("own", Mode::Fiat, "taken as given", &[]),
            node("shared", Mode::Fiat, "cited twice", &[]),
        ]);

        // Act
        let queue = derive_queue(&view, &phases("M1"), &[], &EvidenceSet::default())
            .expect("the queue derives");

        // Assert
        assert_eq!(
            queued_ids(&queue),
            ["own", "shared", "second", "first", "root"]
        );
    }

    #[test]
    fn satisfied_nodes_leave_the_order_of_the_rest_unchanged() {
        // Arrange: the middle of the chain is approved, its neighbours not.
        let view = chain();
        let records = vec![approve(
            Mode::Structure,
            "branch",
            hash_of(&view, "branch"),
            "iwata",
        )];

        // Act
        let queue = derive_queue(&view, &phases("M1"), &records, &EvidenceSet::default())
            .expect("the queue derives");

        // Assert
        assert_eq!(queued_ids(&queue), ["leaf", "root"]);
    }

    #[test]
    fn nodes_no_root_reaches_follow_the_rooted_ones_leaves_first() {
        // Arrange: an orphaned structure and its leaf beside the main chain.
        let view = view(vec![
            node(
                "a_orphan",
                Mode::Structure,
                "nothing cites this",
                &["b_orphan_leaf"],
            ),
            node("b_orphan_leaf", Mode::Fiat, "taken as given", &[]),
            node("branch", Mode::Structure, "the branch claim", &["leaf"]),
            node("leaf", Mode::Fiat, "taken as given", &[]),
            node("root", Mode::Objective, "the root claim", &["branch"]),
        ]);

        // Act
        let queue = derive_queue(&view, &phases("M1"), &[], &EvidenceSet::default())
            .expect("the queue derives");

        // Assert
        assert_eq!(
            queued_ids(&queue),
            ["leaf", "branch", "root", "b_orphan_leaf", "a_orphan"]
        );
    }

    #[test]
    fn blocked_targets_are_listed_leaves_first_too() {
        // Arrange: two undelivered binds whose ids sort against the tree.
        let view = view(vec![
            node("a_upper", Mode::Bind, "checked in CI", &["z_lower"]),
            node("root", Mode::Objective, "the root claim", &["a_upper"]),
            node("z_lower", Mode::Bind, "checked in CI", &[]),
        ]);

        // Act
        let queue = derive_queue(&view, &phases("M1"), &[], &EvidenceSet::default())
            .expect("the queue derives");

        // Assert
        let blocked: Vec<&str> = queue.blocked.iter().map(|item| item.id.as_str()).collect();
        assert_eq!(blocked, ["z_lower", "a_upper"]);
    }

    #[test]
    fn a_cycle_in_a_hand_built_view_still_lists_every_node_once() {
        // Arrange: a checked project cannot contain a cycle; the walk must
        // end all the same.
        let view = view(vec![
            node("a", Mode::Structure, "leans on b", &["b"]),
            node("b", Mode::Structure, "leans on a", &["a"]),
        ]);

        // Act
        let order: Vec<&str> = leaves_first(&view)
            .into_iter()
            .map(|node| node.id.as_str())
            .collect();

        // Assert
        assert_eq!(order, ["b", "a"]);
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
        assert_eq!(queued_ids(&queue), ["leaf", "branch"]);
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
