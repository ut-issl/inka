//! Subprocess tests for `inka review`: queue listing, the non-interactive
//! single-target form, argument-parsing guarantees, and the non-TTY refusal.
//!
//! The scenario tests drive a small three-node project in a temporary
//! directory through the binary alone — approve everything, watch the queue
//! empty, edit a description, watch exactly the target and its direct parent
//! come back — so the queue, the records and `inka eval` are exercised as one
//! workflow, the way a reviewer uses them.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::prelude::*;

mod common;
use common::workspace_fixture;

/// A fresh `inka` command bound to the crate's built binary.
fn inka() -> Command {
    Command::cargo_bin("inka").expect("the inka binary should build")
}

/// Writes the three-node scenario project: objective root → structure branch
/// → fiat leaf.
fn scenario_project(dir: &Path) {
    fs::write(
        dir.join("inka.toml"),
        "[project]\nname = \"review-scenario\"\n\n[phases]\norder = [\"M0\", \"M1\"]\ncurrent = \"M0\"\n\n[values]\nfiles = []\n",
    )
    .expect("the manifest writes");
    fs::create_dir(dir.join("argument")).expect("the argument directory creates");
    write_argument(dir, "taken as given");
}

/// (Re)writes the argument file with `leaf_description` as the leaf contract.
fn write_argument(dir: &Path, leaf_description: &str) {
    let text = format!(
        "objective root {{\n  ```\n  the root claim\n  ```\n  children: branch\n}}\n\n\
         structure branch {{\n  ```\n  the branch claim\n  ```\n  children: leaf\n}}\n\n\
         fiat leaf {{\n  ```\n  {leaf_description}\n  ```\n}}\n"
    );
    fs::write(dir.join("argument/arg.inka"), text).expect("the argument writes");
}

/// The calc scenario's delivery: one verification, assuming the fiat.
const CALC_EVIDENCE: &str = r#"{"v": 1, "targets": [{"path": "Power::?verify_margin",
    "definitions": [{"path": "Power::?verify_margin", "kind": "verification", "source": "def verify_margin(): ...\n"}],
    "assumes": ["model_valid"], "leaves": [{"id": "Power::?verify_margin", "result": true}], "valid": true}]}"#;

/// Writes the calc scenario project: a root over one bind backed by the calc
/// evidence in `evidence`, and the fiat that evidence assumes.
fn calc_project(dir: &Path, evidence: &str) {
    fs::write(
        dir.join("inka.toml"),
        "[project]\nname = \"calc-scenario\"\n\n[phases]\norder = [\"M0\"]\ncurrent = \"M0\"\n\n\
         [values]\nfiles = []\n\n[evidence]\ncalcs = [\"calc-evidence.json\"]\n",
    )
    .expect("the manifest writes");
    fs::create_dir(dir.join("argument")).expect("the argument directory creates");
    fs::write(
        dir.join("argument/arg.inka"),
        "objective root {\n  ```\n  the root claim\n  ```\n  children: margin_ok, model_valid\n}\n\n\
         bind margin_ok {\n  ```\n  the margin holds\n  ```\n  calc: \"Power::?verify_margin\"\n}\n\n\
         fiat model_valid {\n  ```\n  the model envelopes reality\n  ```\n}\n",
    )
    .expect("the argument writes");
    fs::write(dir.join("calc-evidence.json"), evidence).expect("the evidence writes");
}

/// `inka review --path <dir> --as <approver> margin_ok`, awaiting its decision.
fn review_margin(dir: &Path, approver: &str) -> Command {
    let mut command = inka();
    command
        .arg("review")
        .arg("--path")
        .arg(dir)
        .arg("--as")
        .arg(approver)
        .arg("margin_ok");
    command
}

/// Every file in the project's snapshots directory, sorted.
fn snapshots(dir: &Path) -> Vec<PathBuf> {
    let mut paths: Vec<_> = fs::read_dir(dir.join("approvals/snapshots"))
        .expect("the snapshots directory exists")
        .map(|entry| entry.expect("an entry").path())
        .collect();
    paths.sort();
    paths
}

