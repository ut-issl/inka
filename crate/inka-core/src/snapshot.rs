//! Approval snapshots: what a reviewer was shown when a calc bind was
//! approved, kept so the next review can say what changed.
//!
//! A bind approval is bound to a hash, and a hash says only *that* something
//! changed. For a calc bind the interesting change is often the assumption
//! set — a node added upstream, a wording edited — so on approval `inka
//! review` writes `approvals/snapshots/<hex>.json`, `<hex>` being the
//! approved view hash's digest, with the target path, the assumption set
//! (each node's id and rendered description) and the definitions (path,
//! kind and source) as presented. The file is content-addressed: two
//! approvers approving the same view write the same bytes, and nothing is
//! appended.
//!
//! At the next review the current assumption set is diffed against the
//! baseline: the snapshot of the most recently approved view hash of the
//! same bind — the latest `approve` record for it whose snapshot exists.
//! Snapshots are presentation aids and nothing more. Evaluation never reads
//! them, so a missing or deleted snapshot changes what a reviewer is shown,
//! never what is established.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::calc_evidence::{CalcTarget, DefinitionKind};
use crate::hash::ViewHash;
use crate::record::{Label, Record};
use crate::view::{Mode, ViewNode};

/// The directory snapshots live under, relative to the approvals directory.
pub const SNAPSHOTS_DIR: &str = "snapshots";

/// The snapshot format version this build reads and writes.
pub const VERSION: u64 = 1;

/// The snapshots directory of a project's approvals directory.
#[must_use]
pub fn snapshots_dir(approvals_dir: &Path) -> PathBuf {
    approvals_dir.join(SNAPSHOTS_DIR)
}

/// What one approval of a calc bind was shown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    /// The format version.
    pub v: u64,
    /// The calc target path the bind referenced.
    pub target: String,
    /// The assumed nodes, in sorted block-ID order.
    pub assumptions: Vec<Assumption>,
    /// The definitions of the target's upstream subgraph, as delivered.
    pub definitions: Vec<Definition>,
}

/// One assumed node: its id and the description the reviewer saw.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assumption {
    pub id: String,
    pub description: String,
}

/// One definition of the upstream subgraph, as presented.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub path: String,
    pub kind: DefinitionKind,
    pub source: String,
}

impl Snapshot {
    /// The snapshot of a calc bind as it is presented: `target` is the
    /// imported evidence the bind's reference names, `assumed` the nodes it
    /// assumes.
    #[must_use]
    pub fn of(path: &str, target: &CalcTarget, assumed: &[&ViewNode]) -> Self {
        Self {
            v: VERSION,
            target: path.to_owned(),
            assumptions: assumed.iter().map(|node| Assumption::of(node)).collect(),
            definitions: target
                .definitions
                .iter()
                .map(|definition| Definition {
                    path: definition.path.clone(),
                    kind: definition.kind,
                    source: definition.source.clone(),
                })
                .collect(),
        }
    }

    /// The file a view hash's snapshot lives in: the digest's hex, `.json`.
    #[must_use]
    pub fn file_name(hash: ViewHash) -> String {
        let digest = hash.to_string();
        let hex = digest
            .strip_prefix("sha256:")
            .expect("a view hash displays as sha256:<hex>");
        format!("{hex}.json")
    }
}

impl Assumption {
    /// The assumption a node is, as the reviewer sees it.
    #[must_use]
    pub fn of(node: &ViewNode) -> Self {
        Self {
            id: node.id.clone(),
            description: node.description.clone(),
        }
    }
}

/// A reason a snapshot could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error("failed to write snapshot {}: {source}", path.display())]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("failed to read snapshot {}: {source}", path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("snapshot {} is not a valid v{VERSION} snapshot: {source}", path.display())]
    Malformed {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("snapshot {} has version {found}; this build reads v{VERSION}", path.display())]
    UnsupportedVersion { path: PathBuf, found: u64 },
}

