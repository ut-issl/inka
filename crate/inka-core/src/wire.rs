//! The data the Web UI receives: one [`Page`] per request, defined here and
//! nowhere else.
//!
//! The front end draws; it does not decide. Everything that carries meaning —
//! which nodes exist and in what order, who their parents are, whether each
//! is established and why not, what the review queue holds, what a node's
//! local view shows and which hash a decision on it would name — is computed
//! here from the same passes `inka eval` and `inka review` run, and arrives
//! ready to present. The front end's TypeScript types are generated from
//! these definitions (see the `generated` test below), so the two sides
//! cannot drift.
//!
//! A page is one consistent reading of the repository: the argument, the
//! evaluation, the queue and every local view come from a single load, so a
//! hash shown beside a description is the hash of that description.
//!
//! The shape is independent of any graph library. `inka export` keeps writing
//! cytoscape `elements` for external tools; this is not that.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::approvals::{self, APPROVALS_DIR};
use crate::argument::{Argument, Rejected, load_argument};
use crate::eval::{BlameReason, EvalError, evaluate};
use crate::hash::ViewHash;
use crate::local::LocalContent;
use crate::record::{Record, Timestamp};
use crate::review::{
    self, BlockedReason, DecideError, Decision, QueueState, ReviewQueue, Reviewer, current_hash,
    derive_queue,
};
use crate::source::{DiagnosticRef, Diagnostics, Error, SourceMap, Warning};
use crate::view::{ArgumentView, Mode, ViewNode};

/// What one request for a project answers with.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Page {
    /// The project presents: its argument, evaluated, with its review queue.
    Argument(Snapshot),
    /// The project has no presentable argument; these are the reasons.
    Rejected(Rejection),
}

/// One consistent reading of a project that passes the check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Snapshot {
    /// How many nodes are established, as `inka eval` counts them.
    pub established: u32,
    /// How many nodes the argument has.
    pub total: u32,
    /// Every node, in sorted block-ID order.
    pub nodes: Vec<NodeData>,
    /// The objectives the argument hangs from, in sorted block-ID order.
    pub roots: Vec<String>,
    /// Every node id with parents before children; among nodes that are
    /// ready together, sorted block-ID order decides.
    pub order: Vec<String>,
    /// The nodes grouped by namespace: groups in the order their first member
    /// appears in [`Snapshot::order`], members in that order too.
    pub namespaces: Vec<NamespaceGroup>,
    pub queue: QueueData,
    /// Suspicions raised while loading; the argument stands despite them.
    pub warnings: Vec<Finding>,
}

/// One node with everything any surface shows of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct NodeData {
    pub id: String,
    pub mode: Mode,
    /// Until namespace resolution lands, the stem of the node's argument file.
    pub namespace: String,
    /// Tags without their leading `#`, in declaration order.
    pub tags: Vec<String>,
    /// Whether the node is one of [`Snapshot::roots`].
    pub root: bool,
    /// Direct parents, in sorted block-ID order.
    pub parents: Vec<String>,
    /// Direct children, in the order the block declares them.
    pub children: Vec<String>,
    /// How many distinct nodes lie below this one.
    pub below: u32,
    /// How many of those are blamed.
    pub blamed_below: u32,
    pub status: Status,
    /// The node's place in the review queue; `None` when it needs no review.
    pub review: Option<ReviewState>,
    /// The hash a decision on this node recorded now would name; `None` when
    /// there is none to name (see [`ReviewState::Blocked`]).
    #[cfg_attr(test, ts(as = "Option<String>"))]
    pub view_hash: Option<ViewHash>,
    /// The node's local view: its description and everything else an
    /// approval of it binds to.
    pub local: LocalContent,
}

/// The verdict `inka eval` gives a node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Status {
    Established,
    /// Not established through a cause of its own.
    Blamed {
        reasons: Vec<BlameReason>,
    },
    /// Not established only because something below it is not.
    Unestablished,
}

/// Why a node is in the review queue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ReviewState {
    /// No record targets the node.
    Unapproved,
    /// Approvals exist, but only for earlier content.
    Stale { approvers: Vec<String> },
    /// A decline vetoes the current content; shown for information.
    Declined { by: Vec<String> },
    /// No decision can be recorded until something else changes.
    Blocked { reason: BlockedReason },
}

/// The four states the queue is grouped by, in the order it lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "snake_case")]
pub enum ReviewGroup {
    Unapproved,
    Stale,
    Declined,
    Blocked,
}

impl ReviewState {
    /// The group the state is listed under.
    #[must_use]
    pub const fn group(&self) -> ReviewGroup {
        match self {
            ReviewState::Unapproved => ReviewGroup::Unapproved,
            ReviewState::Stale { .. } => ReviewGroup::Stale,
            ReviewState::Declined { .. } => ReviewGroup::Declined,
            ReviewState::Blocked { .. } => ReviewGroup::Blocked,
        }
    }
}

