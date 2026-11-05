//! Evaluation: which nodes are established, and who is to blame.
//!
//! `inka eval` answers one question about a loaded argument: is it currently
//! valid? The answer is computed from repository content alone — the argument,
//! the approval records, and the declared phase order. No clock is read and no
//! external API is called, so the same repository state always evaluates the
//! same way, on any machine.
//!
//! Whether one approval still counts is decided by [`approval_status`], which
//! is the shared validity function: the review-queue derivation must agree
//! with evaluation about which approvals are live, so both read this one
//! judgement. A record counts iff its target and view hash match the current
//! values, no decline exists on the same hash (a decline is a veto regardless
//! of quorum), and the node's `until:` has not expired against the declared
//! phase order. Expiry is hard: once the current phase is past `until:`, no
//! approval of that node counts — including approvals granted after the
//! deadline — because an `until:` is a jointly-approved decline placed toward
//! the future, and lifting it means editing the annotation, which changes the
//! contract and demands fresh approval.
//!
//! `established` is a fixpoint over the DAG, computed leaves first: a fiat is
//! established by a satisfied approval alone; a structure or objective needs a
//! satisfied approval and every child established; a bind needs a satisfied
//! approval and passing test evidence. A bind's current view hash is computed
//! from the interface hash of the imported evidence its `test:` reference
//! names, so approval staleness and evidence failure stay independent causes;
//! a reference no loaded evidence provides is the per-bind missing-evidence
//! cause, not an input error. veriq evidence is not evaluated yet.
//!
//! Blame lists only nodes whose failure cause is their own — a missing, stale,
//! declined, or expired approval, or missing/failed/incomplete evidence —
//! never nodes failing solely through their children. The unestablished
//! child is the finding; its
//! ancestors are consequences.
//!
//! Policy is the built-in default only: every target needs `any(1)`, with the
//! decline veto on top.

use std::collections::{HashMap, VecDeque};
use std::fmt;

use serde::Serialize;

use crate::evidence::{EvidenceSet, Outcome, TestEvidence};
use crate::hash::{LocalView, LocalViewError, ViewHash};
use crate::manifest::PhasesSection;
use crate::record::{Label, Record};
use crate::view::{ArgumentView, EvidenceKind, Mode, ViewNode};

/// The declared phase order, resolved to ordinals for expiry comparison.
///
/// Phases are compared by their position in `phases.order`, never by a clock:
/// a node with `until: M2` is expired exactly when the current phase sits
/// after `M2` in the declared order.
#[derive(Debug, Clone)]
pub struct PhaseOrder {
    ordinals: HashMap<String, usize>,
    current: usize,
}

impl PhaseOrder {
    /// Resolves a manifest's phase section into comparable ordinals.
    ///
    /// # Errors
    ///
    /// Returns [`EvalError::CurrentPhaseNotInOrder`] when the current phase is
    /// not listed in the order. A manifest the loader validated cannot trip
    /// this; a hand-built section can.
    pub fn new(phases: &PhasesSection) -> Result<Self, EvalError> {
        let ordinals: HashMap<String, usize> = phases
            .order
            .iter()
            .enumerate()
            .map(|(ordinal, name)| (name.clone(), ordinal))
            .collect();
        let current =
            *ordinals
                .get(&phases.current)
                .ok_or_else(|| EvalError::CurrentPhaseNotInOrder {
                    current: phases.current.clone(),
                })?;
        Ok(Self { ordinals, current })
    }

    /// Whether the current phase is past `until`, or `None` when `until` is
    /// not a declared phase and therefore cannot be compared.
    fn is_past(&self, until: &str) -> Option<bool> {
        self.ordinals
            .get(until)
            .map(|&ordinal| self.current > ordinal)
    }
}

/// What the records say about one node, under the default `any(1)` policy.
///
/// The variants are mutually exclusive and decided in veto order: a decline on
/// the current hash wins over everything, expiry wins over counting, and only
/// then does an approval on the current hash satisfy the node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalStatus<'r> {
    /// At least one approval counts.
    Satisfied {
        /// The approvals that count, in record order.
        approvals: Vec<&'r Record>,
    },
    /// A decline exists on the current hash: a veto regardless of quorum.
    Declined {
        /// Every decline on the current hash, in record order.
        declines: Vec<&'r Record>,
    },
    /// The node's `until:` has expired; no approval counts any more.
    Expired {
        /// The phase the node was approved toward.
        until: String,
    },
    /// Approvals exist, but only for earlier content.
    Stale {
        /// Who approved an earlier version, sorted and deduplicated.
        approvers: Vec<String>,
    },
    /// No record targets this node at all.
    Missing,
}

