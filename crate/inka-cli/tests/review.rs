//! Subprocess tests for `inka review`: queue listing, the non-interactive
//! single-target form, argument-parsing guarantees, and the non-TTY refusal.
//!
//! The scenario tests drive a small three-node project in a temporary
//! directory through the binary alone — approve everything, watch the queue
//! empty, edit a description, watch exactly the target and its direct parent
//! come back — so the queue, the records and `inka eval` are exercised as one
//! workflow, the way a reviewer uses them.

use std::fs;
use std::path::Path;

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