/// The review queue as `inka review` presents it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct QueueData {
    /// The targets a session walks — unapproved and stale — from the leaves
    /// up, as the interactive `inka review` visits them.
    pub walk: Vec<String>,
    /// What `inka review --list` prints: one section per state that has
    /// targets, each from the leaves up.
    pub groups: Vec<QueueGroup>,
}

/// One section of the queue.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct QueueGroup {
    pub state: ReviewGroup,
    pub targets: Vec<String>,
}

/// The nodes of one namespace.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct NamespaceGroup {
    pub name: String,
    pub nodes: Vec<String>,
}

/// Why a project has no presentable argument.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Rejection {
    /// Every finding of the run, in source order; at least one is an error.
    pub findings: Vec<Finding>,
}

/// One finding about the project's sources.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Finding {
    pub severity: Severity,
    pub message: String,
    /// Where the finding points; `None` for a judgement on the project as a
    /// whole.
    pub location: Option<Location>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

/// The start of the source range a finding is about.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Location {
    /// The file, as the command line names it.
    pub path: String,
    /// One-based line.
    pub line: u32,
    /// One-based column, counted in bytes.
    pub column: u32,
}

/// Who the server records decisions as, if it records at all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Session {
    /// `None` when the server was started without an approver: it then offers
    /// no way to record, and the Web UI is read-only throughout.
    pub reviewer: Option<ReviewerSession>,
}

/// The identity a recording server was started with. The page shows it and
/// cannot choose or change it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ReviewerSession {
    pub approver: String,
    pub key_id: String,
    /// Proof that a request comes from a page this server served for this
    /// run; sent back with every decision.
    pub token: String,
}

/// One decision, as a page asks for it to be recorded.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct DecisionRequest {
    /// The id of the node the decision is on.
    pub target: String,
    /// The view hash of the local view that was on screen. The decision is
    /// recorded only if this is still the target's current hash.
    pub view_hash: String,
    pub decision: DecisionData,
}

/// The judgement itself.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "label", rename_all = "snake_case")]
pub enum DecisionData {
    /// An approval, with an optional note.
    Approve { comment: Option<String> },
    /// A veto; it must say why.
    Decline { comment: String },
}

/// What became of a decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum DecisionResponse {
    /// One record was appended.
    Recorded,
    /// The target's content is no longer what was on screen. Nothing was
    /// written; the page should show the current content.
    Changed,
    /// The decision cannot be recorded as asked — an unknown target, a target
    /// with no hash to name, a decline without a reason. Nothing was written.
    Refused { message: String },
    /// The decision was sound but the record could not be stored.
    Failed { message: String },
}

/// Records the decision a page asked for, as `inka review` would: through the
/// same [`review::decide`], into the same file.
///
/// The project is read afresh, and the decision is recorded only if the hash
/// the request names is the target's hash in that reading — so an edit made
/// while the page was open is never approved unseen. `approver`, `key_id`
/// and `timestamp` are the caller's: a request cannot name them.
#[must_use]
pub fn decide(
    project_dir: &Path,
    approver: &str,
    key_id: Option<&str>,
    request: DecisionRequest,
    timestamp: Timestamp,
) -> DecisionResponse {
    let refused = |message: String| DecisionResponse::Refused { message };
    let Ok(expected) = request.view_hash.parse::<ViewHash>() else {
        return refused(format!("`{}` is not a view hash", request.view_hash));
    };
    let Ok(argument) = load_argument(project_dir) else {
        return refused("the project does not check; nothing can be decided on it".to_owned());
    };
    let approvals_dir = project_dir.join(APPROVALS_DIR);
    let reviewer = Reviewer {
        approvals_dir: &approvals_dir,
        approver,
        key_id,
        phase: &argument.manifest.phases.current,
    };
    let decision = match request.decision {
        DecisionData::Approve { comment } => Decision::Approve { comment },
        DecisionData::Decline { comment } => Decision::Decline { comment },
    };
    match review::decide(
        &reviewer,
        &argument.view,
        &argument.evidence,
        &request.target,
        Some(expected),
        decision,
        timestamp,
    ) {
        Ok(_) => DecisionResponse::Recorded,
        Err(DecideError::Changed { .. }) => DecisionResponse::Changed,
        Err(DecideError::Store(error)) => DecisionResponse::Failed {
            message: error.to_string(),
        },
        Err(error) => refused(error.to_string()),
    }
}

/// Reads the project rooted at `project_dir` and answers with its page.
///
/// Never fails: whatever keeps the argument from being presented — a source
/// that does not check, an unreadable approvals store, a phase the manifest
/// does not order — is the content of a [`Page::Rejected`].
#[must_use]
pub fn page(project_dir: &Path) -> Page {
    let argument = match load_argument(project_dir) {
        Ok(argument) => argument,
        Err(Rejected {
            source_map,
            diagnostics,
            ..
        }) => {
            return Page::Rejected(Rejection {
                findings: findings(&source_map, &diagnostics),
            });
        }
    };
    let records = match approvals::load_dir(&project_dir.join(APPROVALS_DIR)) {
        Ok(records) => records,
        Err(error) => return rejected(&argument, &error.to_string()),
    };
    match snapshot(&argument, &records) {
        Ok(snapshot) => Page::Argument(snapshot),
        Err(error) => rejected(&argument, &error.to_string()),
    }
}