/// Writes the snapshot of `hash`, creating the directory as needed.
///
/// Content-addressed, so an existing file is left as it is: the second
/// approver of the same view finds the first one's snapshot and writes
/// nothing. Returns whether a file was written.
///
/// # Errors
///
/// Returns [`SnapshotError::Write`] when the directory or file cannot be
/// created or written.
///
/// # Panics
///
/// Panics if the snapshot fails to serialize, which a snapshot built from a
/// loaded target cannot do.
pub fn write(
    snapshots_dir: &Path,
    hash: ViewHash,
    snapshot: &Snapshot,
) -> Result<bool, SnapshotError> {
    let path = snapshots_dir.join(Snapshot::file_name(hash));
    if path.exists() {
        return Ok(false);
    }
    fs::create_dir_all(snapshots_dir).map_err(|source| SnapshotError::Write {
        path: snapshots_dir.to_owned(),
        source,
    })?;
    let mut text = serde_json::to_string_pretty(snapshot).expect("a snapshot serializes to JSON");
    text.push('\n');
    fs::write(&path, text).map_err(|source| SnapshotError::Write { path, source })?;
    Ok(true)
}

/// Reads the snapshot of `hash`, or `None` when none was written.
///
/// # Errors
///
/// Returns [`SnapshotError::Read`] when the file exists but cannot be read,
/// [`SnapshotError::Malformed`] when it is not a snapshot, and
/// [`SnapshotError::UnsupportedVersion`] when it declares another version.
pub fn read(snapshots_dir: &Path, hash: ViewHash) -> Result<Option<Snapshot>, SnapshotError> {
    let path = snapshots_dir.join(Snapshot::file_name(hash));
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(SnapshotError::Read { path, source }),
    };
    let snapshot: Snapshot =
        serde_json::from_str(&text).map_err(|source| SnapshotError::Malformed {
            path: path.clone(),
            source,
        })?;
    if snapshot.v != VERSION {
        return Err(SnapshotError::UnsupportedVersion {
            path,
            found: snapshot.v,
        });
    }
    Ok(Some(snapshot))
}

/// The snapshot a review diffs against, and the approval it belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Baseline<'r> {
    /// The approval whose view the snapshot records.
    pub record: &'r Record,
    pub snapshot: Snapshot,
}

/// The baseline of a bind: the snapshot of its most recently approved view
/// hash — the latest `approve` record targeting the bind whose snapshot
/// exists. Approvals without a snapshot are skipped, so a record written
/// before snapshots existed does not hide an older one that has one.
///
/// "Latest" is by the records' timestamps; two records at the same instant
/// are ordered as loaded (later file or line wins).
///
/// # Errors
///
/// Returns a [`SnapshotError`] when a snapshot file exists but cannot be
/// read as one.
pub fn baseline<'r>(
    snapshots_dir: &Path,
    records: &'r [Record],
    node: &ViewNode,
) -> Result<Option<Baseline<'r>>, SnapshotError> {
    let mut approvals: Vec<(jiff::Timestamp, usize, &Record)> = records
        .iter()
        .enumerate()
        .filter(|(_, record)| {
            record.label() == Label::Approve
                && record.target().kind() == Mode::Bind
                && record.target().id() == node.id
        })
        .map(|(index, record)| (instant(record), index, record))
        .collect();
    approvals.sort_unstable_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    for (_, _, record) in approvals {
        if let Some(snapshot) = read(snapshots_dir, record.view_hash())? {
            return Ok(Some(Baseline { record, snapshot }));
        }
    }
    Ok(None)
}

/// A record's instant; the stored spelling is validated RFC3339 UTC.
fn instant(record: &Record) -> jiff::Timestamp {
    record
        .timestamp()
        .as_str()
        .parse()
        .expect("a record timestamp is a valid RFC3339 UTC instant")
}

/// How the current assumption set differs from a baseline, by node id.
///
/// A renamed node reads as removed and added: ids are what the two sets are
/// joined on, and the hash — which never saw the id — says nothing changed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AssumptionDiff {
    /// In both sets with the same description, in id order.
    pub unchanged: Vec<Assumption>,
    /// In the current set only, in id order.
    pub added: Vec<Assumption>,
    /// In the baseline only, in id order.
    pub removed: Vec<Assumption>,
    /// In both sets with a different description, in id order.
    pub reworded: Vec<Reworded>,
}

/// One assumption whose description changed since the baseline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reworded {
    pub id: String,
    pub before: String,
    pub after: String,
}

impl AssumptionDiff {
    /// Whether the two sets are the same.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.removed.is_empty() && self.reworded.is_empty()
    }
}