/// Approves the calc bind as alice and returns the one snapshot that wrote.
fn approved_snapshot(dir: &Path) -> PathBuf {
    review_margin(dir, "alice")
        .arg("--approve")
        .assert()
        .success();
    let snapshots = snapshots(dir);
    assert_eq!(snapshots.len(), 1, "{snapshots:?}");
    snapshots.into_iter().next().expect("one snapshot")
}

/// The first half of the file at `path`: what an interrupted write leaves.
fn truncate(path: &Path) {
    let text = fs::read_to_string(path).expect("the file reads");
    fs::write(path, &text[..text.len() / 2]).expect("the file truncates");
}

#[test]
fn the_dogfood_queue_matches_its_snapshot() {
    // Arrange & Act: the dogfood's one bind is approved and backed by
    // delivered evidence, so it stays out; everything else is missing.
    let output = inka()
        .arg("review")
        .arg("--path")
        .arg(workspace_fixture("dogfood"))
        .arg("--as")
        .arg("shimomura")
        .arg("--list")
        .output()
        .expect("the command runs");

    // Assert
    assert!(output.status.success());
    insta::assert_snapshot!(String::from_utf8(output.stdout).expect("the queue is UTF-8"));
}

#[test]
fn approving_every_target_empties_the_queue_and_eval_is_clean() {
    // Arrange
    let dir = tempfile::tempdir().expect("a temp dir creates");
    scenario_project(dir.path());

    // Act: approve all three targets through the non-interactive form.
    for id in ["root", "branch", "leaf"] {
        inka()
            .arg("review")
            .arg("--path")
            .arg(dir.path())
            .arg("--as")
            .arg("alice")
            .arg(id)
            .arg("--approve")
            .assert()
            .success()
            .stdout(predicate::str::contains("recorded approve of"))
            .stdout(predicate::str::contains("approvals/alice.jsonl"));
    }

    // Assert: the queue is empty and eval agrees the argument is established.
    inka()
        .arg("review")
        .arg("--path")
        .arg(dir.path())
        .arg("--as")
        .arg("alice")
        .arg("--list")
        .assert()
        .success()
        .stdout("nothing needs review\n");
    inka()
        .arg("eval")
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::starts_with("established 3/3"));
}

#[test]
fn editing_a_description_returns_the_target_and_its_direct_parent_as_stale() {
    // Arrange: a fully approved project, then the leaf's contract changes.
    let dir = tempfile::tempdir().expect("a temp dir creates");
    scenario_project(dir.path());
    for id in ["root", "branch", "leaf"] {
        inka()
            .arg("review")
            .arg("--path")
            .arg(dir.path())
            .arg("--as")
            .arg("alice")
            .arg(id)
            .arg("--approve")
            .assert()
            .success();
    }
    write_argument(dir.path(), "taken as given, but edited");

    // Act & Assert: exactly the leaf and its direct parent come back, stale;
    // the root's local view is untouched, so it stays out.
    inka()
        .arg("review")
        .arg("--path")
        .arg(dir.path())
        .arg("--as")
        .arg("alice")
        .arg("--list")
        .assert()
        .success()
        .stdout(predicate::str::contains("targets needing review: 2"))
        .stdout(predicate::str::contains(
            "structure:branch — approvals are stale: alice approved an earlier version",
        ))
        .stdout(predicate::str::contains(
            "fiat:leaf — approvals are stale: alice approved an earlier version",
        ))
        .stdout(predicate::str::contains("root").not());
}

#[test]
fn approving_a_calc_bind_writes_a_content_addressed_snapshot() {
    // Arrange: a project whose one bind is backed by loaded calc evidence
    // assuming the fiat.
    let dir = tempfile::tempdir().expect("a temp dir creates");
    calc_project(dir.path(), CALC_EVIDENCE);

    // Act: approve the bind twice, as two approvers.
    for approver in ["alice", "bob"] {
        review_margin(dir.path(), approver)
            .arg("--approve")
            .assert()
            .success()
            .stdout(predicate::str::contains("; snapshot approvals/snapshots/"))
            .stdout(predicate::str::contains(".json\n"));
    }

    // Assert: one snapshot for the one approved view, recording what was shown.
    let snapshots = snapshots(dir.path());
    assert_eq!(snapshots.len(), 1, "{snapshots:?}");
    let snapshot: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&snapshots[0]).expect("reads"))
            .expect("valid JSON");
    assert_eq!(snapshot["v"], 1);
    assert_eq!(snapshot["target"], "Power::?verify_margin");
    assert_eq!(snapshot["assumptions"][0]["id"], "model_valid");
    assert_eq!(
        snapshot["assumptions"][0]["description"],
        "the model envelopes reality"
    );
    assert_eq!(snapshot["definitions"][0]["path"], "Power::?verify_margin");
    // The records still load: the snapshots directory is not a records file.
    inka()
        .arg("eval")
        .arg(dir.path())
        .assert()
        .failure()
        .stdout(predicate::str::starts_with("established 0/3"));
}

