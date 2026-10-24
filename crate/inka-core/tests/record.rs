//! Approval-record storage against real files, and the frozen v1 record
//! vectors.
//!
//! The storage half exercises `approvals::append` / `load_file` / `load_dir`
//! in a throwaway directory: append-only growth, loud failure with file and
//! line on anything malformed, and the empty case a fresh project presents.
//!
//! The golden half replays `tests/golden/record/v1/`. Each vector freezes one
//! stored line and the canonical signing bytes it reduces to; the directory is
//! append-only, guarded in CI, and a failure here is never fixed by updating
//! the file — either the change under test is wrong, or the layout moves on
//! as a parallel v2.

use std::fs;
use std::path::{Path, PathBuf};

use inka_core::approvals::{self, StoreError};
use inka_core::record::{Label, Record, RecordFields, Target};
use inka_core::view::Mode;
use serde::Deserialize;

mod common;
use common::golden_dir;

const VIEW_HASH: &str = "sha256:59481b66866b8ca5f21be852caa2f692a757841d5f58221fa189b2e12a6b42a5";

/// A throwaway approvals directory, removed on drop.
struct TempApprovals {
    dir: PathBuf,
}

impl TempApprovals {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("inka-approvals-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        Self { dir }
    }
}

impl Drop for TempApprovals {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn fields(approver: &str) -> RecordFields {
    RecordFields {
        target: Target::new(Mode::Fiat, "mass_margin").expect("a valid target"),
        view_hash: VIEW_HASH.parse().expect("a valid digest"),
        approver: approver.to_owned(),
        key_id: format!("{approver}-2026"),
        timestamp: "2026-09-10T08:30:00Z".parse().expect("a valid timestamp"),
        phase: "M3".to_owned(),
        until: None,
        policy_hash: None,
    }
}

fn approval(approver: &str) -> Record {
    Record::approve(fields(approver), None).expect("a valid record")
}

#[test]
fn appends_one_line_per_record_to_the_approvers_file() {
    // Arrange
    let temp = TempApprovals::new("append");
    let first = approval("iwata");
    let second = Record::decline(fields("iwata"), "the margin math is wrong".to_owned())
        .expect("a valid record");

    // Act
    approvals::append(&temp.dir, &first).expect("the first append creates the file");
    approvals::append(&temp.dir, &second).expect("the second append grows it");

    // Assert: append-only growth, in order, one line each.
    let text = fs::read_to_string(temp.dir.join("iwata.jsonl")).expect("the file exists");
    assert_eq!(text.lines().count(), 2);
    let loaded = approvals::load_file(&temp.dir.join("iwata.jsonl")).expect("the file loads");
    assert_eq!(loaded, vec![first, second]);
}

#[test]
fn appending_after_an_unterminated_record_preserves_both_records() {
    // Arrange: a valid last record need not have a final newline to load.
    let temp = TempApprovals::new("unterminated");
    let first = approval("iwata");
    let second = Record::decline(fields("iwata"), "the margin math is wrong".to_owned())
        .expect("a valid record");
    fs::create_dir_all(&temp.dir).expect("the directory creates");
    let path = temp.dir.join("iwata.jsonl");
    let original = serde_json::to_string(&first).expect("a record serializes");
    fs::write(&path, &original).expect("the unterminated record writes");
    assert_eq!(approvals::load_file(&path).unwrap(), vec![first.clone()]);

    // Act
    approvals::append(&temp.dir, &second).expect("the second record appends");

    // Assert: existing bytes stay intact, with a separator before the new record.
    let text = fs::read_to_string(&path).expect("the file exists");
    assert!(text.starts_with(&format!("{original}\n")));
    assert_eq!(text.lines().count(), 2);
    assert_eq!(approvals::load_file(&path).unwrap(), vec![first, second]);
}

#[test]
fn appending_to_an_empty_file_does_not_add_a_blank_line() {
    // Arrange
    let temp = TempApprovals::new("empty-file");
    fs::create_dir_all(&temp.dir).expect("the directory creates");
    let path = temp.dir.join("iwata.jsonl");
    fs::write(&path, "").expect("the empty file writes");
    assert_eq!(approvals::load_file(&path).unwrap(), []);
    let record = approval("iwata");

    // Act
    approvals::append(&temp.dir, &record).expect("the record appends");

    // Assert
    assert_eq!(approvals::load_file(&path).unwrap(), vec![record]);
}

#[test]
fn invalid_utf8_fails_with_the_file_and_record_line() {
    for (name, ending) in [("invalid-utf8-lf", "\n"), ("invalid-utf8-crlf", "\r\n")] {
        // Arrange: the second record contains an invalid UTF-8 byte.
        let temp = TempApprovals::new(name);
        fs::create_dir_all(&temp.dir).expect("the directory creates");
        let path = temp.dir.join("iwata.jsonl");
        let line = serde_json::to_string(&approval("iwata")).expect("a record serializes");
        let mut bytes = format!("{line}{ending}").into_bytes();
        bytes.extend_from_slice(b"{\"comment\":\"\xff\"}");
        fs::write(&path, bytes).expect("the malformed record writes");

        // Act
        let error = approvals::load_file(&path).expect_err("invalid UTF-8 is fatal");

        // Assert
        assert!(
            matches!(&error, StoreError::MalformedLine { path: p, line: 2, .. } if p == &path),
            "unexpected error: {error}"
        );
        assert!(error.to_string().contains("utf-8"), "{error}");
    }
}

#[test]
fn loads_every_approvers_file_in_sorted_order() {
    // Arrange
    let temp = TempApprovals::new("load-dir");
    approvals::append(&temp.dir, &approval("suzuki")).expect("suzuki's record appends");
    approvals::append(&temp.dir, &approval("iwata")).expect("iwata's record appends");
    // A stray non-jsonl entry is not a record file.
    fs::write(temp.dir.join("README.md"), "not a record").expect("the stray file writes");

    // Act
    let loaded = approvals::load_dir(&temp.dir).expect("the directory loads");

    // Assert
    let approvers: Vec<&str> = loaded.iter().map(Record::approver).collect();
    assert_eq!(approvers, ["iwata", "suzuki"]);
}

#[test]
fn a_project_with_no_approvals_directory_has_no_records() {
    let temp = TempApprovals::new("missing-dir");

    let loaded = approvals::load_dir(&temp.dir).expect("a missing directory is empty");

    assert_eq!(loaded, []);
}

#[test]
fn a_malformed_line_fails_loudly_with_file_and_line() {
    // Arrange: a valid first line, then a truncated second one.
    let temp = TempApprovals::new("malformed");
    approvals::append(&temp.dir, &approval("iwata")).expect("the valid record appends");
    let path = temp.dir.join("iwata.jsonl");
    let mut text = fs::read_to_string(&path).expect("the file exists");
    text.push_str("{\"v\":1,\"target\":\n");
    fs::write(&path, text).expect("the corruption writes");

    // Act
    let error = approvals::load_file(&path).expect_err("the malformed line is fatal");

    // Assert
    assert!(
        matches!(&error, StoreError::MalformedLine { path: p, line: 2, .. } if p == &path),
        "unexpected error: {error}"
    );
}

#[test]
fn an_unknown_field_fails_loudly_with_file_and_line() {
    // Arrange
    let temp = TempApprovals::new("unknown-field");
    approvals::append(&temp.dir, &approval("iwata")).expect("the valid record appends");
    let path = temp.dir.join("iwata.jsonl");
    let text = fs::read_to_string(&path).expect("the file exists");
    let extended = text.replace(",\"sig\":\"\"}", ",\"sig\":\"\",\"mood\":\"ok\"}");
    fs::write(&path, extended).expect("the extended line writes");

    // Act
    let error = approvals::load_file(&path).expect_err("an unknown field is fatal");

    // Assert
    let message = error.to_string();
    assert!(
        message.contains("iwata.jsonl") && message.contains(":1:"),
        "{message}"
    );
    assert!(message.contains("mood"), "{message}");
}

#[test]
fn a_blank_line_is_not_a_record() {
    // Arrange: a blank line slipped between two valid records.
    let temp = TempApprovals::new("blank-line");
    approvals::append(&temp.dir, &approval("iwata")).expect("the valid record appends");
    let path = temp.dir.join("iwata.jsonl");
    let text = fs::read_to_string(&path).expect("the file exists");
    fs::write(&path, format!("{text}\n{text}")).expect("the blank line writes");

    // Act
    let error = approvals::load_file(&path).expect_err("a blank line is fatal");

    // Assert
    assert!(
        matches!(error, StoreError::MalformedLine { line: 2, .. }),
        "unexpected error: {error}"
    );
}

#[test]
fn a_record_in_the_wrong_approvers_file_fails_loudly() {
    // Arrange: suzuki's record hand-copied into iwata's file.
    let temp = TempApprovals::new("wrong-file");
    approvals::append(&temp.dir, &approval("suzuki")).expect("suzuki's record appends");
    let line = fs::read_to_string(temp.dir.join("suzuki.jsonl")).expect("the file exists");
    let path = temp.dir.join("iwata.jsonl");
    fs::write(&path, line).expect("the misfiled record writes");

    // Act
    let error = approvals::load_file(&path).expect_err("a misfiled record is fatal");

    // Assert
    assert!(
        matches!(
            &error,
            StoreError::ApproverMismatch { line: 1, found, expected, .. }
                if found == "suzuki" && expected == "iwata"
        ),
        "unexpected error: {error}"
    );
}

#[test]
fn a_decline_missing_its_comment_fails_loudly_on_load() {
    // Arrange: a decline whose comment was nulled out by hand.
    let temp = TempApprovals::new("decline-comment");
    let record = Record::decline(fields("iwata"), "the margin math is wrong".to_owned())
        .expect("a valid record");
    approvals::append(&temp.dir, &record).expect("the valid record appends");
    let path = temp.dir.join("iwata.jsonl");
    let text = fs::read_to_string(&path).expect("the file exists");
    fs::write(&path, text.replace("\"the margin math is wrong\"", "null"))
        .expect("the stripped comment writes");

    // Act
    let error = approvals::load_file(&path).expect_err("a comment-less decline is fatal");

    // Assert
    assert!(error.to_string().contains("decline"), "{error}");
}

/// One frozen vector: a stored line and the canonical signing bytes it
/// reduces to.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Vector {
    /// One stored jsonl line, exactly as `approvals::append` writes it.
    line: String,
    /// The canonical signing bytes: sorted-key compact JSON minus `sig`.
    canonical: String,
}