/// The rejection of a project that loaded but cannot be evaluated: the one
/// error that stopped it, after every warning the load had already raised —
/// a rejection lists every finding of the run, not only the last.
fn rejected(argument: &Argument, message: &str) -> Page {
    let diagnostics = Diagnostics {
        errors: vec![Error::unanchored(message)],
        warnings: argument.warnings.clone(),
    };
    Page::Rejected(Rejection {
        findings: findings(&argument.source_map, &diagnostics),
    })
}

/// Evaluates `argument` against `records` and arranges the result for
/// presentation.
///
/// # Errors
///
/// Returns the [`EvalError`] evaluation or the queue reports; a checked
/// project only trips the phase errors.
pub fn snapshot(argument: &Argument, records: &[Record]) -> Result<Snapshot, EvalError> {
    let Argument {
        manifest,
        evidence,
        view,
        source_map,
        warnings,
    } = argument;
    let evaluation = evaluate(view, &manifest.phases, records, evidence)?;
    let queue = derive_queue(view, &manifest.phases, records, evidence)?;

    let blame: HashMap<&str, &[BlameReason]> = evaluation
        .blame
        .iter()
        .map(|blame| (blame.node.as_str(), blame.reasons.as_slice()))
        .collect();
    let review = review_states(&queue);
    let relations = Relations::of(view);
    let roots: HashSet<&str> = view.roots.iter().map(String::as_str).collect();

    let nodes = view
        .nodes
        .iter()
        .zip(&evaluation.nodes)
        .enumerate()
        .map(|(index, (node, verdict))| {
            let status = if verdict.established {
                Status::Established
            } else {
                match blame.get(node.id.as_str()) {
                    Some(reasons) => Status::Blamed {
                        reasons: reasons.to_vec(),
                    },
                    None => Status::Unestablished,
                }
            };
            let below = relations.below(index);
            let blamed_below = below
                .iter()
                .filter(|&&below| blame.contains_key(view.nodes[below].id.as_str()))
                .count();
            NodeData {
                id: node.id.clone(),
                mode: node.mode,
                namespace: node.namespace.clone(),
                tags: node.tags.clone(),
                root: roots.contains(node.id.as_str()),
                parents: relations.parents[index]
                    .iter()
                    .map(|&parent| view.nodes[parent].id.clone())
                    .collect(),
                children: node.children.clone(),
                below: count(below.len()),
                blamed_below: count(blamed_below),
                status,
                review: review.get(node.id.as_str()).cloned(),
                view_hash: current_hash(view, node, evidence).ok(),
                local: LocalContent::of(view, node),
            }
        })
        .collect();

    let order = relations.topological_order();
    Ok(Snapshot {
        established: count(evaluation.established_count()),
        total: count(evaluation.total()),
        nodes,
        roots: view.roots.clone(),
        namespaces: namespaces(view, &order),
        order: order
            .iter()
            .map(|&index| view.nodes[index].id.clone())
            .collect(),
        queue: queue_data(&queue),
        warnings: warning_findings(source_map, warnings),
    })
}

/// A count as the page carries it. An argument is nowhere near `u32::MAX`
/// nodes; one that were would saturate rather than wrap.
fn count(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(u32::MAX)
}

/// Each queued node's state, by id.
fn review_states(queue: &ReviewQueue) -> HashMap<&str, ReviewState> {
    let items = queue.items.iter().map(|item| {
        let state = match &item.state {
            QueueState::Missing => ReviewState::Unapproved,
            QueueState::Stale { approvers } => ReviewState::Stale {
                approvers: approvers.clone(),
            },
            QueueState::Declined { by } => ReviewState::Declined { by: by.clone() },
        };
        (item.id.as_str(), state)
    });
    let blocked = queue.blocked.iter().map(|item| {
        (
            item.id.as_str(),
            ReviewState::Blocked {
                reason: item.reason.clone(),
            },
        )
    });
    items.chain(blocked).collect()
}

fn queue_data(queue: &ReviewQueue) -> QueueData {
    let of = |group: ReviewGroup| -> Vec<String> {
        queue
            .items
            .iter()
            .filter(|item| {
                group
                    == match item.state {
                        QueueState::Missing => ReviewGroup::Unapproved,
                        QueueState::Stale { .. } => ReviewGroup::Stale,
                        QueueState::Declined { .. } => ReviewGroup::Declined,
                    }
            })
            .map(|item| item.id.clone())
            .collect()
    };
    let groups = [
        (ReviewGroup::Unapproved, of(ReviewGroup::Unapproved)),
        (ReviewGroup::Stale, of(ReviewGroup::Stale)),
        (ReviewGroup::Declined, of(ReviewGroup::Declined)),
        (
            ReviewGroup::Blocked,
            queue.blocked.iter().map(|item| item.id.clone()).collect(),
        ),
    ]
    .into_iter()
    .filter(|(_, targets)| !targets.is_empty())
    .map(|(state, targets)| QueueGroup { state, targets })
    .collect();
    let walk = queue
        .items
        .iter()
        .filter(|item| !matches!(item.state, QueueState::Declined { .. }))
        .map(|item| item.id.clone())
        .collect();
    QueueData { walk, groups }
}