/// Judges one node's records against its current content.
///
/// This is the validity function evaluation and the review-queue derivation
/// share. The judgement, in order:
///
/// 1. Only records whose target and view hash match the current values are
///    considered live; a mismatch is stale — not an error, just not counted.
/// 2. Any decline on the current hash is a veto regardless of quorum.
/// 3. When the node's `until:` has expired, no approval counts — including
///    approvals granted after the deadline; `record.phase` is never read.
/// 4. What remains satisfies the node when at least one approval is left, the
///    built-in `any(1)` policy.
///
/// # Errors
///
/// Returns [`EvalError::UnknownUntilPhase`] when the node's `until:` names a
/// phase the declared order does not list: an incomparable deadline must fail
/// loudly, not silently count or discount approvals.
pub fn approval_status<'r>(
    records: &'r [Record],
    node: &ViewNode,
    current_hash: ViewHash,
    phases: &PhaseOrder,
) -> Result<ApprovalStatus<'r>, EvalError> {
    let targeting: Vec<&Record> = records
        .iter()
        .filter(|record| record.target().kind() == node.mode && record.target().id() == node.id)
        .collect();
    let (live, stale): (Vec<&Record>, Vec<&Record>) = targeting
        .into_iter()
        .partition(|record| record.view_hash() == current_hash);

    let declines: Vec<&Record> = live
        .iter()
        .filter(|record| record.label() == Label::Decline)
        .copied()
        .collect();
    if !declines.is_empty() {
        return Ok(ApprovalStatus::Declined { declines });
    }

    if let Some(until) = expired_deadline(node, phases)? {
        return Ok(ApprovalStatus::Expired { until });
    }

    let approvals: Vec<&Record> = live
        .into_iter()
        .filter(|record| record.label() == Label::Approve)
        .collect();
    if !approvals.is_empty() {
        return Ok(ApprovalStatus::Satisfied { approvals });
    }

    let mut approvers: Vec<String> = stale
        .into_iter()
        .filter(|record| record.label() == Label::Approve)
        .map(|record| record.approver().to_owned())
        .collect();
    approvers.sort_unstable();
    approvers.dedup();
    if approvers.is_empty() {
        Ok(ApprovalStatus::Missing)
    } else {
        Ok(ApprovalStatus::Stale { approvers })
    }
}

/// The node's `until:` phase when it has expired, `None` when it has not or
/// the node carries no deadline. Hash-independent, so callers that cannot
/// compute a view hash can still judge it.
///
/// # Errors
///
/// Returns [`EvalError::UnknownUntilPhase`] when `until:` names a phase the
/// declared order does not list.
fn expired_deadline(node: &ViewNode, phases: &PhaseOrder) -> Result<Option<String>, EvalError> {
    let Some(deadline) = &node.deadline else {
        return Ok(None);
    };
    let expired = phases
        .is_past(&deadline.until)
        .ok_or_else(|| EvalError::UnknownUntilPhase {
            id: node.id.clone(),
            until: deadline.until.clone(),
        })?;
    Ok(expired.then(|| deadline.until.clone()))
}

/// A cause a node fails on its own, as blame reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "cause", rename_all = "snake_case")]
pub enum BlameReason {
    /// No record targets the node's current content or any earlier version.
    MissingApproval,
    /// Approvals exist, but only for earlier content.
    StaleApproval { approvers: Vec<String> },
    /// A decline on the current content: a veto regardless of quorum.
    Declined { by: String, comment: String },
    /// The node's `until:` has expired; no approval counts any more.
    UntilExpired { until: String },
    /// No loaded test evidence provides the bind's `test:` reference.
    MissingEvidence { target: String },
    /// Required executions reported `fail`.
    EvidenceFailed { cases: Vec<String> },
    /// Required executions were skipped, cancelled or reported no result.
    EvidenceIncomplete { cases: Vec<String> },
    /// veriq evidence is not evaluated yet.
    VeriqNotEvaluated,
}

