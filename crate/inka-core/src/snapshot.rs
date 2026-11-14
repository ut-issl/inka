//! Approval snapshots: what a reviewer was shown when a calc bind was
//! approved, kept so the next review can say what changed.
//!
//! A bind approval is bound to a hash, and a hash says only *that* something
//! changed. For a calc bind the interesting change is often the assumption
//! set — a node added upstream, a wording edited — so on approval `inka
//! review` writes `approvals/snapshots/<hex>.json`, `<hex>` being the
//! approved view hash's digest, with the target path, the assumption set
//! (each node's id and rendered description) and the definitions (path,
//! kind and source). The file is content-addressed and stored in a canonical
//! form — definitions sorted by path, sources with the line endings the hash
//! reads — so two approvers approving the same view write the same bytes
//! however the evidence was delivered, and nothing is appended. A node's id
//! and a definition's kind are outside the hash; for those the first
//! approval's spelling stands.
//!
//! At the next review the current assumption set is diffed against the
//! baseline: the snapshot of the most recently approved view hash of the
//! same bind — the latest `approve` record for it whose snapshot exists.
//! Snapshots are presentation aids and nothing more. Evaluation never reads
//! them, so a missing, deleted or damaged snapshot changes what a reviewer
//! is shown, never what is established. A file is written whole or not at
//! all (a temporary file renamed into place), and a destination that is not
//! a snapshot is replaced by the next approval of the same view.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use crate::calc_evidence::{CalcTarget, DefinitionKind};
use crate::hash::ViewHash;
use crate::normalize;
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
    /// The definitions of the target's upstream subgraph, sorted bytewise
    /// by path, each source with CRLF read as LF.
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
    /// The snapshot of a calc bind: `target` is the imported evidence the
    /// bind's reference names, `assumed` the nodes it assumes.
    ///
    /// Definitions are stored the way the interface hash reads them — sorted
    /// bytewise by path, CRLF as LF — so deliveries the hash cannot tell
    /// apart snapshot to the same bytes.
    #[must_use]
    pub fn of(path: &str, target: &CalcTarget, assumed: &[&ViewNode]) -> Self {
        let mut definitions: Vec<Definition> = target
            .definitions
            .iter()
            .map(|definition| Definition {
                path: definition.path.clone(),
                kind: definition.kind,
                source: normalize::evidence_body(&definition.source).into_owned(),
            })
            .collect();
        definitions.sort_unstable_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
        Self {
            v: VERSION,
            target: path.to_owned(),
            assumptions: assumed.iter().map(|node| Assumption::of(node)).collect(),
            definitions,
        }
    }

    /// The file a view hash's snapshot lives in: the digest's hex, `.json`.
    #[must_use]
    pub fn file_name(hash: ViewHash) -> String {
        let digest = hash.to_string();
        let hex = digest.strip_prefix("sha256:").unwrap_or(&digest);
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
/// Content-addressed, so a destination that already holds a snapshot is left
/// as it is: the second approver of the same view finds the first one's file
/// and writes nothing. A destination that cannot be read as a snapshot — a
/// truncated or otherwise damaged file — is replaced, which is how damage
/// heals. One a newer build wrote ([`SnapshotError::UnsupportedVersion`]) is
/// not damage and is left alone. Returns whether a file was written.
///
/// The file appears whole or not at all: the text goes to a temporary file
/// in the same directory, which is then renamed into place.
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
    match read(snapshots_dir, hash) {
        Ok(Some(_)) | Err(SnapshotError::UnsupportedVersion { .. }) => return Ok(false),
        // Absent, or present and not a snapshot: either way this view has
        // no snapshot yet, and writing one is what puts that right.
        Ok(None) | Err(_) => {}
    }
    fs::create_dir_all(snapshots_dir).map_err(|source| SnapshotError::Write {
        path: snapshots_dir.to_owned(),
        source,
    })?;
    let mut text = serde_json::to_string_pretty(snapshot).expect("a snapshot serializes to JSON");
    text.push('\n');
    let name = Snapshot::file_name(hash);
    let path = snapshots_dir.join(&name);
    let temporary = snapshots_dir.join(temporary_name(&name));
    let renamed = write_new(&temporary, &text).and_then(|()| fs::rename(&temporary, &path));
    if let Err(source) = renamed {
        // Best effort: the write already failed, and that is what is reported.
        let _ = fs::remove_file(&temporary);
        return Err(SnapshotError::Write { path, source });
    }
    Ok(true)
}