#[test]
fn a_truncated_baseline_snapshot_warns_and_the_next_approval_repairs_it() {
    // Arrange: an approved calc bind whose snapshot was cut short.
    let dir = tempfile::tempdir().expect("a temp dir creates");
    calc_project(dir.path(), CALC_EVIDENCE);
    let snapshot = approved_snapshot(dir.path());
    let intact = fs::read(&snapshot).expect("the snapshot reads");
    truncate(&snapshot);
    let name = snapshot
        .file_name()
        .and_then(|name| name.to_str())
        .expect("a UTF-8 file name");

    // Act: the same view is approved again.
    let output = review_margin(dir.path(), "bob")
        .arg("--approve")
        .output()
        .expect("the command runs");

    // Assert: the approval goes through with one warning naming the file,
    // and the snapshot it writes replaces the damaged one.
    assert!(output.status.success(), "{output:?}");
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    assert!(stderr.starts_with("warning: snapshot "), "{stderr}");
    assert!(stderr.contains(name), "{stderr}");
    assert!(stderr.contains("reviewing without a baseline"), "{stderr}");
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    assert!(
        stdout.starts_with("recorded approve of bind:margin_ok"),
        "{stdout}"
    );
    assert_eq!(fs::read(&snapshot).expect("the snapshot reads"), intact);
    assert_eq!(snapshots(dir.path()), [snapshot]);
}

#[test]
fn a_truncated_baseline_snapshot_does_not_stop_a_decline() {
    // Arrange
    let dir = tempfile::tempdir().expect("a temp dir creates");
    calc_project(dir.path(), CALC_EVIDENCE);
    let snapshot = approved_snapshot(dir.path());
    truncate(&snapshot);
    let damaged = fs::read(&snapshot).expect("the snapshot reads");

    // Act & Assert: a decline never reads the baseline, so it has nothing to
    // warn about, and it writes no snapshot over the damaged one.
    review_margin(dir.path(), "bob")
        .arg("--decline")
        .arg("--comment")
        .arg("the margin is not shown")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "recorded decline of bind:margin_ok",
        ))
        .stdout(predicate::str::contains("snapshot").not())
        .stderr("");
    assert!(dir.path().join("approvals/bob.jsonl").is_file());
    assert_eq!(fs::read(&snapshot).expect("the snapshot reads"), damaged);
}

#[test]
fn a_valid_snapshot_is_left_untouched_by_a_later_approval() {
    // Arrange: the snapshot of the approved view, respelled without changing
    // what it says, so a rewrite would show.
    let dir = tempfile::tempdir().expect("a temp dir creates");
    calc_project(dir.path(), CALC_EVIDENCE);
    let snapshot = approved_snapshot(dir.path());
    let value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&snapshot).expect("reads")).expect("valid JSON");
    let respelled = serde_json::to_string(&value).expect("serializes");
    fs::write(&snapshot, &respelled).expect("the snapshot rewrites");

    // Act
    review_margin(dir.path(), "bob")
        .arg("--approve")
        .assert()
        .success()
        .stderr("");

    // Assert
    assert_eq!(fs::read_to_string(&snapshot).expect("reads"), respelled);
}

