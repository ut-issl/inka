//! Offline bind scenarios using synthetic approvals and independent bundles.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use assert_cmd::Command;
use inka_core::approvals;
use inka_core::evidence::{Bundle, CaseResult, ResultKind};
use inka_core::graph::Graph;
use inka_core::hash::LocalView;
use inka_core::project::load_project;
use inka_core::record::{Record, RecordFields, Target};
use inka_core::view::ArgumentView;

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Project {
    path: PathBuf,
    bundle: Bundle,
}
impl Project {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "inka-bind-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(path.join("argument")).unwrap();
        fs::write(path.join("inka.toml"), "[project]\nname = 'ci'\n[phases]\norder = ['M0', 'M1']\ncurrent = 'M0'\n[values]\nfiles = []\n[evidence.sources.upstream]\nid = 'github:example/source'\nfile = 'import.json'\n").unwrap();
        fs::write(path.join("argument/main.inka"), "objective root {\n```\nThe selected property holds\n```\nchildren: tested\n}\nbind tested {\n```\nThe arithmetic property holds\n```\ntest: \"upstream/tests/example.rs\"\n}\n").unwrap();
        let vectors: Vec<serde_json::Value> = serde_json::from_str(include_str!(
            "../../../tests/golden/evidence/v2/contracts.json"
        ))
        .unwrap();
        let project = Self {
            path,
            bundle: Bundle::parse(&vectors[0]["bundle"].to_string()).unwrap(),
        };
        project.write_bundle();
        project
    }
    fn write_bundle(&self) {
        fs::write(
            self.path.join("import.json"),
            serde_json::to_string_pretty(&self.bundle).unwrap(),
        )
        .unwrap();
    }
    fn approve(&self) {
        let project = load_project(&self.path).result.unwrap();
        let graph = Graph::build(&project.blocks).unwrap();
        let view = ArgumentView::build(&graph, &project.source_map);
        for node in &view.nodes {
            let hash = LocalView::of_node(
                &view,
                node,
                Some(self.bundle.targets[0].hash(&self.bundle.source.id)),
            )
            .unwrap()
            .view_hash();
            let record = Record::approve(
                RecordFields {
                    target: Target::new(node.mode, &node.id).unwrap(),
                    view_hash: hash,
                    approver: "synthetic-test".into(),
                    key_id: "test-only".into(),
                    timestamp: "2026-09-11T00:00:00Z".parse().unwrap(),
                    phase: "M0".into(),
                    until: None,
                    policy_hash: None,
                },
                None,
            )
            .unwrap();
            approvals::append(&self.path.join("approvals"), &record).unwrap();
        }
    }
    fn eval(&self, success: bool) -> serde_json::Value {
        let mut command = Command::cargo_bin("inka").unwrap();
        let assertion = command.args(["eval", "--json"]).arg(&self.path).assert();
        let assertion = if success {
            assertion.success()
        } else {
            assertion.failure()
        };
        let assertion = assertion.stderr("");
        serde_json::from_slice(&assertion.get_output().stdout).unwrap()
    }
}
impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn pass_and_approval_are_both_required_and_result_updates_do_not_stale_approval() {
    let mut project = Project::new();
    let unapproved = project.eval(false);
    assert_eq!(
        unapproved["blame"][1]["reasons"][0]["cause"],
        "missing_approval"
    );
    project.approve();
    assert_eq!(project.eval(true)["established"], 2);
    for result in [ResultKind::Fail, ResultKind::Skip, ResultKind::Cancelled] {
        project.bundle.targets[0].results[0].result = result;
        project.write_bundle();
        let report = project.eval(false);
        assert_eq!(report["blame"].as_array().unwrap().len(), 1);
        assert_eq!(
            report["blame"][0]["reasons"][0]["cause"],
            "evidence_not_passed"
        );
        assert_eq!(report["blame"][0]["reasons"].as_array().unwrap().len(), 1);
    }
    project.bundle.targets[0].results.clear();
    project.write_bundle();
    assert!(project.eval(false)["blame"][0]["reasons"][0]["cases"][0]["result"].is_null());
}