impl fmt::Display for BlameReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BlameReason::MissingApproval => f.write_str("no approval covers the current content"),
            BlameReason::StaleApproval { approvers } => write!(
                f,
                "approvals are stale: {} approved an earlier version",
                approvers.join(", ")
            ),
            BlameReason::Declined { by, comment } => write!(f, "declined by {by}: {comment}"),
            BlameReason::UntilExpired { until } => write!(
                f,
                "approvals no longer count: the current phase is past until:{until}"
            ),
            BlameReason::MissingEvidence { target } => {
                write!(f, "no imported test evidence provides `{target}`")
            }
            BlameReason::EvidenceFailed { cases } => {
                write!(f, "required executions failed: {}", cases.join(", "))
            }
            BlameReason::EvidenceIncomplete { cases } => write!(
                f,
                "required executions were skipped, cancelled or missing: {}",
                cases.join(", ")
            ),
            BlameReason::VeriqNotEvaluated => f.write_str("veriq evidence is not evaluated yet"),
        }
    }
}

/// One node that fails through its own cause, with every reason it does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Blame {
    /// The failing node's id.
    pub node: String,
    /// The failing node's mode, so the finding reads `<kind>:<id>`.
    pub mode: Mode,
    /// Every own cause the node fails on, in veto order.
    pub reasons: Vec<BlameReason>,
}

/// One node's verdict.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NodeStatus {
    /// The node's id.
    pub id: String,
    /// The node's mode.
    pub mode: Mode,
    /// Whether the node is established under the current records and phase.
    pub established: bool,
}

/// The whole argument, evaluated.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Evaluation {
    /// Every node's verdict, in the view's sorted block-ID order.
    pub nodes: Vec<NodeStatus>,
    /// Every node failing through its own cause, in the same order.
    pub blame: Vec<Blame>,
}

impl Evaluation {
    /// How many nodes are established.
    #[must_use]
    pub fn established_count(&self) -> usize {
        self.nodes.iter().filter(|node| node.established).count()
    }

    /// How many nodes the argument has.
    #[must_use]
    pub fn total(&self) -> usize {
        self.nodes.len()
    }

    /// Whether every node is established.
    #[must_use]
    pub fn is_clean(&self) -> bool {
        self.nodes.iter().all(|node| node.established)
    }
}

/// A reason evaluation could not run at all.
///
/// These are failures of the inputs, not verdicts about the argument: a
/// verdict — however bad — is an [`Evaluation`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EvalError {
    #[error("phases.current `{current}` is not listed in phases.order")]
    CurrentPhaseNotInOrder { current: String },
    #[error("`{id}` names until:{until}, which phases.order does not list")]
    UnknownUntilPhase { id: String, until: String },
    #[error(transparent)]
    LocalView(#[from] LocalViewError),
    #[error("the argument graph has a cycle; run `inka check` first")]
    Cycle,
}

/// Evaluates a whole argument against its records and phase order.
///
/// Nodes are judged leaves first, so every parent reads settled child
/// verdicts. The result carries every node's verdict and the blame list; the
/// caller decides how bad a verdict is (the CLI maps a non-clean evaluation to
/// a non-zero exit status).
///
/// # Errors
///
/// Returns an [`EvalError`] when the current phase or a node's `until:` phase
/// is not in the declared order, when the view is not a DAG, or when a
/// hand-built view names a child it does not define. A view projected from a
/// checked project only trips the phase errors.
pub fn evaluate(
    view: &ArgumentView,
    phases: &PhasesSection,
    records: &[Record],
    evidence: &EvidenceSet,
) -> Result<Evaluation, EvalError> {
    let order = PhaseOrder::new(phases)?;

    let index_of: HashMap<&str, usize> = view
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| (node.id.as_str(), index))
        .collect();
    let (parents_of, mut pending_children) = dependencies(view, &index_of);

    let mut queue: VecDeque<usize> = pending_children
        .iter()
        .enumerate()
        .filter(|&(_, &pending)| pending == 0)
        .map(|(index, _)| index)
        .collect();

    let mut established: Vec<Option<bool>> = vec![None; view.nodes.len()];
    let mut reasons: Vec<Vec<BlameReason>> = vec![Vec::new(); view.nodes.len()];
    while let Some(index) = queue.pop_front() {
        let node = &view.nodes[index];
        let (satisfied, own_reasons) = judge(view, node, records, &order, evidence)?;
        let children_established = node.children.iter().all(|child| {
            index_of
                .get(child.as_str())
                .is_some_and(|&child_index| established[child_index] == Some(true))
        });
        established[index] = Some(match node.mode {
            // A fiat needs its approval alone; for a bind, judge already
            // folded the evidence verdict into `satisfied`, and neither has
            // children to wait for.
            Mode::Fiat | Mode::Bind => satisfied,
            Mode::Structure | Mode::Objective => satisfied && children_established,
        });
        reasons[index] = own_reasons;
        for &parent in &parents_of[index] {
            pending_children[parent] -= 1;
            if pending_children[parent] == 0 {
                queue.push_back(parent);
            }
        }
    }
    if established.iter().any(Option::is_none) {
        return Err(EvalError::Cycle);
    }
    Ok(assemble(view, &established, reasons))
}