#[test]
fn declining_a_calc_bind_writes_no_snapshot() {
    // Arrange
    let dir = tempfile::tempdir().expect("a temp dir creates");
    calc_project(dir.path(), CALC_EVIDENCE);

    // Act
    review_margin(dir.path(), "alice")
        .arg("--decline")
        .arg("--comment")
        .arg("the margin is not shown")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "recorded decline of bind:margin_ok",
        ))
        .stdout(predicate::str::contains("snapshot").not());

    // Assert: the decline is on record, and nothing was snapshotted.
    assert!(dir.path().join("approvals/alice.jsonl").is_file());
    assert!(!dir.path().join("approvals/snapshots").exists());
}

#[test]
fn approving_a_test_bind_writes_no_snapshot() {
    // Arrange: a bind backed by loaded test evidence, not by a calculation.
    let dir = tempfile::tempdir().expect("a temp dir creates");
    fs::write(
        dir.path().join("inka.toml"),
        "[project]\nname = \"test-scenario\"\n\n[phases]\norder = [\"M0\"]\ncurrent = \"M0\"\n\n\
         [values]\nfiles = []\n\n[evidence]\ntests = [\"evidence.json\"]\n",
    )
    .expect("the manifest writes");
    fs::create_dir(dir.path().join("argument")).expect("the argument directory creates");
    fs::write(
        dir.path().join("argument/arg.inka"),
        "objective root {\n  ```\n  the root claim\n  ```\n  children: tested\n}\n\n\
         bind tested {\n  ```\n  the hash survives a round trip\n  ```\n  test: \"tests/clean.rs::holds\"\n}\n",
    )
    .expect("the argument writes");
    fs::write(
        dir.path().join("evidence.json"),
        r##"{"v": 1, "tests": [{"id": "tests/clean.rs::holds", "body": "#[test]\nfn holds() {}\n",
            "executions": [{"case": "default", "outcome": "pass"}]}]}"##,
    )
    .expect("the evidence writes");

    // Act
    inka()
        .arg("review")
        .arg("--path")
        .arg(dir.path())
        .arg("--as")
        .arg("alice")
        .arg("tested")
        .arg("--approve")
        .assert()
        .success()
        .stdout(predicate::str::contains("recorded approve of bind:tested"))
        .stdout(predicate::str::contains("snapshot").not());

    // Assert
    assert!(dir.path().join("approvals/alice.jsonl").is_file());
    assert!(!dir.path().join("approvals/snapshots").exists());
}

#[test]
fn control_characters_in_a_snapshot_never_reach_the_terminal() {
    // Arrange: the baseline snapshot carries a field whose name is an escape
    // sequence — clear the screen, retitle the window, ring the bell — with a
    // line separator and a right-to-left override for good measure. The
    // parser quotes the name it does not know.
    let dir = tempfile::tempdir().expect("a temp dir creates");
    calc_project(dir.path(), CALC_EVIDENCE);
    let snapshot = approved_snapshot(dir.path());
    fs::write(
        &snapshot,
        r#"{"v": 1, "\u001b[2J\u001b]0;pwn\u0007\u2028\u202e": true}"#,
    )
    .expect("the snapshot rewrites");

    // Act
    let output = review_margin(dir.path(), "bob")
        .arg("--approve")
        .output()
        .expect("the command runs");

    // Assert: the name is reported, stripped of everything a terminal obeys.
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).expect("stdout is UTF-8");
    let stderr = String::from_utf8(output.stderr).expect("stderr is UTF-8");
    assert!(stderr.contains("unknown field `[2J]0;pwn`"), "{stderr:?}");
    for stream in [&stdout, &stderr] {
        assert!(
            !stream.contains(['\u{1b}', '\u{07}', '\u{2028}', '\u{202e}']),
            "{stream:?}"
        );
    }
}

#[test]
fn record_timestamps_beyond_a_calendar_library_do_not_stop_a_review() {
    // Arrange: two approvals whose timestamps are valid records but outside
    // what a nanosecond, year-9999 calendar holds — thirteen fractional
    // digits, and the last second of the format.
    let dir = tempfile::tempdir().expect("a temp dir creates");
    calc_project(dir.path(), CALC_EVIDENCE);
    for (approver, timestamp) in [
        ("alice", "2026-11-10T02:55:10.1234567890123Z"),
        ("bob", "9999-12-31T23:59:59Z"),
    ] {
        review_margin(dir.path(), approver)
            .arg("--approve")
            .assert()
            .success();
        let records = dir.path().join(format!("approvals/{approver}.jsonl"));
        let mut record: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&records).expect("reads"))
                .expect("one JSON record");
        record["timestamp"] = timestamp.into();
        fs::write(&records, format!("{record}\n")).expect("the record rewrites");
    }

    // Act & Assert: both the queue and a further approval read the baseline.
    review_margin(dir.path(), "carol")
        .arg("--approve")
        .assert()
        .success()
        .stderr("");
    inka()
        .arg("review")
        .arg("--path")
        .arg(dir.path())
        .arg("--as")
        .arg("carol")
        .arg("--list")
        .assert()
        .success();
}

