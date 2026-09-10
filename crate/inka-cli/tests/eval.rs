//! Subprocess tests for `inka eval`: exit status, the human summary, the
//! blame list, and the JSON report.
//!
//! The `unestablished` fixture is run with the fixture directory as the
//! working directory and `.` as the path argument, so any path the command
//! prints is relative and stable across machines. The clean-project test
//! builds its records programmatically against the current view hashes, so no
//! checked-in hash can drift out of date.

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use predicates::prelude::*;

use inka_core::approvals;
use inka_core::graph::Graph;
use inka_core::hash::LocalView;
use inka_core::project::load_project;
use inka_core::record::{Record, RecordFields, Target};
use inka_core::view::ArgumentView;

mod common;
use common::{local_fixture, workspace_fixture};

/// A fresh `inka` command bound to the crate's built binary.
fn inka() -> Command {
    Command::cargo_bin("inka").expect("the inka binary should build")
}

#[test]
fn dogfood_project_evaluates_and_reports_nothing_established() {
    // Arrange & Act & Assert: the dogfood closing condition — the command can
    // evaluate the dogfood project. Nothing is approved there yet, so the
    // verdict is a full blame list and a non-zero exit status.
    inka()
        .arg("eval")
        .arg(workspace_fixture("dogfood"))
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::starts_with("established 0/"))
        .stderr("");
}

#[test]
fn path_defaults_to_the_current_directory() {
    inka()
        .current_dir(workspace_fixture("dogfood"))
        .arg("eval")
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::starts_with("established 0/"));
}

#[test]
fn each_failure_kind_is_blamed_on_the_failing_node_alone() {
    // Act: the fixture holds one node per failure kind — missing, stale,
    // declined, expired, and the stubbed bind — under one unapproved objective.
    // The declined leaf also has a current approval, exercising the veto.
    let assert = inka()
        .current_dir(local_fixture("unestablished"))
        .args(["eval", "."])
        .assert()
        .failure()
        .code(1)
        .stderr("");

    // Assert: freeze the whole report so wording and ordering drift is caught.
    let stdout = String::from_utf8(assert.get_output().stdout.clone())
        .expect("the report should be valid UTF-8");
    insta::assert_snapshot!(stdout);
}

#[test]
fn a_fully_approved_project_is_clean_and_exits_zero() {
    // Arrange
    let project = approved_project("clean");

    // Act & Assert
    inka()
        .arg("eval")
        .arg(&project)
        .assert()
        .success()
        .stdout("established 2/2\n")
        .stderr("");
}

#[test]
fn the_json_report_matches_the_human_verdict() {
    // Arrange
    let project = approved_project("clean-json");

    // Act
    let assert = inka()
        .arg("eval")
        .arg("--json")
        .arg(&project)
        .assert()
        .success()
        .stderr("");

    // Assert
    let json: serde_json::Value = serde_json::from_slice(&assert.get_output().stdout)
        .expect("the report should be valid JSON");
    assert_eq!(json["established"], 2);
    assert_eq!(json["total"], 2);
    assert_eq!(json["nodes"].as_array().map(Vec::len), Some(2));
    assert_eq!(json["blame"].as_array().map(Vec::len), Some(0));
}

#[test]
fn the_json_report_spells_every_blame_cause() {
    // Act: the fixture exercises every blame cause the JSON contract has —
    // missing, stale, declined, expired, and the stubbed bind.
    let assert = inka()
        .current_dir(local_fixture("unestablished"))
        .args(["eval", "--json", "."])
        .assert()
        .failure()
        .code(1)
        .stderr("");

    // Assert: freeze the machine-readable shape so a field rename is caught.
    let stdout = String::from_utf8(assert.get_output().stdout.clone())
        .expect("the report should be valid UTF-8");
    insta::assert_snapshot!(stdout);
}

#[test]
fn a_broken_argument_reports_diagnostics_and_no_verdict() {
    // Act & Assert: eval holds the same bar as check — an argument that does
    // not load has no verdict, only findings.
    inka()
        .current_dir(local_fixture("undefined-reference"))
        .args(["eval", "."])
        .assert()
        .failure()
        .code(1)
        .stdout("")
        .stderr(predicate::str::contains("undefined reference"));
}

#[test]
fn a_malformed_approvals_file_fails_loudly_with_file_and_line() {
    // Arrange: a valid argument whose approvals directory holds garbage.
    let project = approved_project("malformed-approvals");
    std::fs::write(
        project.join("approvals").join("sato.jsonl"),
        "not a record\n",
    )
    .expect("the fixture should be writable");

    // Act & Assert
    inka()
        .arg("eval")
        .arg(&project)
        .assert()
        .failure()
        .code(1)
        .stdout("")
        .stderr(predicate::str::contains("sato.jsonl:1"));
}

/// Builds a two-node project under `CARGO_TARGET_TMPDIR` and approves every
/// node against its current view hash, the way the recording command will.
fn approved_project(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("eval-{name}"));
    // A fresh directory per run: a leftover approvals file from an earlier
    // test run would otherwise leak into this one.
    if dir.exists() {
        std::fs::remove_dir_all(&dir).expect("the old fixture should be removable");
    }
    std::fs::create_dir_all(dir.join("argument")).expect("the fixture should be creatable");
    std::fs::write(
        dir.join("inka.toml"),
        "[project]\nname = \"clean\"\n\n[phases]\norder = [\"M1\"]\ncurrent = \"M1\"\n\n\
         [values]\nfiles = []\n",
    )
    .expect("the manifest should be writable");
    std::fs::write(
        dir.join("argument").join("main.inka"),
        "objective root {\n  ```\n  the mission succeeds\n  ```\n  children: leaf\n}\n\n\
         fiat leaf {\n  ```\n  taken as given\n  ```\n}\n",
    )
    .expect("the argument should be writable");

    let project = load_project(&dir)
        .result
        .expect("the fixture project should load");
    let graph = Graph::build(&project.blocks).expect("the fixture graph should build");
    let view = ArgumentView::build(&graph, &project.source_map);
    for node in &view.nodes {
        let hash = LocalView::of_node(&view, node, None)
            .expect("fixture nodes should be hashable")
            .view_hash();
        let record = Record::approve(
            RecordFields {
                target: Target::new(node.mode, &node.id).expect("a valid target"),
                view_hash: hash,
                approver: "iwata".to_owned(),
                key_id: "iwata-2026".to_owned(),
                timestamp: "2026-09-10T08:30:00Z".parse().expect("a valid timestamp"),
                phase: "M1".to_owned(),
                until: None,
                policy_hash: None,
            },
            None,
        )
        .expect("a valid record");
        approvals::append(&dir.join("approvals"), &record)
            .expect("the record should be appendable");
    }
    dir
}