/// A name no other writer is using: this process's id and a counter, so two
/// processes — or two threads — writing the same snapshot never share a
/// temporary file. Dot-prefixed, and not `.json`, so it is never mistaken
/// for a snapshot.
fn temporary_name(name: &str) -> String {
    static WRITES: AtomicU64 = AtomicU64::new(0);
    let write = WRITES.fetch_add(1, Ordering::Relaxed);
    format!(".{name}.{}.{write}.tmp", std::process::id())
}

/// Writes `text` to `path` and flushes it to the device, so the rename that
/// follows never publishes a file whose content is still in flight.
fn write_new(path: &Path, text: &str) -> io::Result<()> {
    let mut file = File::create(path)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()
}

/// The one field every version of the format carries.
#[derive(Deserialize)]
struct Versioned {
    v: u64,
}

/// Reads the snapshot of `hash`, or `None` when none was written.
///
/// # Errors
///
/// Returns [`SnapshotError::Read`] when the file exists but cannot be read,
/// [`SnapshotError::UnsupportedVersion`] when it declares another version —
/// whatever else it holds — and [`SnapshotError::Malformed`] when it is not
/// a snapshot.
pub fn read(snapshots_dir: &Path, hash: ViewHash) -> Result<Option<Snapshot>, SnapshotError> {
    let path = snapshots_dir.join(Snapshot::file_name(hash));
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(source) => return Err(SnapshotError::Read { path, source }),
    };
    // The version is read on its own first: another version need not have
    // this one's fields, and the strict parse below would call it malformed.
    if let Ok(Versioned { v: found }) = serde_json::from_str(&text)
        && found != VERSION
    {
        return Err(SnapshotError::UnsupportedVersion { path, found });
    }
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|source| SnapshotError::Malformed { path, source })
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
/// Returns a [`SnapshotError`] when the snapshot of the latest approval that
/// has one exists but cannot be read as one. An older readable snapshot is
/// not offered in its place: it would be a baseline the reviewer did not
/// last approve against.
pub fn baseline<'r>(
    snapshots_dir: &Path,
    records: &'r [Record],
    node: &ViewNode,
) -> Result<Option<Baseline<'r>>, SnapshotError> {
    let mut approvals: Vec<(Instant<'_>, usize, &Record)> = records
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

/// The length of `YYYY-MM-DDThh:mm:ss`, the fixed-width part of a timestamp.
const WHOLE_SECONDS_LEN: usize = 19;

/// A record's instant as a sort key: the whole seconds, then the fraction's
/// significant digits.
///
/// A stored timestamp has one spelling — `YYYY-MM-DDThh:mm:ss(.digits)?Z`,
/// every field fixed-width and always UTC — so comparing the pieces bytewise
/// is comparing the instants. Nothing is converted, so no accepted spelling
/// can fail to order: not a fraction finer than a nanosecond, not an instant
/// at the far end of the calendar.
type Instant<'r> = (&'r str, &'r str);

fn instant(record: &Record) -> Instant<'_> {
    let text = record.timestamp().as_str();
    let (seconds, rest) = text
        .split_at_checked(WHOLE_SECONDS_LEN)
        .unwrap_or((text, ""));
    // `.5`, `.50` and no fraction against `.0` are the same instants:
    // trailing zeros carry nothing, and without them digit strings order as
    // the fractions they spell.
    let fraction = rest
        .trim_start_matches('.')
        .trim_end_matches('Z')
        .trim_end_matches('0');
    (seconds, fraction)
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

    use crate::calc_evidence::DefinitionSource;
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

    /// A calc target delivering `definitions` in the given order.
    fn calc_target(definitions: &[(&str, DefinitionKind, &str)]) -> CalcTarget {
        let hashed: Vec<crate::calc_evidence::Definition<'_>> = definitions
            .iter()
            .map(|&(path, _, source)| crate::calc_evidence::Definition { path, source })
            .collect();
        CalcTarget {
            definitions: definitions
                .iter()
                .map(|&(path, kind, source)| DefinitionSource {
                    path: path.to_owned(),
                    kind,
                    source: source.to_owned(),
                })
                .collect(),
            assumes: Vec::new(),
            leaves: Vec::new(),
            valid: true,
            xfail: false,
            interface_hash: crate::calc_evidence::interface_hash(TARGET, &hashed, &[]),
        }
    }

    const TARGET: &str = "Power::?verify_margin";

    fn snapshot_text(snapshots: &Path, hash: ViewHash) -> String {
        fs::read_to_string(snapshots.join(Snapshot::file_name(hash))).expect("reads")
    }

    #[test]
    fn a_snapshot_stores_definitions_sorted_by_path_with_lf_line_endings() {
        // Arrange: delivered budget-first, with CRLF line endings.
        let target = calc_target(&[
            (
                "Power::@budget",
                DefinitionKind::Calculation,
                "def budget():\r\n    ...\r\n",
            ),
            (TARGET, DefinitionKind::Verification, "def verify(): ...\n"),
        ]);

        // Act
        let snapshot = Snapshot::of(TARGET, &target, &[]);

        // Assert: the order and the normalization the interface hash uses.
        assert_eq!(
            snapshot.definitions,
            [
                Definition {
                    path: TARGET.to_owned(),
                    kind: DefinitionKind::Verification,
                    source: "def verify(): ...\n".to_owned(),
                },
                Definition {
                    path: "Power::@budget".to_owned(),
                    kind: DefinitionKind::Calculation,
                    source: "def budget():\n    ...\n".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn deliveries_the_hash_cannot_tell_apart_write_identical_bytes() {
        // Arrange: the same view delivered in another definition order and
        // with CRLF line endings — the interface hash is the same.
        let lf = calc_target(&[
            (TARGET, DefinitionKind::Verification, "def verify(): ...\n"),
            (
                "Power::@budget",
                DefinitionKind::Calculation,
                "def budget():\n    ...\n",
            ),
        ]);
        let crlf = calc_target(&[
            (
                "Power::@budget",
                DefinitionKind::Calculation,
                "def budget():\r\n    ...\r\n",
            ),
            (
                TARGET,
                DefinitionKind::Verification,
                "def verify(): ...\r\n",
            ),
        ]);
        assert_eq!(lf.interface_hash, crlf.interface_hash);
        let (first, second) = (
            tempfile::tempdir().expect("a temp dir"),
            tempfile::tempdir().expect("a temp dir"),
        );
        let hash = view_hash("same view");

        // Act
        write(first.path(), hash, &Snapshot::of(TARGET, &lf, &[])).expect("writes");
        write(second.path(), hash, &Snapshot::of(TARGET, &crlf, &[])).expect("writes");

        // Assert
        assert_eq!(
            snapshot_text(first.path(), hash),
            snapshot_text(second.path(), hash)
        );
    }

    #[test]
    fn a_newer_version_is_unsupported_whatever_fields_it_carries() {
        // Arrange: a v2 file shaped unlike v1 — a field this build does not
        // know, and one it requires missing.
        let dir = tempfile::tempdir().expect("a temp dir");
        let hash = view_hash("v2");
        fs::write(
            dir.path().join(Snapshot::file_name(hash)),
            r#"{"v": 2, "target": "t", "assumptions": [], "sources": {}}"#,
        )
        .expect("writes");

        // Act
        let read = read(dir.path(), hash);

        // Assert
        assert!(
            matches!(
                read,
                Err(SnapshotError::UnsupportedVersion { found: 2, .. })
            ),
            "{read:?}"
        );
    }

    #[test]
    fn a_write_replaces_a_destination_that_is_not_a_snapshot() {
        // Arrange: an interrupted write left half a file.
        let dir = tempfile::tempdir().expect("a temp dir");
        let hash = view_hash("truncated");
        let path = dir.path().join(Snapshot::file_name(hash));
        fs::write(&path, "{\n  \"v\": 1,\n  \"target\": \"Pow").expect("writes");
        let fresh = snapshot(vec![assumption("a", "the a model holds")]);

        // Act
        let written = write(dir.path(), hash, &fresh).expect("writes");

        // Assert: the next approval repaired it.
        assert!(written);
        assert_eq!(read(dir.path(), hash).expect("reads"), Some(fresh));
    }

    #[test]
    fn a_write_leaves_a_newer_version_destination_alone() {
        // Arrange: a newer build already recorded this view.
        let dir = tempfile::tempdir().expect("a temp dir");
        let hash = view_hash("newer");
        let newer = r#"{"v": 2, "target": "t", "sources": {}}"#;
        fs::write(dir.path().join(Snapshot::file_name(hash)), newer).expect("writes");

        // Act
        let written = write(dir.path(), hash, &snapshot(vec![])).expect("writes nothing");

        // Assert
        assert!(!written);
        assert_eq!(snapshot_text(dir.path(), hash), newer);
    }

    #[test]
    fn a_write_leaves_nothing_but_the_snapshot_in_the_directory() {
        // Arrange
        let dir = tempfile::tempdir().expect("a temp dir");
        let snapshots = snapshots_dir(dir.path());
        let hash = view_hash("atomic");

        // Act: a fresh write, then a repair of a corrupt destination.
        write(&snapshots, hash, &snapshot(vec![])).expect("writes");
        fs::write(snapshots.join(Snapshot::file_name(hash)), "").expect("truncates");
        write(&snapshots, hash, &snapshot(vec![])).expect("rewrites");

        // Assert: the temporary file was renamed into place, not left over.
        let names: Vec<String> = fs::read_dir(&snapshots)
            .expect("lists")
            .map(|entry| {
                entry
                    .expect("an entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(names, [Snapshot::file_name(hash)]);
    }

    #[test]
    fn a_write_into_a_directory_that_cannot_exist_is_an_error() {
        // Arrange: the snapshots directory's name is taken by a file.
        let dir = tempfile::tempdir().expect("a temp dir");
        let snapshots = snapshots_dir(dir.path());
        fs::write(&snapshots, "not a directory").expect("writes");

        // Act
        let written = write(&snapshots, view_hash("blocked"), &snapshot(vec![]));

        // Assert
        assert!(
            matches!(written, Err(SnapshotError::Write { .. })),
            "{written:?}"
        );
        assert_eq!(
            fs::read_to_string(&snapshots).expect("reads"),
            "not a directory"
        );
    }

    #[test]
    fn the_baseline_orders_every_accepted_timestamp_without_panicking() {
        // Arrange: spellings a record accepts but jiff cannot represent —
        // more than nine fractional digits, and an instant past its range —
        // next to ordinary ones.
        let dir = tempfile::tempdir().expect("a temp dir");
        let hashes: Vec<ViewHash> = ["a", "b", "c", "d", "e"]
            .into_iter()
            .map(view_hash)
            .collect();
        for (hash, name) in hashes.iter().zip(["a", "b", "c", "d", "e"]) {
            write(dir.path(), *hash, &snapshot(vec![assumption("a", name)])).expect("writes");
        }
        let timestamps = [
            "2026-11-10T02:55:10.2Z",
            "2026-11-10T02:55:10.1234567890123Z",
            "2026-11-10T02:55:10.1234567890124Z",
            "2026-11-10T02:55:10.123456789Z",
            "2026-11-10T02:55:10Z",
        ];
        let mut records: Vec<Record> = hashes
            .iter()
            .zip(timestamps)
            .map(|(hash, timestamp)| approve("margin_ok", *hash, timestamp))
            .collect();
        let latest_of = |records: &[Record]| {
            baseline(dir.path(), records, &bind("margin_ok"))
                .expect("reads")
                .expect("a baseline exists")
                .snapshot
                .assumptions[0]
                .description
                .clone()
        };

        // Act & Assert: the order is the instants' own, digit for digit, as
        // one record after another is taken away.
        assert_eq!(latest_of(&records), "a");
        records.remove(0);
        assert_eq!(latest_of(&records), "c");
        records.remove(1);
        assert_eq!(latest_of(&records), "b");
        records.remove(0);
        assert_eq!(latest_of(&records), "d");

        // An instant at the end of the calendar is simply the latest.
        records.push(approve("margin_ok", hashes[4], "9999-12-31T23:59:59Z"));
        assert_eq!(latest_of(&records), "e");
    }

    #[test]
    fn equal_instants_spelled_differently_fall_back_to_load_order() {
        // Arrange: `.5`, `.50` and `.500` name one instant.
        let dir = tempfile::tempdir().expect("a temp dir");
        let (first, second, third) = (view_hash("1"), view_hash("2"), view_hash("3"));
        for (hash, name) in [(first, "first"), (second, "second"), (third, "third")] {
            write(dir.path(), hash, &snapshot(vec![assumption("a", name)])).expect("writes");
        }
        let records = [
            approve("margin_ok", first, "2026-11-10T02:55:10.50Z"),
            approve("margin_ok", second, "2026-11-10T02:55:10.500Z"),
            approve("margin_ok", third, "2026-11-10T02:55:10.5Z"),
        ];

        // Act
        let found = baseline(dir.path(), &records, &bind("margin_ok"))
            .expect("reads")
            .expect("a baseline exists");

        // Assert: the last one loaded wins the tie.
        assert_eq!(found.record.view_hash(), third);
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