/// The reverse adjacency and per-node dependency counts Kahn's algorithm
/// consumes, so a node is judged only after every child it depends on.
///
/// Both are read from each node's own child list — the same list the
/// judgement walks — so evaluation never assumes `view.edges` agrees with it.
/// A child id the view does not define can only appear in a hand-built view;
/// the judging pass reports it through [`LocalView::of_node`] rather than
/// panicking here.
fn dependencies(
    view: &ArgumentView,
    index_of: &HashMap<&str, usize>,
) -> (Vec<Vec<usize>>, Vec<usize>) {
    let mut parents_of: Vec<Vec<usize>> = vec![Vec::new(); view.nodes.len()];
    let mut pending_children: Vec<usize> = vec![0; view.nodes.len()];
    for (parent, node) in view.nodes.iter().enumerate() {
        for child in &node.children {
            if let Some(&child_index) = index_of.get(child.as_str()) {
                parents_of[child_index].push(parent);
                pending_children[parent] += 1;
            }
        }
    }
    (parents_of, pending_children)
}

/// Folds the per-node verdicts and reasons into the evaluation, keeping the
/// view's sorted block-ID order in both lists.
fn assemble(
    view: &ArgumentView,
    established: &[Option<bool>],
    reasons: Vec<Vec<BlameReason>>,
) -> Evaluation {
    let nodes = view
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| NodeStatus {
            id: node.id.clone(),
            mode: node.mode,
            established: established[index] == Some(true),
        })
        .collect();
    let blame = view
        .nodes
        .iter()
        .zip(reasons)
        .filter(|(_, reasons)| !reasons.is_empty())
        .map(|(node, reasons)| Blame {
            node: node.id.clone(),
            mode: node.mode,
            reasons,
        })
        .collect();
    Evaluation { nodes, blame }
}

/// Judges one node's own cause: whether its approval is satisfied, and the
/// blame reasons it carries itself.
///
/// A bind is judged by [`judge_bind`]: its view hash depends on the imported
/// evidence, and its verdict folds the evidence outcome in.
fn judge(
    view: &ArgumentView,
    node: &ViewNode,
    records: &[Record],
    phases: &PhaseOrder,
    evidence: &EvidenceSet,
) -> Result<(bool, Vec<BlameReason>), EvalError> {
    if node.mode == Mode::Bind {
        return judge_bind(view, node, records, phases, evidence);
    }

    let current_hash = LocalView::of_node(view, node, None)?.view_hash();
    let status = approval_status(records, node, current_hash, phases)?;
    Ok(approval_verdict(status))
}

/// Judges a bind: approval and evidence are independent causes, and both
/// must hold for the bind to be established.
///
/// Without imported evidence for the `test:` reference there is no interface
/// hash, so no current view hash and no record-based judgement. The `until:`
/// deadline needs no hash, so it is still judged: an expired bind is reported
/// as both expired and missing its evidence, and an unknown `until:` phase
/// fails loudly whether or not the evidence has been delivered. veriq
/// references stay unevaluated.
fn judge_bind(
    view: &ArgumentView,
    node: &ViewNode,
    records: &[Record],
    phases: &PhaseOrder,
    evidence: &EvidenceSet,
) -> Result<(bool, Vec<BlameReason>), EvalError> {
    let Some(reference) = node.evidence.as_ref() else {
        // Projection always carries a bind's evidence reference; a hand-built
        // view without one cannot be hashed, which is already an error.
        return Err(LocalViewError::MissingInterfaceHash {
            id: node.id.clone(),
        }
        .into());
    };
    match reference.kind {
        EvidenceKind::Veriq => Ok((false, vec![BlameReason::VeriqNotEvaluated])),
        EvidenceKind::Test => {
            let Some(test) = evidence.get(&reference.target) else {
                let mut reasons = Vec::new();
                if let Some(until) = expired_deadline(node, phases)? {
                    reasons.push(BlameReason::UntilExpired { until });
                }
                reasons.push(BlameReason::MissingEvidence {
                    target: reference.target.clone(),
                });
                return Ok((false, reasons));
            };
            let current_hash =
                LocalView::of_node(view, node, Some(test.interface_hash))?.view_hash();
            let status = approval_status(records, node, current_hash, phases)?;
            let (approved, mut reasons) = approval_verdict(status);
            let evidence_blame = evidence_reasons(test);
            let passing = evidence_blame.is_empty();
            reasons.extend(evidence_blame);
            Ok((approved && passing, reasons))
        }
    }
}