#[test]
fn deliveries_the_hash_cannot_tell_apart_snapshot_identically() {
    // Arrange: one calculation delivered twice — its definitions in either
    // order, its sources with either line ending. The view hash is the same,
    // so the two approvals address the same snapshot file.
    let ordered = r#"{"v": 1, "targets": [{"path": "Power::?verify_margin",
        "definitions": [
            {"path": "Power::?verify_margin", "kind": "verification", "source": "def verify_margin():\n    ...\n"},
            {"path": "Power::margin", "kind": "calculation", "source": "def margin():\n    ...\n"}],
        "assumes": ["model_valid"], "leaves": [{"id": "Power::?verify_margin", "result": true}], "valid": true}]}"#;
    let shuffled = r#"{"v": 1, "targets": [{"path": "Power::?verify_margin",
        "definitions": [
            {"path": "Power::margin", "kind": "calculation", "source": "def margin():\r\n    ...\r\n"},
            {"path": "Power::?verify_margin", "kind": "verification", "source": "def verify_margin():\r\n    ...\r\n"}],
        "assumes": ["model_valid"], "leaves": [{"id": "Power::?verify_margin", "result": true}], "valid": true}]}"#;
    let first = tempfile::tempdir().expect("a temp dir creates");
    let second = tempfile::tempdir().expect("a temp dir creates");
    calc_project(first.path(), ordered);
    calc_project(second.path(), shuffled);

    // Act
    let from_ordered = approved_snapshot(first.path());
    let from_shuffled = approved_snapshot(second.path());

    // Assert: whichever delivery is approved first, the file is the same.
    assert_eq!(from_ordered.file_name(), from_shuffled.file_name());
    let text = fs::read_to_string(&from_ordered).expect("the snapshot reads");
    assert_eq!(
        text,
        fs::read_to_string(&from_shuffled).expect("the snapshot reads")
    );
    assert!(!text.contains("\\r"), "{text}");
}

#[test]
fn a_snapshot_that_cannot_be_written_leaves_no_approval_record() {
    // Arrange: a file sits where the snapshots directory belongs.
    let dir = tempfile::tempdir().expect("a temp dir creates");
    calc_project(dir.path(), CALC_EVIDENCE);
    fs::create_dir(dir.path().join("approvals")).expect("the approvals directory creates");
    fs::write(dir.path().join("approvals/snapshots"), "in the way\n").expect("the file writes");

    // Act & Assert: the command fails, saying what it could not write and
    // that nothing was recorded — so running it again is the whole remedy.
    review_margin(dir.path(), "alice")
        .arg("--approve")
        .assert()
        .failure()
        .stdout("")
        .stderr(predicate::str::starts_with(
            "error: failed to write snapshot ",
        ))
        .stderr(predicate::str::contains("the approval was not recorded"));
    assert!(!dir.path().join("approvals/alice.jsonl").exists());
}

#[test]
fn a_declined_target_shows_as_declined_in_the_queue() {
    // Arrange
    let dir = tempfile::tempdir().expect("a temp dir creates");
    scenario_project(dir.path());

    // Act
    inka()
        .arg("review")
        .arg("--path")
        .arg(dir.path())
        .arg("--as")
        .arg("bob")
        .arg("leaf")
        .arg("--decline")
        .arg("--comment")
        .arg("the assumption does not hold")
        .assert()
        .success()
        .stdout(predicate::str::contains("recorded decline of fiat:leaf"));

    // Assert: the veto is shown for information, and eval reports it too.
    inka()
        .arg("review")
        .arg("--path")
        .arg(dir.path())
        .arg("--as")
        .arg("bob")
        .arg("--list")
        .assert()
        .success()
        .stdout(predicate::str::contains("fiat:leaf — declined by bob"));
    inka()
        .arg("eval")
        .arg(dir.path())
        .assert()
        .failure()
        .stdout(predicate::str::contains(
            "declined by bob: the assumption does not hold",
        ));
}