#[test]
fn imported_content_identity_and_required_selection_changes_need_reapproval() {
    for change in ["content", "target", "source", "required"] {
        let mut project = Project::new();
        project.approve();
        match change {
            "content" => project.bundle.targets[0].content.push_str("// changed\n"),
            "target" => {
                project.bundle.targets[0].id = "renamed".into();
                let file = project.path.join("argument/main.inka");
                fs::write(
                    &file,
                    fs::read_to_string(&file)
                        .unwrap()
                        .replace("upstream/tests/example.rs", "upstream/renamed"),
                )
                .unwrap();
            }
            "source" => {
                project.bundle.source.id = "github:example/other".into();
                let file = project.path.join("inka.toml");
                fs::write(
                    &file,
                    fs::read_to_string(&file)
                        .unwrap()
                        .replace("github:example/source", "github:example/other"),
                )
                .unwrap();
            }
            _ => {
                project.bundle.targets[0]
                    .required
                    .push("windows/works".into());
                project.bundle.targets[0].results.push(CaseResult {
                    case: "windows/works".into(),
                    result: ResultKind::Pass,
                });
            }
        }
        project.bundle.targets[0].interface_hash = project.bundle.targets[0]
            .hash(&project.bundle.source.id)
            .to_string();
        project.write_bundle();
        assert_eq!(
            project.eval(false)["blame"][0]["reasons"][0]["cause"],
            "stale_approval",
            "{change}"
        );
        project.approve();
        project.eval(true);
    }
}

#[test]
fn upstream_edits_do_not_affect_imported_evaluation_and_corrupt_bundles_abort() {
    let project = Project::new();
    project.approve();
    fs::write(
        project.path.join("unimported-source.rs"),
        "changed upstream",
    )
    .unwrap();
    project.eval(true);
    fs::remove_file(project.path.join("import.json")).unwrap();
    assert_eq!(
        project.eval(false)["blame"][0]["reasons"][0]["problem"]["cause"],
        "missing_bundle"
    );
    fs::write(project.path.join("import.json"), "not-json").unwrap();
    Command::cargo_bin("inka")
        .unwrap()
        .args(["eval", "--json"])
        .arg(&project.path)
        .assert()
        .failure()
        .stdout("")
        .stderr(predicates::str::contains("import.json"));
}

#[test]
fn valid_evidence_does_not_override_decline_or_expiry() {
    let project = Project::new();
    project.approve();
    let file = project.path.join("approvals/synthetic-test.jsonl");
    let text = fs::read_to_string(&file).unwrap();
    let mut record: serde_json::Value = text
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .find(|record| record["target"] == "bind:tested")
        .unwrap();
    record["label"] = "decline".into();
    record["comment"] = "Synthetic veto".into();
    fs::write(
        file,
        format!("{text}{}\n", serde_json::to_string(&record).unwrap()),
    )
    .unwrap();
    assert_eq!(
        project.eval(false)["blame"][0]["reasons"][0]["cause"],
        "declined"
    );
    let expired = Project::new();
    let arg = expired.path.join("argument/main.inka");
    fs::write(
        &arg,
        fs::read_to_string(&arg)
            .unwrap()
            .replace("bind tested {", "bind tested {\nuntil: M0"),
    )
    .unwrap();
    expired.approve();
    let manifest = expired.path.join("inka.toml");
    fs::write(
        &manifest,
        fs::read_to_string(&manifest)
            .unwrap()
            .replace("current = 'M0'", "current = 'M1'"),
    )
    .unwrap();
    assert_eq!(
        expired.eval(false)["blame"][0]["reasons"][0]["cause"],
        "until_expired"
    );
}

#[test]
fn test_evidence_exports_its_public_kind_and_rejects_old_dsl_syntax() {
    let project = Project::new();
    let output = Command::cargo_bin("inka")
        .unwrap()
        .arg("export")
        .arg(&project.path)
        .assert()
        .success();
    let elements: Vec<serde_json::Value> =
        serde_json::from_slice(&output.get_output().stdout).unwrap();
    let tested = elements
        .iter()
        .find(|element| element["data"]["id"] == "tested")
        .unwrap();
    assert_eq!(tested["data"]["evidence"]["kind"], "test");
    assert_eq!(
        tested["data"]["evidence"]["target"],
        "upstream/tests/example.rs"
    );

    let argument = project.path.join("argument/main.inka");
    fs::write(
        &argument,
        fs::read_to_string(&argument)
            .unwrap()
            .replace("test:", "ci:"),
    )
    .unwrap();
    Command::cargo_bin("inka")
        .unwrap()
        .arg("check")
        .arg(&project.path)
        .assert()
        .failure()
        .stderr(predicates::str::contains("unknown property `ci`"));
}
