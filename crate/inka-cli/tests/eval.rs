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
use inka_core::calc_evidence;
use inka_core::evidence;
use inka_core::graph::Graph;
use inka_core::hash::{EvidenceContract, LocalView};
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
fn dogfood_project_establishes_its_first_real_bind() {
    // Arrange & Act & Assert: the dogfood project carries one approved bind
    // backed by adapter-emitted, capsula-delivered evidence (#75); the rest
    // of the argument stays unapproved, so the verdict still blames and the
    // exit status stays non-zero.
    inka()
        .arg("eval")
        .arg(workspace_fixture("dogfood"))
        .assert()
        .failure()
        .code(1)
        .stdout(predicate::str::starts_with("established 1/"))
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
        .stdout(predicate::str::starts_with("established 1/"));
}

#[test]
fn each_failure_kind_is_blamed_on_the_failing_node_alone() {
    // Act: the fixture holds one node per failure kind — missing, stale,
    // declined, expired, test binds with missing, failing and incomplete
    // evidence, and calc binds with missing evidence, false and missing
    // leaves, a failed validity condition, an expected failure and an
    // unestablished assumption — under one unapproved objective.
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
    // missing, stale, declined, expired, missing/failed/incomplete test
    // evidence, and every calc evidence cause.
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

#[test]
fn a_bind_established_through_loaded_evidence_evaluates_clean() {
    // Arrange: the manifest's [evidence] file carries a passing test, and the
    // bind approval is bound to the interface-bearing view hash.
    let project = evidence_project("evidence-clean");

    // Act & Assert: the one new CLI behavior end to end — evidence loaded
    // from disk through the manifest establishes the bind.
    inka()
        .arg("eval")
        .arg(&project)
        .assert()
        .success()
        .stdout("established 2/2\n")
        .stderr("");
}

#[test]
fn a_calc_bind_established_through_loaded_evidence_evaluates_clean() {
    // Arrange: the manifest's [evidence] calcs file carries a passing target
    // that assumes the fiat; every approval is bound to the current hashes,
    // the bind's carrying the interface hash and the fiat's description.
    let project = calc_evidence_project("calc-evidence-clean");

    // Act & Assert: the closing condition of #40 end to end — calc evidence
    // loaded from disk through the manifest establishes the bind, assumption
    // included.
    inka()
        .arg("eval")
        .arg(&project)
        .assert()
        .success()
        .stdout("established 3/3\n")
        .stderr("");
}

#[test]
fn an_invalid_evidence_file_fails_before_any_verdict() {
    // Act & Assert: a configured evidence file with an unsupported version is
    // a CLI input error, not a verdict.
    inka()
        .current_dir(local_fixture("broken-evidence"))
        .args(["eval", "."])
        .assert()
        .failure()
        .code(1)
        .stdout("")
        .stderr(predicate::str::contains("test evidence"));
}

#[test]
fn an_invalid_calc_evidence_file_fails_before_any_verdict() {
    inka()
        .current_dir(local_fixture("broken-calc-evidence"))
        .args(["eval", "."])
        .assert()
        .failure()
        .code(1)
        .stdout("")
        .stderr(predicate::str::contains("calc evidence"));
}

/// The contract fixtures of the evidence-established project.
const EVIDENCE_TEST_ID: &str = "tests/clean.rs::holds";
const EVIDENCE_BODY: &str = "#[test]\nfn holds() {}\n";

/// Builds a project whose bind is established through loaded test evidence:
/// a manifest with `[evidence]`, a passing evidence file, and approvals
/// bound to the current view hashes including the bind's interface hash.
fn evidence_project(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("eval-{name}"));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).expect("the old fixture should be removable");
    }
    std::fs::create_dir_all(dir.join("argument")).expect("the fixture should be creatable");
    std::fs::write(
        dir.join("inka.toml"),
        "[project]\nname = \"evidence-clean\"\n\n[phases]\norder = [\"M1\"]\ncurrent = \"M1\"\n\n\
         [values]\nfiles = []\n\n[evidence]\ntests = [\"evidence.json\"]\n",
    )
    .expect("the manifest should be writable");
    std::fs::write(
        dir.join("argument").join("main.inka"),
        format!(
            "objective root {{\n  ```\n  the mission succeeds\n  ```\n  children: tested\n}}\n\n\
             bind tested {{\n  ```\n  the hash survives a round trip\n  ```\n  test: \"{EVIDENCE_TEST_ID}\"\n}}\n"
        ),
    )
    .expect("the argument should be writable");
    let body_json = serde_json::to_string(EVIDENCE_BODY).expect("a JSON string");
    std::fs::write(
        dir.join("evidence.json"),
        format!(
            r#"{{"v": 1, "tests": [{{"id": "{EVIDENCE_TEST_ID}", "body": {body_json}, "executions": [{{"case": "default", "outcome": "pass"}}]}}]}}"#
        ),
    )
    .expect("the evidence should be writable");

    let project = load_project(&dir)
        .result
        .expect("the fixture project should load");
    let graph = Graph::build(&project.blocks).expect("the fixture graph should build");
    let view = ArgumentView::build(&graph, &project.source_map);
    let interface = evidence::interface_hash(EVIDENCE_TEST_ID, EVIDENCE_BODY, &["default"]);
    for node in &view.nodes {
        let bind_interface =
            (node.mode == inka_core::view::Mode::Bind).then(|| EvidenceContract::from(interface));
        let hash = LocalView::of_node(&view, node, bind_interface)
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

/// The contract fixtures of the calc-evidence-established project.
const CALC_TARGET: &str = "AOCS::?verify_detumble";
const CALC_SOURCE: &str =
    "@aocs.verification()\ndef verify_detumble(r: Result) -> bool:\n    return r.ok\n";
const ASSUMED_DESCRIPTION: &str = "the disturbance model envelopes reality";

/// Builds a project whose calc bind is established through loaded calc
/// evidence: a manifest with `[evidence] calcs`, a passing target assuming
/// the fiat, and approvals bound to the current view hashes — the bind's
/// including the interface hash and the assumed fiat's description.
fn calc_evidence_project(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("eval-{name}"));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).expect("the old fixture should be removable");
    }
    std::fs::create_dir_all(dir.join("argument")).expect("the fixture should be creatable");
    std::fs::write(
        dir.join("inka.toml"),
        "[project]\nname = \"calc-evidence-clean\"\n\n[phases]\norder = [\"M1\"]\ncurrent = \"M1\"\n\n\
         [values]\nfiles = []\n\n[evidence]\ncalcs = [\"calc-evidence.json\"]\n",
    )
    .expect("the manifest should be writable");
    std::fs::write(
        dir.join("argument").join("main.inka"),
        format!(
            "objective root {{\n  ```\n  the mission succeeds\n  ```\n  children: modelled, assumed\n}}\n\n\
             bind modelled {{\n  ```\n  the controller detumbles\n  ```\n  calc: \"{CALC_TARGET}\"\n}}\n\n\
             fiat assumed {{\n  ```\n  {ASSUMED_DESCRIPTION}\n  ```\n}}\n"
        ),
    )
    .expect("the argument should be writable");
    let source_json = serde_json::to_string(CALC_SOURCE).expect("a JSON string");
    std::fs::write(
        dir.join("calc-evidence.json"),
        format!(
            r#"{{"v": 1, "targets": [{{"path": "{CALC_TARGET}",
                "definitions": [{{"path": "{CALC_TARGET}", "kind": "verification", "source": {source_json}}}],
                "assumes": ["assumed"], "leaves": [{{"id": "{CALC_TARGET}", "result": true}}], "valid": true}}]}}"#
        ),
    )
    .expect("the evidence should be writable");

    let project = load_project(&dir)
        .result
        .expect("the fixture project should load");
    let graph = Graph::of_project(&project).expect("the fixture graph should build");
    let view = ArgumentView::build(&graph, &project.source_map);
    let interface = calc_evidence::interface_hash(
        CALC_TARGET,
        &[calc_evidence::Definition {
            path: CALC_TARGET,
            source: CALC_SOURCE,
        }],
        &[CALC_TARGET],
    );
    for node in &view.nodes {
        let evidence = (node.mode == inka_core::view::Mode::Bind).then(|| EvidenceContract {
            interface,
            assumptions: vec![ASSUMED_DESCRIPTION],
        });
        let hash = LocalView::of_node(&view, node, evidence)
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