#[test]
fn a_second_approval_appends_without_touching_the_first_record() {
    // Arrange: one approval on the leaf…
    let dir = tempfile::tempdir().expect("a temp dir creates");
    scenario_project(dir.path());
    inka()
        .arg("review")
        .arg("--path")
        .arg(dir.path())
        .arg("--as")
        .arg("alice")
        .arg("leaf")
        .arg("--approve")
        .assert()
        .success();
    let file = dir.path().join("approvals/alice.jsonl");
    let first = fs::read_to_string(&file).expect("the record file reads");

    // Act: …and a second on another target, by the same approver.
    inka()
        .arg("review")
        .arg("--path")
        .arg(dir.path())
        .arg("--as")
        .arg("alice")
        .arg("branch")
        .arg("--approve")
        .assert()
        .success();

    // Assert: the file still starts with the first record, byte for byte.
    let after = fs::read_to_string(&file).expect("the record file reads");
    assert!(
        after.starts_with(&first),
        "existing lines are never rewritten"
    );
    assert_eq!(after.lines().count(), 2);
}

#[test]
fn the_key_id_defaults_to_the_approver_and_an_override_wins() {
    // Arrange
    let dir = tempfile::tempdir().expect("a temp dir creates");
    scenario_project(dir.path());

    // Act: one default record, one with --key-id.
    inka()
        .arg("review")
        .arg("--path")
        .arg(dir.path())
        .arg("--as")
        .arg("alice")
        .arg("leaf")
        .arg("--approve")
        .assert()
        .success();
    inka()
        .arg("review")
        .arg("--path")
        .arg(dir.path())
        .arg("--as")
        .arg("alice")
        .arg("--key-id")
        .arg("alice-2026")
        .arg("branch")
        .arg("--approve")
        .assert()
        .success();

    // Assert
    let text = fs::read_to_string(dir.path().join("approvals/alice.jsonl"))
        .expect("the record file reads");
    let lines: Vec<&str> = text.lines().collect();
    assert!(lines[0].contains("\"key_id\":\"alice\""));
    assert!(lines[1].contains("\"key_id\":\"alice-2026\""));
}

#[test]
fn a_decline_without_a_comment_is_rejected_at_argument_parsing() {
    // Arrange & Act & Assert: the parser enforces the mandatory comment, so
    // no project is even loaded.
    inka()
        .arg("review")
        .arg("--as")
        .arg("bob")
        .arg("leaf")
        .arg("--decline")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--comment"));
}

#[test]
fn a_target_without_a_decision_is_rejected_at_argument_parsing() {
    inka()
        .arg("review")
        .arg("--as")
        .arg("bob")
        .arg("leaf")
        .assert()
        .code(2)
        .stderr(predicate::str::contains("--approve").or(predicate::str::contains("--decline")));
}

#[test]
fn an_unknown_target_is_an_error() {
    // Arrange
    let dir = tempfile::tempdir().expect("a temp dir creates");
    scenario_project(dir.path());

    // Act & Assert
    inka()
        .arg("review")
        .arg("--path")
        .arg(dir.path())
        .arg("--as")
        .arg("alice")
        .arg("nonesuch")
        .arg("--approve")
        .assert()
        .failure()
        .stderr(predicate::str::contains("no block named `nonesuch`"));
}

#[test]
fn interactive_mode_without_a_terminal_is_refused_with_a_hint() {
    // Arrange & Act & Assert: a piped stdin must not silently consume the
    // queue; the refusal points at the non-interactive forms.
    inka()
        .arg("review")
        .arg("--path")
        .arg(workspace_fixture("dogfood"))
        .arg("--as")
        .arg("shimomura")
        .assert()
        .failure()
        .stderr(predicate::str::contains("standard input is not a terminal"))
        .stderr(predicate::str::contains("--list"));
}