/// The edges of a view by node index — a node's index being its position in
/// sorted block-ID order.
struct Relations {
    /// Each node's direct parents, ascending.
    parents: Vec<Vec<usize>>,
    /// Each node's direct children, in declaration order.
    children: Vec<Vec<usize>>,
}

impl Relations {
    /// A child the view does not define is left out, as every pass over a
    /// hand-built view leaves it out; a checked project has none.
    fn of(view: &ArgumentView) -> Self {
        let index_of: HashMap<&str, usize> = view
            .nodes
            .iter()
            .enumerate()
            .map(|(index, node)| (node.id.as_str(), index))
            .collect();
        let children: Vec<Vec<usize>> = view
            .nodes
            .iter()
            .map(|node: &ViewNode| {
                node.children
                    .iter()
                    .filter_map(|child| index_of.get(child.as_str()).copied())
                    .collect()
            })
            .collect();
        let mut parents = vec![Vec::new(); children.len()];
        for (parent, below) in children.iter().enumerate() {
            for &child in below {
                parents[child].push(parent);
            }
        }
        Self { parents, children }
    }

    /// Every node below `start`, each once.
    fn below(&self, start: usize) -> HashSet<usize> {
        let mut seen = HashSet::new();
        let mut pending = self.children[start].clone();
        while let Some(node) = pending.pop() {
            if seen.insert(node) {
                pending.extend(&self.children[node]);
            }
        }
        seen.remove(&start);
        seen
    }

    /// Parents before children; among the nodes ready at once, the lowest
    /// index first. Nodes on a cycle — which a checked project cannot have —
    /// never become ready and are left out.
    fn topological_order(&self) -> Vec<usize> {
        let mut waiting: Vec<usize> = self.parents.iter().map(Vec::len).collect();
        let mut ready: BinaryHeap<Reverse<usize>> = waiting
            .iter()
            .enumerate()
            .filter(|&(_, &parents)| parents == 0)
            .map(|(index, _)| Reverse(index))
            .collect();
        let mut order = Vec::with_capacity(waiting.len());
        while let Some(Reverse(node)) = ready.pop() {
            order.push(node);
            for &child in &self.children[node] {
                waiting[child] -= 1;
                if waiting[child] == 0 {
                    ready.push(Reverse(child));
                }
            }
        }
        order
    }
}

fn namespaces(view: &ArgumentView, order: &[usize]) -> Vec<NamespaceGroup> {
    let mut groups: Vec<NamespaceGroup> = Vec::new();
    let mut position: HashMap<&str, usize> = HashMap::new();
    for &index in order {
        let node = &view.nodes[index];
        let group = *position.entry(node.namespace.as_str()).or_insert_with(|| {
            groups.push(NamespaceGroup {
                name: node.namespace.clone(),
                nodes: Vec::new(),
            });
            groups.len() - 1
        });
        groups[group].nodes.push(node.id.clone());
    }
    groups
}

fn warning_findings(source_map: &SourceMap, warnings: &[Warning]) -> Vec<Finding> {
    let diagnostics = Diagnostics {
        errors: Vec::new(),
        warnings: warnings.to_vec(),
    };
    findings(source_map, &diagnostics)
}

