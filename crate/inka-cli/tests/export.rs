//! Subprocess tests for `inka export`: the JSON the fixtures produce, its
//! stability, and what happens when there is no argument to export.
//!
//! The snapshots freeze the wire format itself. `inka serve` and any external
//! consumer read exactly these bytes, so a change to them is a change to an
//! interface and should be reviewed as one.

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;

mod common;
use common::{local_fixture, workspace_fixture};

/// A fresh `inka` command bound to the crate's built binary.
fn inka() -> Command {
    Command::cargo_bin("inka").expect("the inka binary should build")
}

/// Exports a shared workspace fixture, asserting the run succeeded.
fn export(fixture: &str) -> String {
    let assert = inka()
        .arg("export")
        .arg(workspace_fixture(fixture))
        .assert()
        .success()
        .stderr("");
    String::from_utf8(assert.get_output().stdout.clone()).expect("the export should be UTF-8")
}

fn elements(fixture: &str) -> Vec<Value> {
    serde_json::from_str(&export(fixture)).expect("the export should be valid JSON")
}

#[test]
fn aocs_export_matches_its_snapshot() {
    insta::assert_snapshot!(export("aocs"));
}

#[test]
fn dogfood_export_matches_its_snapshot() {
    insta::assert_snapshot!(export("dogfood"));
}

#[test]
fn export_is_byte_for_byte_reproducible() {
    // Arrange & Act: two runs of the same project, in separate processes.
    let first = export("aocs");
    let second = export("aocs");

    // Assert
    assert_eq!(first, second);
}

#[test]
fn every_element_carries_a_data_object() {
    for fixture in ["aocs", "dogfood"] {
        for element in elements(fixture) {
            assert!(
                element.get("data").is_some_and(Value::is_object),
                "{fixture}: every cytoscape element wraps its data"
            );
        }
    }
}

#[test]
fn every_edge_names_two_exported_nodes() {
    for fixture in ["aocs", "dogfood"] {
        let elements = elements(fixture);
        let ids: Vec<&str> = elements
            .iter()
            .map(|element| element["data"]["id"].as_str().expect("ids are strings"))
            .collect();

        for element in &elements {
            let data = &element["data"];
            let (Some(source), Some(target)) = (data.get("source"), data.get("target")) else {
                continue;
            };
            assert!(
                ids.contains(&source.as_str().expect("source is a string")),
                "{fixture}: {source} should be an exported node"
            );
            assert!(
                ids.contains(&target.as_str().expect("target is a string")),
                "{fixture}: {target} should be an exported node"
            );
        }
    }
}

#[test]
fn every_node_is_exported_exactly_once() {
    // A node with several parents is shown through its edges, never duplicated.
    let elements = elements("aocs");
    let mut node_ids: Vec<&str> = elements
        .iter()
        .filter(|element| element["data"].get("source").is_none())
        .map(|element| element["data"]["id"].as_str().expect("ids are strings"))
        .collect();
    let total = node_ids.len();

    node_ids.sort_unstable();
    node_ids.dedup();

    assert_eq!(node_ids.len(), total);
    assert!(node_ids.contains(&"actuator_authority_ok"));
}

#[test]
fn descriptions_arrive_interpolated() {
    for fixture in ["aocs", "dogfood"] {
        for element in elements(fixture) {
            let Some(description) = element["data"].get("description") else {
                continue;
            };
            assert!(
                !description
                    .as_str()
                    .expect("descriptions are strings")
                    .contains("{$"),
                "{fixture}: no placeholder should reach a consumer"
            );
        }
    }
}

#[test]
fn path_defaults_to_the_current_directory() {
    inka()
        .current_dir(workspace_fixture("dogfood"))
        .arg("export")
        .assert()
        .success()
        .stderr("")
        .stdout(predicate::str::starts_with("[\n"));
}

#[test]
fn an_argument_that_fails_the_check_exports_nothing() {
    // Arrange & Act: the same fixture `inka check` rejects for a dangling
    // reference. An export of part of an argument would be indistinguishable
    // from a smaller argument, so nothing is written at all.
    inka()
        .current_dir(local_fixture("undefined-reference"))
        .args(["export", "."])
        .assert()
        .failure()
        .code(1)
        .stdout("")
        .stderr(predicate::str::contains("undefined reference"));
}

#[test]
fn a_project_without_an_objective_exports_nothing() {
    inka()
        .current_dir(local_fixture("no-objective"))
        .args(["export", "."])
        .assert()
        .failure()
        .code(1)
        .stdout("")
        .stderr(predicate::str::contains("no objective block found"));
}

#[test]
fn a_warning_does_not_stop_the_export() {
    // The unreachable-node fixture is well-formed but suspicious: it warns on
    // stderr and still exports, exactly as `inka check` still exits 0.
    inka()
        .current_dir(local_fixture("unreachable"))
        .args(["export", "."])
        .assert()
        .success()
        .stdout(predicate::str::starts_with("[\n"))
        .stderr(predicate::str::contains("unreachable"));
}

#[test]
fn missing_manifest_reports_a_structural_error() {
    inka()
        .arg("export")
        .arg(workspace_fixture("aocs").join("argument"))
        .assert()
        .failure()
        .code(1)
        .stdout("")
        .stderr(predicate::str::contains("inka.toml"));
}