/// Every `*.toml` under the record vector directory, in sorted path order.
fn vector_files() -> Vec<PathBuf> {
    let dir = golden_dir().join("record/v1");
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("{} should be readable: {error}", dir.display()))
        .map(|entry| entry.expect("a directory entry should be readable").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "toml")
        })
        .collect();
    files.sort();
    files
}

fn read_vector(path: &Path) -> Vector {
    let text = fs::read_to_string(path)
        .unwrap_or_else(|error| panic!("{} should be readable: {error}", path.display()));
    toml::from_str(&text)
        .unwrap_or_else(|error| panic!("{} should be a valid vector: {error}", path.display()))
}

#[test]
fn every_recorded_vector_still_loads_and_reduces_to_its_recorded_bytes() {
    let files = vector_files();
    assert!(
        !files.is_empty(),
        "the v1 record layout should be frozen by vectors"
    );

    for path in &files {
        // Arrange
        let vector = read_vector(path);
        let name = path.display();

        // Act
        let record: Record = serde_json::from_str(&vector.line)
            .unwrap_or_else(|error| panic!("{name}: the frozen line should load: {error}"));

        // Assert: the reader, the writer, and the signing bytes all stand still.
        assert_eq!(
            serde_json::to_string(&record).expect("a record serializes"),
            vector.line,
            "{name}: the stored spelling is frozen — a parallel v2 is how it changes"
        );
        assert_eq!(
            String::from_utf8(record.canonical_bytes()).expect("JSON is UTF-8"),
            vector.canonical,
            "{name}: the canonical signing bytes are frozen — a parallel v2 is how they change"
        );
    }
}

#[test]
fn the_vector_set_covers_both_labels() {
    // Arrange & Act: what each recorded vector exercises.
    let mut approve = false;
    let mut decline = false;
    for path in vector_files() {
        let record: Record =
            serde_json::from_str(&read_vector(&path).line).expect("a frozen line should load");
        match record.label() {
            Label::Approve => approve = true,
            Label::Decline => decline = true,
        }
    }

    // Assert: a label with no vector has an unfrozen layout.
    assert!(
        approve && decline,
        "both labels need at least one vector (approve, decline)"
    );
}