/// Folds nonpass executions into blame: failures and skipped/cancelled/
/// missing results are distinct causes, reported together when both occur.
/// An empty result means every execution passed, which is the bind's
/// evidence verdict; the match is exhaustive so a new outcome cannot leave
/// a bind unestablished without a reason.
fn evidence_reasons(test: &TestEvidence) -> Vec<BlameReason> {
    let mut failed = Vec::new();
    let mut incomplete = Vec::new();
    for execution in &test.executions {
        match execution.outcome {
            Some(Outcome::Pass) => {}
            Some(Outcome::Fail) => failed.push(execution.case.clone()),
            None | Some(Outcome::Skip | Outcome::Cancelled) => {
                incomplete.push(execution.case.clone());
            }
        }
    }
    let mut reasons = Vec::new();
    if !failed.is_empty() {
        reasons.push(BlameReason::EvidenceFailed { cases: failed });
    }
    if !incomplete.is_empty() {
        reasons.push(BlameReason::EvidenceIncomplete { cases: incomplete });
    }
    reasons
}

/// Maps an approval status onto the shared (satisfied, blame) shape.
fn approval_verdict(status: ApprovalStatus<'_>) -> (bool, Vec<BlameReason>) {
    match status {
        ApprovalStatus::Satisfied { .. } => (true, Vec::new()),
        ApprovalStatus::Declined { declines } => {
            let reasons = declines
                .iter()
                .map(|decline| BlameReason::Declined {
                    by: decline.approver().to_owned(),
                    comment: decline
                        .comment()
                        .expect("a decline record carries a comment")
                        .to_owned(),
                })
                .collect();
            (false, reasons)
        }
        ApprovalStatus::Expired { until } => (false, vec![BlameReason::UntilExpired { until }]),
        ApprovalStatus::Stale { approvers } => {
            (false, vec![BlameReason::StaleApproval { approvers }])
        }
        ApprovalStatus::Missing => (false, vec![BlameReason::MissingApproval]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::record::{RecordFields, Target};
    use crate::view::{Deadline, Evidence, EvidenceKind, NodeComments, ViewEdge};

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

    /// Assembles a view from nodes, deriving edges the way projection does.
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

    /// The current view hash of one node, as evaluation recomputes it.
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
            key_id: format!("{approver}-2026"),
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

    /// One approval per node, bound to the current hashes of `view`.
    fn approve_all(view: &ArgumentView) -> Vec<Record> {
        view.nodes
            .iter()
            .map(|node| approve(node.mode, &node.id, hash_of(view, &node.id), "iwata"))
            .collect()
    }

    fn established_ids(evaluation: &Evaluation) -> Vec<&str> {
        evaluation
            .nodes
            .iter()
            .filter(|node| node.established)
            .map(|node| node.id.as_str())
            .collect()
    }

    fn blamed_ids(evaluation: &Evaluation) -> Vec<&str> {
        evaluation
            .blame
            .iter()
            .map(|blame| blame.node.as_str())
            .collect()
    }

    #[test]
    fn a_fully_approved_argument_is_clean() {
        // Arrange
        let view = chain();
        let records = approve_all(&view);

        // Act
        let evaluation = evaluate(&view, &phases("M1"), &records, &EvidenceSet::default())
            .expect("evaluation runs");

        // Assert: the fixpoint reaches the objective and nothing is blamed.
        assert!(evaluation.is_clean());
        assert_eq!(evaluation.established_count(), 3);
        assert_eq!(evaluation.blame, []);
    }

    #[test]
    fn a_missing_approval_blames_only_the_node_itself() {
        // Arrange: everyone approved except the leaf.
        let view = chain();
        let records: Vec<Record> = approve_all(&view)
            .into_iter()
            .filter(|record| record.target().id() != "leaf")
            .collect();

        // Act
        let evaluation = evaluate(&view, &phases("M1"), &records, &EvidenceSet::default())
            .expect("evaluation runs");

        // Assert: the ancestors fail only through the leaf, so only the leaf
        // is blamed — and its reason is its own.
        assert_eq!(established_ids(&evaluation), Vec::<&str>::new());
        assert_eq!(blamed_ids(&evaluation), ["leaf"]);
        assert_eq!(evaluation.blame[0].reasons, [BlameReason::MissingApproval]);
    }

    #[test]
    fn editing_a_leaf_blames_the_leaf_and_its_direct_parent_only() {
        // Arrange: approvals were granted against the original wording…
        let before = chain();
        let records = approve_all(&before);
        // …and then the leaf's contract changed.
        let after = view(vec![
            node("branch", Mode::Structure, "the branch claim", &["leaf"]),
            node("leaf", Mode::Fiat, "taken as given, but edited", &[]),
            node("root", Mode::Objective, "the root claim", &["branch"]),
        ]);

        // Act
        let evaluation = evaluate(&after, &phases("M1"), &records, &EvidenceSet::default())
            .expect("evaluation runs");

        // Assert: the leaf and its direct parent are stale; the grandparent's
        // hash covers only its own children's text, so it stays satisfied and
        // is not blamed — the most important sanity check of local hashing.
        assert_eq!(blamed_ids(&evaluation), ["branch", "leaf"]);
        assert!(evaluation.blame.iter().all(|blame| blame.reasons
            == [BlameReason::StaleApproval {
                approvers: vec!["iwata".to_owned()],
            }]));
        assert_eq!(established_ids(&evaluation), Vec::<&str>::new());
    }

    #[test]
    fn a_decline_vetoes_however_many_approvals_exist() {
        // Arrange: two approvals and one decline, all on the current hash.
        let view = chain();
        let leaf_hash = hash_of(&view, "leaf");
        let mut records = approve_all(&view);
        records.push(approve(Mode::Fiat, "leaf", leaf_hash, "tanaka"));
        records.push(decline(
            Mode::Fiat,
            "leaf",
            leaf_hash,
            "sato",
            "the margin math is wrong",
        ));

        // Act
        let evaluation = evaluate(&view, &phases("M1"), &records, &EvidenceSet::default())
            .expect("evaluation runs");

        // Assert
        assert_eq!(blamed_ids(&evaluation), ["leaf"]);
        assert_eq!(
            evaluation.blame[0].reasons,
            [BlameReason::Declined {
                by: "sato".to_owned(),
                comment: "the margin math is wrong".to_owned(),
            }]
        );
    }

    #[test]
    fn a_decline_on_an_earlier_version_does_not_veto() {
        // Arrange: the decline is bound to a hash the argument no longer has.
        let view = chain();
        let mut records = approve_all(&view);
        let old_hash = LocalView::fiat("an earlier wording", None).view_hash();
        records.push(decline(
            Mode::Fiat,
            "leaf",
            old_hash,
            "sato",
            "objection to the earlier wording",
        ));

        // Act
        let evaluation = evaluate(&view, &phases("M1"), &records, &EvidenceSet::default())
            .expect("evaluation runs");

        // Assert: a stale decline is simply not counted.
        assert!(evaluation.is_clean());
    }

    #[test]
    fn an_expired_until_discounts_even_current_approvals() {
        // Arrange: the leaf was approved toward M2, and the project moved on.
        // A post-deadline approval is the same record set — an approval on the
        // current hash — and must not count either.
        let mut nodes = vec![
            node("leaf", Mode::Fiat, "holds until the freeze", &[]),
            node("root", Mode::Objective, "the root claim", &["leaf"]),
        ];
        nodes[0].deadline = Some(Deadline {
            until: "M2".to_owned(),
            promote: None,
        });
        let view = view(nodes);
        let records = approve_all(&view);

        // Act
        let at_m2 = evaluate(&view, &phases("M2"), &records, &EvidenceSet::default())
            .expect("evaluation runs");
        let at_m3 = evaluate(&view, &phases("M3"), &records, &EvidenceSet::default())
            .expect("evaluation runs");

        // Assert: valid at the deadline, hard-expired one phase later.
        assert!(at_m2.is_clean());
        assert_eq!(blamed_ids(&at_m3), ["leaf"]);
        assert_eq!(
            at_m3.blame[0].reasons,
            [BlameReason::UntilExpired {
                until: "M2".to_owned(),
            }]
        );
    }

    #[test]
    fn a_test_bind_without_imported_evidence_is_blamed_for_missing_evidence() {
        // Arrange
        let view = view(vec![
            node("root", Mode::Objective, "the root claim", &["tested"]),
            node("tested", Mode::Bind, "checked in CI", &[]),
        ]);

        // Act: no records at all — the missing-evidence lookup returns before
        // approval checking.
        let evaluation =
            evaluate(&view, &phases("M1"), &[], &EvidenceSet::default()).expect("evaluation runs");

        // Assert
        let tested = evaluation
            .blame
            .iter()
            .find(|blame| blame.node == "tested")
            .expect("the bind is blamed");
        assert_eq!(
            tested.reasons,
            [BlameReason::MissingEvidence {
                target: "tests/x.rs".to_owned()
            }]
        );
        assert!(
            !evaluation
                .nodes
                .iter()
                .find(|node| node.id == "tested")
                .expect("the bind has a verdict")
                .established
        );
    }

    #[test]
    fn an_unapproved_objective_is_blamed_itself() {
        // Arrange: the children are all established; only the root is bare.
        let view = chain();
        let records: Vec<Record> = approve_all(&view)
            .into_iter()
            .filter(|record| record.target().id() != "root")
            .collect();

        // Act
        let evaluation = evaluate(&view, &phases("M1"), &records, &EvidenceSet::default())
            .expect("evaluation runs");

        // Assert: the root's wording is agreed like any other contract.
        assert_eq!(blamed_ids(&evaluation), ["root"]);
        assert_eq!(evaluation.blame[0].reasons, [BlameReason::MissingApproval]);
        assert_eq!(established_ids(&evaluation), ["branch", "leaf"]);
    }

    #[test]
    fn an_approval_under_the_wrong_kind_does_not_count_and_is_not_stale() {
        // Arrange: the record targets `fiat:branch`, but `branch` is a
        // structure — re-moding a block stops old approvals from matching.
        let view = chain();
        let mut records: Vec<Record> = approve_all(&view)
            .into_iter()
            .filter(|record| record.target().id() != "branch")
            .collect();
        records.push(approve(
            Mode::Fiat,
            "branch",
            hash_of(&view, "branch"),
            "iwata",
        ));

        // Act
        let evaluation = evaluate(&view, &phases("M1"), &records, &EvidenceSet::default())
            .expect("evaluation runs");

        // Assert: a kind mismatch is a different target entirely, so the node
        // reads as missing, not stale.
        assert_eq!(blamed_ids(&evaluation), ["branch"]);
        assert_eq!(evaluation.blame[0].reasons, [BlameReason::MissingApproval]);
    }

    #[test]
    fn a_declined_node_reports_the_veto_over_its_expiry() {
        // Arrange: the node is both declined and expired; the veto order puts
        // the decline first, so the accountable objection is what surfaces.
        let mut nodes = vec![
            node("leaf", Mode::Fiat, "holds until the freeze", &[]),
            node("root", Mode::Objective, "the root claim", &["leaf"]),
        ];
        nodes[0].deadline = Some(Deadline {
            until: "M2".to_owned(),
            promote: None,
        });
        let view = view(nodes);
        let mut records = approve_all(&view);
        records.push(decline(
            Mode::Fiat,
            "leaf",
            hash_of(&view, "leaf"),
            "sato",
            "the assumption no longer holds",
        ));

        // Act
        let evaluation = evaluate(&view, &phases("M3"), &records, &EvidenceSet::default())
            .expect("evaluation runs");

        // Assert
        assert_eq!(
            evaluation.blame[0].reasons,
            [BlameReason::Declined {
                by: "sato".to_owned(),
                comment: "the assumption no longer holds".to_owned(),
            }]
        );
    }

    #[test]
    fn a_test_bind_without_evidence_is_still_judged_against_its_deadline() {
        // Arrange: the bind's evidence has not been delivered, and its
        // `until:` is already behind the project.
        let mut nodes = vec![
            node("root", Mode::Objective, "the root claim", &["tested"]),
            node("tested", Mode::Bind, "checked in CI", &[]),
        ];
        nodes[1].deadline = Some(Deadline {
            until: "M2".to_owned(),
            promote: None,
        });
        let view = view(nodes);

        // Act
        let evaluation =
            evaluate(&view, &phases("M3"), &[], &EvidenceSet::default()).expect("evaluation runs");

        // Assert: both hash-independent causes are reported, expiry first as
        // in the evidence-present path.
        let tested = evaluation
            .blame
            .iter()
            .find(|blame| blame.node == "tested")
            .expect("the bind is blamed");
        assert_eq!(
            tested.reasons,
            [
                BlameReason::UntilExpired {
                    until: "M2".to_owned(),
                },
                BlameReason::MissingEvidence {
                    target: "tests/x.rs".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn a_test_bind_without_evidence_fails_loudly_on_an_unknown_until_phase() {
        // Arrange: the error must not wait for the evidence to arrive.
        let mut nodes = vec![node("tested", Mode::Bind, "checked in CI", &[])];
        nodes[0].deadline = Some(Deadline {
            until: "FM".to_owned(),
            promote: None,
        });
        let view = view(nodes);

        // Act
        let error = evaluate(&view, &phases("M1"), &[], &EvidenceSet::default())
            .expect_err("FM is not a phase");

        // Assert
        assert_eq!(
            error,
            EvalError::UnknownUntilPhase {
                id: "tested".to_owned(),
                until: "FM".to_owned(),
            }
        );
    }

    #[test]
    fn an_unknown_until_phase_fails_loudly() {
        // Arrange
        let mut nodes = vec![node("leaf", Mode::Fiat, "taken as given", &[])];
        nodes[0].deadline = Some(Deadline {
            until: "FM".to_owned(),
            promote: None,
        });
        let view = view(nodes);

        // Act
        let error = evaluate(&view, &phases("M1"), &[], &EvidenceSet::default())
            .expect_err("FM is not a phase");

        // Assert
        assert_eq!(
            error,
            EvalError::UnknownUntilPhase {
                id: "leaf".to_owned(),
                until: "FM".to_owned(),
            }
        );
    }

    #[test]
    fn an_unknown_current_phase_fails_loudly() {
        // Arrange
        let section = PhasesSection {
            order: vec!["M1".to_owned()],
            current: "M9".to_owned(),
        };

        // Act
        let error = evaluate(&chain(), &section, &[], &EvidenceSet::default())
            .expect_err("M9 is not a phase");

        // Assert
        assert_eq!(
            error,
            EvalError::CurrentPhaseNotInOrder {
                current: "M9".to_owned(),
            }
        );
    }

    #[test]
    fn a_cyclic_view_is_an_input_error_not_a_verdict() {
        // Arrange: a hand-built cycle; a checked project cannot contain one.
        let view = view(vec![
            node("a", Mode::Structure, "a", &["b"]),
            node("b", Mode::Structure, "b", &["a"]),
        ]);

        // Act & Assert
        assert_eq!(
            evaluate(&view, &phases("M1"), &[], &EvidenceSet::default())
                .expect_err("a cycle cannot be evaluated"),
            EvalError::Cycle
        );
    }

    #[test]
    fn satisfied_status_returns_the_approvals_that_count() {
        // Arrange
        let view = chain();
        let leaf_hash = hash_of(&view, "leaf");
        let records = vec![
            approve(Mode::Fiat, "leaf", leaf_hash, "iwata"),
            approve(Mode::Fiat, "leaf", leaf_hash, "tanaka"),
        ];
        let order = PhaseOrder::new(&phases("M1")).expect("a valid section");
        let leaf = view.node("leaf").expect("the fixture defines leaf");

        // Act
        let status =
            approval_status(&records, leaf, leaf_hash, &order).expect("the status resolves");

        // Assert: the shared validity function hands the queue every record
        // that counts, in record order.
        let ApprovalStatus::Satisfied { approvals } = status else {
            panic!("expected Satisfied, got {status:?}");
        };
        let approvers: Vec<&str> = approvals
            .iter()
            .map(|approval| approval.approver())
            .collect();
        assert_eq!(approvers, ["iwata", "tanaka"]);
    }

    #[test]
    fn stale_approvers_are_sorted_and_deduplicated() {
        // Arrange: the same approver endorsed two earlier versions.
        let view = chain();
        let old_one = LocalView::fiat("first wording", None).view_hash();
        let old_two = LocalView::fiat("second wording", None).view_hash();
        let records = vec![
            approve(Mode::Fiat, "leaf", old_one, "tanaka"),
            approve(Mode::Fiat, "leaf", old_two, "iwata"),
            approve(Mode::Fiat, "leaf", old_two, "tanaka"),
        ];
        let order = PhaseOrder::new(&phases("M1")).expect("a valid section");
        let leaf = view.node("leaf").expect("the fixture defines leaf");

        // Act
        let status = approval_status(&records, leaf, hash_of(&view, "leaf"), &order)
            .expect("the status resolves");

        // Assert
        assert_eq!(
            status,
            ApprovalStatus::Stale {
                approvers: vec!["iwata".to_owned(), "tanaka".to_owned()],
            }
        );
    }
}