/// Every finding in source order, each with where it points.
fn findings(source_map: &SourceMap, diagnostics: &Diagnostics) -> Vec<Finding> {
    diagnostics
        .in_source_order()
        .into_iter()
        .map(|diagnostic| Finding {
            severity: match diagnostic {
                DiagnosticRef::Error(_) => Severity::Error,
                DiagnosticRef::Warning(_) => Severity::Warning,
            },
            message: diagnostic.message().to_owned(),
            location: diagnostic.span().and_then(|span| {
                let file = source_map.file(span.file_id)?;
                let (line, column) = file.line_col(span.start);
                Some(Location {
                    path: file.path.display().to_string(),
                    line: count(line + 1),
                    column: count(column + 1),
                })
            }),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;

    use ts_rs::TS;

    use crate::review::decide as record;

    const MANIFEST: &str = "[project]\nname = \"t\"\n\n[phases]\norder = [\"M1\"]\ncurrent = \"M1\"\n\n[values]\nfiles = []\n";

    fn block(keyword: &str, id: &str, children: &[&str]) -> String {
        let children = if children.is_empty() {
            String::new()
        } else {
            format!("    children:\n        {}\n", children.join(",\n        "))
        };
        format!("{keyword} {id} {{\n    ```\n    {id} holds\n    ```\n{children}}}\n\n")
    }

    /// A diamond with a spare leaf, split over two files:
    ///
    /// ```text
    /// root ─┬─ left ──┐
    ///       └─ right ─┴─ shared
    ///            └─ lonely
    /// ```
    fn diamond() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("a temporary directory");
        fs::write(dir.path().join("inka.toml"), MANIFEST).expect("the manifest is written");
        fs::create_dir(dir.path().join("argument")).expect("the argument directory is created");
        fs::write(
            dir.path().join("argument/a.inka"),
            block("objective", "root", &["left", "right"])
                + &block("structure", "left", &["shared"]),
        )
        .expect("the first file is written");
        fs::write(
            dir.path().join("argument/b.inka"),
            block("structure", "right", &["shared", "lonely"])
                + &block("fiat", "shared", &[])
                + &block("fiat", "lonely", &[]),
        )
        .expect("the second file is written");
        dir
    }

    fn approve(dir: &Path, targets: &[&str]) {
        let argument = load_argument(dir).expect("the project loads");
        let approvals_dir = dir.join(APPROVALS_DIR);
        let reviewer = Reviewer {
            approvals_dir: &approvals_dir,
            approver: "iwata",
            key_id: None,
            phase: "M1",
        };
        for target in targets {
            record(
                &reviewer,
                &argument.view,
                &argument.evidence,
                target,
                None,
                Decision::Approve { comment: None },
                "2026-09-10T08:30:00Z".parse().expect("a valid timestamp"),
            )
            .expect("the approval records");
        }
    }

    fn snapshot_of(dir: &Path) -> Snapshot {
        match page(dir) {
            Page::Argument(snapshot) => snapshot,
            Page::Rejected(rejection) => panic!("the project should present: {rejection:?}"),
        }
    }

    fn node<'s>(snapshot: &'s Snapshot, id: &str) -> &'s NodeData {
        snapshot
            .nodes
            .iter()
            .find(|node| node.id == id)
            .expect("the snapshot has the node")
    }

    #[test]
    fn nodes_come_in_sorted_id_order_with_their_relations() {
        // Arrange
        let dir = diamond();

        // Act
        let snapshot = snapshot_of(dir.path());

        // Assert
        let ids: Vec<&str> = snapshot.nodes.iter().map(|node| node.id.as_str()).collect();
        assert_eq!(ids, ["left", "lonely", "right", "root", "shared"]);
        assert_eq!(snapshot.roots, ["root"]);
        assert!(node(&snapshot, "root").root);
        assert!(!node(&snapshot, "left").root);
        assert_eq!(node(&snapshot, "shared").parents, ["left", "right"]);
        assert_eq!(node(&snapshot, "right").children, ["shared", "lonely"]);
    }

    #[test]
    fn the_order_puts_parents_first_and_breaks_ties_by_id() {
        // Arrange
        let dir = diamond();

        // Act
        let snapshot = snapshot_of(dir.path());

        // Assert: `shared` waits for both parents, so `lonely` overtakes it.
        assert_eq!(
            snapshot.order,
            ["root", "left", "right", "lonely", "shared"]
        );
        assert_eq!(
            snapshot.namespaces,
            [
                NamespaceGroup {
                    name: "a".to_owned(),
                    nodes: vec!["root".to_owned(), "left".to_owned()],
                },
                NamespaceGroup {
                    name: "b".to_owned(),
                    nodes: vec!["right".to_owned(), "lonely".to_owned(), "shared".to_owned()],
                },
            ]
        );
    }

    #[test]
    fn a_node_shared_by_two_parents_is_counted_below_once() {
        // Arrange
        let dir = diamond();

        // Act
        let snapshot = snapshot_of(dir.path());

        // Assert
        assert_eq!(node(&snapshot, "root").below, 4);
        assert_eq!(node(&snapshot, "right").below, 2);
        assert_eq!(node(&snapshot, "shared").below, 0);
    }

    #[test]
    fn status_tells_blamed_from_unestablished_only_through_children() {
        // Arrange: everything approved but `left`.
        let dir = diamond();
        approve(dir.path(), &["root", "right", "shared", "lonely"]);

        // Act
        let snapshot = snapshot_of(dir.path());

        // Assert
        assert_eq!((snapshot.established, snapshot.total), (3, 5));
        assert_eq!(node(&snapshot, "right").status, Status::Established);
        assert_eq!(
            node(&snapshot, "left").status,
            Status::Blamed {
                reasons: vec![BlameReason::MissingApproval],
            }
        );
        assert_eq!(node(&snapshot, "root").status, Status::Unestablished);
        assert_eq!(node(&snapshot, "root").blamed_below, 1);
        assert_eq!(node(&snapshot, "right").blamed_below, 0);
    }

    #[test]
    fn the_queue_groups_by_state_and_walks_what_can_take_a_decision() {
        // Arrange: `shared` is declined, the leaves-first rest is unapproved.
        let dir = diamond();
        let argument = load_argument(dir.path()).expect("the project loads");
        let approvals_dir = dir.path().join(APPROVALS_DIR);
        record(
            &Reviewer {
                approvals_dir: &approvals_dir,
                approver: "iwata",
                key_id: None,
                phase: "M1",
            },
            &argument.view,
            &argument.evidence,
            "shared",
            None,
            Decision::Decline {
                comment: "not yet".to_owned(),
            },
            "2026-09-10T08:30:00Z".parse().expect("a valid timestamp"),
        )
        .expect("the decline records");

        // Act
        let snapshot = snapshot_of(dir.path());

        // Assert
        assert_eq!(snapshot.queue.walk, ["left", "lonely", "right", "root"]);
        assert_eq!(
            snapshot.queue.groups,
            [
                QueueGroup {
                    state: ReviewGroup::Unapproved,
                    targets: vec![
                        "left".to_owned(),
                        "lonely".to_owned(),
                        "right".to_owned(),
                        "root".to_owned(),
                    ],
                },
                QueueGroup {
                    state: ReviewGroup::Declined,
                    targets: vec!["shared".to_owned()],
                },
            ]
        );
        assert_eq!(
            node(&snapshot, "shared").review,
            Some(ReviewState::Declined {
                by: vec!["iwata".to_owned()],
            })
        );
        assert_eq!(
            node(&snapshot, "left").review,
            Some(ReviewState::Unapproved)
        );
    }

    #[test]
    fn a_node_carries_its_local_view_and_the_hash_a_decision_would_name() {
        // Arrange
        let dir = diamond();
        let argument = load_argument(dir.path()).expect("the project loads");

        // Act
        let snapshot = snapshot_of(dir.path());

        // Assert
        let right = node(&snapshot, "right");
        let view_node = argument.view.node("right").expect("the view has the node");
        assert_eq!(right.local, LocalContent::of(&argument.view, view_node));
        assert_eq!(
            right.view_hash,
            current_hash(&argument.view, view_node, &argument.evidence).ok()
        );
        assert!(right.view_hash.is_some());
    }

    #[test]
    fn an_approved_node_leaves_the_queue_and_keeps_its_hash() {
        // Arrange
        let dir = diamond();
        approve(dir.path(), &["lonely"]);

        // Act
        let snapshot = snapshot_of(dir.path());

        // Assert
        let lonely = node(&snapshot, "lonely");
        assert_eq!(lonely.review, None);
        assert!(lonely.view_hash.is_some());
        assert!(!snapshot.queue.walk.contains(&"lonely".to_owned()));
    }

    #[test]
    fn a_source_that_does_not_parse_is_rejected_with_where_it_points() {
        // Arrange
        let dir = diamond();
        fs::write(dir.path().join("argument/b.inka"), "fiat broken {\n")
            .expect("the file is written");

        // Act
        let page = page(dir.path());

        // Assert
        let Page::Rejected(rejection) = page else {
            panic!("a broken source presents nothing");
        };
        let finding = rejection.findings.first().expect("a finding");
        assert_eq!(finding.severity, Severity::Error);
        let location = finding
            .location
            .as_ref()
            .expect("a syntax error is anchored");
        assert!(location.path.ends_with("b.inka"));
        assert!(location.line >= 1 && location.column >= 1);
    }

    #[test]
    fn a_missing_project_and_a_broken_approvals_store_are_rejected_unanchored() {
        // Arrange
        let empty = tempfile::tempdir().expect("a temporary directory");
        let broken = diamond();
        fs::create_dir(broken.path().join(APPROVALS_DIR)).expect("the store is created");
        fs::write(
            broken.path().join(APPROVALS_DIR).join("iwata.jsonl"),
            "not json\n",
        )
        .expect("the record file is written");

        for dir in [empty.path(), broken.path()] {
            // Act
            let page = page(dir);

            // Assert
            let Page::Rejected(rejection) = page else {
                panic!("nothing presents");
            };
            assert_eq!(rejection.findings.len(), 1);
            assert_eq!(rejection.findings[0].severity, Severity::Error);
            assert_eq!(rejection.findings[0].location, None);
        }
    }

    #[test]
    fn a_rejection_after_loading_keeps_the_warnings_the_load_raised() {
        // Arrange: a node no objective reaches, which only warns, and an
        // approvals store that cannot be read, which stops the page.
        let dir = diamond();
        let stray = dir.path().join("argument/c.inka");
        fs::write(&stray, block("fiat", "stray", &[])).expect("the stray node is written");
        fs::create_dir(dir.path().join(APPROVALS_DIR)).expect("the store is created");
        fs::write(
            dir.path().join(APPROVALS_DIR).join("iwata.jsonl"),
            "not json\n",
        )
        .expect("the record file is written");

        // Act
        let page = page(dir.path());

        // Assert: the anchored warning first, then the unanchored error.
        let Page::Rejected(rejection) = page else {
            panic!("an unreadable store presents nothing");
        };
        let severities: Vec<Severity> = rejection
            .findings
            .iter()
            .map(|finding| finding.severity)
            .collect();
        assert_eq!(severities, [Severity::Warning, Severity::Error]);
        assert!(rejection.findings[0].message.contains("stray"));
        let location = rejection.findings[0]
            .location
            .as_ref()
            .expect("the warning points at the node");
        assert!(location.path.ends_with("c.inka"));
        assert_eq!(rejection.findings[1].location, None);
    }

    #[test]
    fn a_warning_does_not_keep_the_argument_from_presenting() {
        // Arrange
        let dir = diamond();
        fs::write(
            dir.path().join("argument/c.inka"),
            block("fiat", "stray", &[]),
        )
        .expect("the stray node is written");

        // Act
        let snapshot = snapshot_of(dir.path());

        // Assert
        assert_eq!(snapshot.warnings.len(), 1);
        assert_eq!(snapshot.warnings[0].severity, Severity::Warning);
        assert!(snapshot.nodes.iter().any(|node| node.id == "stray"));
    }

    #[test]
    fn an_approval_of_earlier_content_reads_as_stale_and_stays_in_the_walk() {
        // Arrange: approve the leaf, then reword it.
        let dir = diamond();
        approve(dir.path(), &["lonely"]);
        let file = dir.path().join("argument/b.inka");
        let reworded = fs::read_to_string(&file)
            .expect("the file reads")
            .replace("lonely holds", "lonely holds, reworded");
        fs::write(&file, reworded).expect("the file is rewritten");

        // Act
        let snapshot = snapshot_of(dir.path());

        // Assert
        let lonely = node(&snapshot, "lonely");
        assert_eq!(
            lonely.review,
            Some(ReviewState::Stale {
                approvers: vec!["iwata".to_owned()],
            })
        );
        assert_eq!(
            lonely.status,
            Status::Blamed {
                reasons: vec![BlameReason::StaleApproval {
                    approvers: vec!["iwata".to_owned()],
                }],
            }
        );
        assert!(snapshot.queue.walk.contains(&"lonely".to_owned()));
        assert!(snapshot.queue.groups.contains(&QueueGroup {
            state: ReviewGroup::Stale,
            targets: vec!["lonely".to_owned()],
        }));
    }

    #[test]
    fn a_node_past_its_deadline_is_blocked_out_of_the_walk_and_keeps_its_hash() {
        // Arrange: the current phase is past the leaf's `until:`.
        let dir = tempfile::tempdir().expect("a temporary directory");
        fs::write(
            dir.path().join("inka.toml"),
            MANIFEST
                .replace("[\"M1\"]", "[\"M1\", \"M2\"]")
                .replace("current = \"M1\"", "current = \"M2\""),
        )
        .expect("the manifest is written");
        fs::create_dir(dir.path().join("argument")).expect("the argument directory is created");
        fs::write(
            dir.path().join("argument/a.inka"),
            block("objective", "root", &["debt"])
                + "fiat debt {\n    ```\n    debt holds\n    ```\n    until: M1\n}\n",
        )
        .expect("the argument is written");

        // Act
        let snapshot = snapshot_of(dir.path());

        // Assert
        let debt = node(&snapshot, "debt");
        assert_eq!(
            debt.review,
            Some(ReviewState::Blocked {
                reason: BlockedReason::UntilExpired {
                    until: "M1".to_owned(),
                },
            })
        );
        assert!(debt.view_hash.is_some());
        assert_eq!(snapshot.queue.walk, ["root"]);
        assert!(snapshot.queue.groups.contains(&QueueGroup {
            state: ReviewGroup::Blocked,
            targets: vec!["debt".to_owned()],
        }));
    }

    #[test]
    fn a_page_serializes_under_its_kind() {
        // Arrange
        let empty = tempfile::tempdir().expect("a temporary directory");

        // Act
        let json = serde_json::to_value(page(empty.path())).expect("a page serializes");

        // Assert
        assert_eq!(json["kind"], "rejected");
        assert_eq!(json["findings"][0]["severity"], "error");
    }

    fn request(target: &str, view_hash: &str, decision: DecisionData) -> DecisionRequest {
        DecisionRequest {
            target: target.to_owned(),
            view_hash: view_hash.to_owned(),
            decision,
        }
    }

    fn stamp() -> Timestamp {
        "2026-09-10T08:30:00Z".parse().expect("a valid timestamp")
    }

    fn hash_on_screen(dir: &Path, id: &str) -> String {
        let hash = node(&snapshot_of(dir), id)
            .view_hash
            .expect("the node can be decided on");
        serde_json::to_value(hash)
            .expect("a hash serializes")
            .as_str()
            .expect("a hash is a string")
            .to_owned()
    }

    #[test]
    fn a_decision_naming_the_hash_on_screen_is_recorded_as_the_server_s_reviewer() {
        // Arrange
        let dir = diamond();
        let shown = hash_on_screen(dir.path(), "lonely");

        // Act
        let response = decide(
            dir.path(),
            "iwata",
            None,
            request("lonely", &shown, DecisionData::Approve { comment: None }),
            stamp(),
        );

        // Assert
        assert_eq!(response, DecisionResponse::Recorded);
        let records =
            approvals::load_dir(&dir.path().join(APPROVALS_DIR)).expect("the store reads");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].approver(), "iwata");
        assert_eq!(records[0].key_id(), "iwata");
        assert_eq!(node(&snapshot_of(dir.path()), "lonely").review, None);
    }

    #[test]
    fn a_decision_on_content_edited_since_it_was_shown_writes_nothing() {
        // Arrange: the page showed the leaf, then the leaf was reworded.
        let dir = diamond();
        let shown = hash_on_screen(dir.path(), "lonely");
        let file = dir.path().join("argument/b.inka");
        let reworded = fs::read_to_string(&file)
            .expect("the file reads")
            .replace("lonely holds", "lonely holds, reworded");
        fs::write(&file, reworded).expect("the file is rewritten");

        // Act
        let response = decide(
            dir.path(),
            "iwata",
            None,
            request("lonely", &shown, DecisionData::Approve { comment: None }),
            stamp(),
        );

        // Assert
        assert_eq!(response, DecisionResponse::Changed);
        assert!(!dir.path().join(APPROVALS_DIR).exists());
    }

    #[test]
    fn a_decision_that_cannot_be_recorded_as_asked_is_refused_with_the_reason() {
        // Arrange
        let dir = diamond();
        let shown = hash_on_screen(dir.path(), "lonely");
        let approve = || DecisionData::Approve { comment: None };
        let refusal =
            |request: DecisionRequest| match decide(dir.path(), "iwata", None, request, stamp()) {
                DecisionResponse::Refused { message } => message,
                other => panic!("expected a refusal, got {other:?}"),
            };

        // Act & Assert
        assert_eq!(
            refusal(request("absent", &shown, approve())),
            "no block named `absent` in the argument"
        );
        assert!(refusal(request("lonely", "sha256:nope", approve())).contains("not a view hash"));
        assert!(
            refusal(request(
                "lonely",
                &shown,
                DecisionData::Decline {
                    comment: "  ".to_owned()
                }
            ))
            .contains("comment")
        );
        assert!(!dir.path().join(APPROVALS_DIR).exists());
    }

    #[test]
    fn a_request_is_read_from_the_json_a_page_sends() {
        // Arrange
        let json = r#"{"target":"leaf","view_hash":"sha256:00","decision":{"label":"decline","comment":"not yet"}}"#;

        // Act
        let parsed: DecisionRequest = serde_json::from_str(json).expect("the request parses");

        // Assert
        assert_eq!(
            parsed,
            request(
                "leaf",
                "sha256:00",
                DecisionData::Decline {
                    comment: "not yet".to_owned()
                }
            )
        );
    }

    /// Where the front end keeps the types generated from this module.
    fn generated_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/src/generated")
    }

    fn typescript_files(dir: &Path) -> BTreeMap<String, String> {
        let Ok(entries) = fs::read_dir(dir) else {
            return BTreeMap::new();
        };
        entries
            .map(|entry| entry.expect("a directory entry").path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "ts"))
            .map(|path| {
                let name = path
                    .file_name()
                    .expect("a file has a name")
                    .to_string_lossy()
                    .into_owned();
                (
                    name,
                    fs::read_to_string(&path).expect("a generated file reads"),
                )
            })
            .collect()
    }

    /// The TypeScript the front end compiles against is what these Rust types
    /// generate. Regenerate with `INKA_UPDATE_GENERATED=1 cargo test -p
    /// inka-core`.
    #[test]
    fn generated() {
        // Arrange
        let fresh_dir = tempfile::tempdir().expect("a temporary directory");
        let config = ts_rs::Config::new()
            .with_out_dir(fresh_dir.path())
            .with_large_int("number");

        // Act
        Page::export_all(&config).expect("the page types export");
        Session::export_all(&config).expect("the session types export");
        DecisionRequest::export_all(&config).expect("the request types export");
        DecisionResponse::export_all(&config).expect("the response types export");
        // The generator leaves spaces at line ends; the repository's hooks
        // would strip them and put the files out of date.
        let fresh: BTreeMap<String, String> = typescript_files(fresh_dir.path())
            .into_iter()
            .map(|(name, text)| {
                let lines: Vec<&str> = text.lines().map(str::trim_end).collect();
                (name, lines.join("\n") + "\n")
            })
            .collect();

        // Assert
        if std::env::var_os("INKA_UPDATE_GENERATED").is_some() {
            let dir = generated_dir();
            for stale in typescript_files(&dir).keys() {
                fs::remove_file(dir.join(stale)).expect("a stale file is removed");
            }
            fs::create_dir_all(&dir).expect("the directory is created");
            for (name, text) in &fresh {
                fs::write(dir.join(name), text).expect("a generated file is written");
            }
        }
        assert!(
            fresh == typescript_files(&generated_dir()),
            "web/src/generated is out of date; run `INKA_UPDATE_GENERATED=1 cargo test -p inka-core`"
        );
    }
}