/// Diffs the current assumption set against a baseline's.
#[must_use]
pub fn diff(current: &[Assumption], baseline: &[Assumption]) -> AssumptionDiff {
    let before: BTreeMap<&str, &str> = baseline
        .iter()
        .map(|assumption| (assumption.id.as_str(), assumption.description.as_str()))
        .collect();
    let after: BTreeMap<&str, &str> = current
        .iter()
        .map(|assumption| (assumption.id.as_str(), assumption.description.as_str()))
        .collect();

    let mut diff = AssumptionDiff::default();
    for (&id, &description) in &after {
        match before.get(id) {
            None => diff.added.push(assumption(id, description)),
            Some(&earlier) if earlier == description => {
                diff.unchanged.push(assumption(id, description));
            }
            Some(&earlier) => diff.reworded.push(Reworded {
                id: id.to_owned(),
                before: earlier.to_owned(),
                after: description.to_owned(),
            }),
        }
    }
    for (&id, &description) in &before {
        if !after.contains_key(id) {
            diff.removed.push(assumption(id, description));
        }
    }
    diff
}

fn assumption(id: &str, description: &str) -> Assumption {
    Assumption {
        id: id.to_owned(),
        description: description.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::hash::LocalView;
    use crate::record::{RecordFields, Target};

    fn assumption(id: &str, description: &str) -> Assumption {
        super::assumption(id, description)
    }

    fn snapshot(assumptions: Vec<Assumption>) -> Snapshot {
        Snapshot {
            v: VERSION,
            target: "Power::?verify_margin".to_owned(),
            assumptions,
            definitions: vec![Definition {
                path: "Power::?verify_margin".to_owned(),
                kind: DefinitionKind::Verification,
                source: "def verify_margin(): ...\n".to_owned(),
            }],
        }
    }

    fn view_hash(text: &str) -> ViewHash {
        LocalView::fiat(text, None).view_hash()
    }

    fn approve(id: &str, hash: ViewHash, timestamp: &str) -> Record {
        Record::approve(
            RecordFields {
                target: Target::new(Mode::Bind, id).expect("a valid target"),
                view_hash: hash,
                approver: "iwata".to_owned(),
                key_id: "iwata-2026".to_owned(),
                timestamp: timestamp.parse().expect("a valid timestamp"),
                phase: "M1".to_owned(),
                until: None,
                policy_hash: None,
            },
            None,
        )
        .expect("a valid record")
    }

    fn bind(id: &str) -> ViewNode {
        ViewNode {
            id: id.to_owned(),
            mode: Mode::Bind,
            namespace: "power".to_owned(),
            description: "the margin holds".to_owned(),
            tags: Vec::new(),
            evidence: None,
            deadline: None,
            children: Vec::new(),
            comments: crate::view::NodeComments::default(),
        }
    }

    #[test]
    fn the_file_name_is_the_digest_hex_without_the_scheme() {
        // Arrange
        let hash = view_hash("x");

        // Act
        let name = Snapshot::file_name(hash);

        // Assert
        assert_eq!(
            format!("sha256:{}", name.trim_end_matches(".json")),
            hash.to_string()
        );
        assert_eq!(name.len(), 64 + ".json".len());
    }

    #[test]
    fn a_snapshot_round_trips_and_a_second_write_is_a_no_op() {
        // Arrange
        let dir = tempfile::tempdir().expect("a temp dir");
        let snapshots = snapshots_dir(dir.path());
        let hash = view_hash("first");
        let first = snapshot(vec![assumption("a", "the a model holds")]);

        // Act
        let written = write(&snapshots, hash, &first).expect("writes");
        let again = write(&snapshots, hash, &snapshot(vec![])).expect("writes nothing");

        // Assert: the first content stands; the same view is one file.
        assert!(written);
        assert!(!again);
        assert_eq!(read(&snapshots, hash).expect("reads"), Some(first));
        assert_eq!(read(&snapshots, view_hash("other")).expect("reads"), None);
        assert_eq!(fs::read_dir(&snapshots).expect("lists").count(), 1);
    }

    #[test]
    fn the_written_file_is_pretty_json_with_a_trailing_newline() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let snapshots = snapshots_dir(dir.path());
        let hash = view_hash("pretty");

        write(&snapshots, hash, &snapshot(vec![])).expect("writes");

        let text = fs::read_to_string(snapshots.join(Snapshot::file_name(hash))).expect("reads");
        assert!(text.starts_with("{\n  \"v\": 1,\n  \"target\": "), "{text}");
        assert!(text.ends_with("}\n"), "{text}");
    }

    #[test]
    fn an_unsupported_version_and_malformed_json_are_errors_not_absence() {
        let dir = tempfile::tempdir().expect("a temp dir");
        let snapshots = snapshots_dir(dir.path());
        fs::create_dir_all(&snapshots).expect("creates");
        let versioned = view_hash("v2");
        let broken = view_hash("broken");
        fs::write(
            snapshots.join(Snapshot::file_name(versioned)),
            r#"{"v": 2, "target": "t", "assumptions": [], "definitions": []}"#,
        )
        .expect("writes");
        fs::write(snapshots.join(Snapshot::file_name(broken)), "{ not json").expect("writes");

        assert!(matches!(
            read(&snapshots, versioned),
            Err(SnapshotError::UnsupportedVersion { found: 2, .. })
        ));
        assert!(matches!(
            read(&snapshots, broken),
            Err(SnapshotError::Malformed { .. })
        ));
    }

    #[test]
    fn the_baseline_is_the_latest_approval_that_has_a_snapshot() {
        // Arrange: three approvals of the bind — the oldest and the middle
        // have snapshots, the newest does not — plus a decline and another
        // bind's approval that must be ignored.
        let dir = tempfile::tempdir().expect("a temp dir");
        let snapshots = snapshots_dir(dir.path());
        let (old, mid, new) = (view_hash("old"), view_hash("mid"), view_hash("new"));
        write(&snapshots, old, &snapshot(vec![assumption("a", "old")])).expect("writes");
        write(&snapshots, mid, &snapshot(vec![assumption("a", "mid")])).expect("writes");
        let records = vec![
            approve("margin_ok", new, "2026-11-03T00:00:00Z"),
            approve("margin_ok", old, "2026-11-01T00:00:00Z"),
            approve("other", mid, "2026-11-09T00:00:00Z"),
            approve("margin_ok", mid, "2026-11-02T00:00:00Z"),
            Record::decline(
                RecordFields {
                    target: Target::new(Mode::Bind, "margin_ok").expect("a valid target"),
                    view_hash: mid,
                    approver: "sato".to_owned(),
                    key_id: "sato-2026".to_owned(),
                    timestamp: "2026-11-08T00:00:00Z".parse().expect("a valid timestamp"),
                    phase: "M1".to_owned(),
                    until: None,
                    policy_hash: None,
                },
                "no".to_owned(),
            )
            .expect("a valid record"),
        ];

        // Act
        let found = baseline(&snapshots, &records, &bind("margin_ok"))
            .expect("reads")
            .expect("a baseline exists");

        // Assert: the newest approval has no snapshot, so the middle one is
        // the baseline — by timestamp, not by record order.
        assert_eq!(found.record.view_hash(), mid);
        assert_eq!(found.snapshot.assumptions, [assumption("a", "mid")]);
        assert_eq!(
            baseline(&snapshots, &records, &bind("unknown")).expect("reads"),
            None
        );
    }

    #[test]
    fn the_diff_sorts_added_removed_and_reworded_by_id() {
        // Arrange
        let before = [
            assumption("kept", "stays"),
            assumption("gone", "was here"),
            assumption("worded", "the old wording"),
        ];
        let after = [
            assumption("worded", "the new wording"),
            assumption("new", "arrived"),
            assumption("kept", "stays"),
        ];

        // Act
        let diff = diff(&after, &before);

        // Assert
        assert_eq!(diff.unchanged, [assumption("kept", "stays")]);
        assert_eq!(diff.added, [assumption("new", "arrived")]);
        assert_eq!(diff.removed, [assumption("gone", "was here")]);
        assert_eq!(
            diff.reworded,
            [Reworded {
                id: "worded".to_owned(),
                before: "the old wording".to_owned(),
                after: "the new wording".to_owned(),
            }]
        );
        assert!(!diff.is_empty());
        assert!(super::diff(&before, &before).is_empty());
    }
}
