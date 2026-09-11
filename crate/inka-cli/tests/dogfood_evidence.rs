//! Actual proptest execution -> prepared evidence -> offline dogfood bind.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;
use inka_core::approvals;
use inka_core::evidence::{Bundle, ResultKind};
use inka_core::graph::Graph;
use inka_core::hash::LocalView;
use inka_core::project::load_project;
use inka_core::record::{Record, RecordFields, Target};
use inka_core::view::ArgumentView;
use serde_json::Value;

fn copy_tree(source: &Path, dest: &Path) {
    fs::create_dir_all(dest).unwrap();
    for entry in fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name() == "evidence" || entry.file_name() == "approvals" {
            continue;
        }
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &dest.join(entry.file_name()));
        } else {
            fs::copy(entry.path(), dest.join(entry.file_name())).unwrap();
        }
    }
}

fn approve(receiver: &Path, bundle: &Bundle) {
    let project = load_project(receiver).result.unwrap();
    let graph = Graph::build(&project.blocks).unwrap();
    let view = ArgumentView::build(&graph, &project.source_map);
    let node = view
        .nodes
        .iter()
        .find(|n| n.id == "normalization_idempotent")
        .unwrap();
    let hash = LocalView::of_node(&view, node, Some(bundle.targets[1].hash(&bundle.source.id)))
        .unwrap()
        .view_hash();
    let record = Record::approve(
        RecordFields {
            target: Target::new(node.mode, &node.id).unwrap(),
            view_hash: hash,
            approver: "synthetic-dogfood-test".into(),
            key_id: "test-only".into(),
            timestamp: "2026-09-11T00:00:00Z".parse().unwrap(),
            phase: "M0".into(),
            until: None,
            policy_hash: None,
        },
        None,
    )
    .unwrap();
    approvals::append(&receiver.join("approvals"), &record).unwrap();
}

fn evaluate(receiver: &Path) -> Value {
    let output = Command::cargo_bin("inka")
        .unwrap()
        .args(["eval", "--json"])
        .arg(receiver)
        .assert()
        .failure()
        .stderr("")
        .get_output()
        .stdout
        .clone();
    serde_json::from_slice(&output).unwrap()
}

fn produce(root: &Path, work: &Path) -> Bundle {
    let mut plan: Value =
        serde_json::from_str(include_str!("../../../examples/ci/dogfood-plan.json")).unwrap();
    plan["source"]["revision"] = "actual-workspace-snapshot".into();
    plan["execution"]["id"] = "test-only-run".into();
    for target in plan["targets"].as_array_mut().unwrap() {
        target["file"] = root
            .join("crate/inka-core/tests/hash_props.rs")
            .display()
            .to_string()
            .into();
    }
    fs::write(work.join("plan.json"), plan.to_string()).unwrap();
    Command::cargo_bin("inka")
        .unwrap()
        .current_dir(work)
        .args([
            "ci-export",
            "prepare-rust",
            "--plan",
            "plan.json",
            "--out",
            "receipt.json",
        ])
        .assert()
        .success();
    // The source-side test harness executes cargo, never the inka CLI itself.
    // This target contains only the eight property tests, not this integration test.
    let run = std::process::Command::new(env!("CARGO"))
        .current_dir(root)
        .args([
            "test",
            "--offline",
            "--locked",
            "--color",
            "never",
            "-p",
            "inka-core",
            "--test",
            "hash_props",
            "--",
            "--format",
            "pretty",
            "--color",
            "never",
        ])
        .output()
        .unwrap();
    assert!(
        run.status.success(),
        "{}",
        String::from_utf8_lossy(&run.stderr)
    );
    fs::write(work.join("libtest.txt"), run.stdout).unwrap();
    Command::cargo_bin("inka")
        .unwrap()
        .current_dir(work)
        .args([
            "ci-export",
            "finish-rust",
            "--receipt",
            "receipt.json",
            "--report",
            "linux=libtest.txt",
            "--out",
            "evidence.json",
        ])
        .assert()
        .success();
    Bundle::parse(&fs::read_to_string(work.join("evidence.json")).unwrap()).unwrap()
}

#[test]
fn actual_hash_proptests_support_only_the_approved_imported_dogfood_contract() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap();
    let work = std::env::temp_dir().join(format!("inka-dogfood-evidence-{}", std::process::id()));
    fs::create_dir_all(&work).unwrap();
    let receiver = work.join("receiver");
    copy_tree(&root.join("tests/fixtures/dogfood"), &receiver);
    let mut bundle = produce(&root, &work);
    assert_eq!(bundle.targets[0].results.len(), 8);
    assert_eq!(bundle.targets[1].results.len(), 1);
    assert!(
        !bundle.targets[1]
            .content
            .contains("fn child_order_does_not_reach_the_hash")
    );
    fs::create_dir_all(receiver.join("evidence")).unwrap();
    let import = |bundle: &Bundle| {
        fs::write(
            receiver.join("evidence/ci.json"),
            serde_json::to_string(bundle).unwrap(),
        )
        .unwrap();
    };
    import(&bundle);
    assert_eq!(evaluate(&receiver)["established"], 0);
    approve(&receiver, &bundle);
    assert_eq!(evaluate(&receiver)["established"], 1); // broader claims remain fiat, unapproved

    // Negative transport scenarios use modified copies of the actual bundle.
    for outcome in [ResultKind::Fail, ResultKind::Skip, ResultKind::Cancelled] {
        bundle.targets[1].results[0].result = outcome;
        import(&bundle);
        let report = evaluate(&receiver);
        assert_eq!(report["established"], 0);
        assert!(report["blame"].to_string().contains("evidence_not_passed"));
    }
    bundle.targets[1].results[0].result = ResultKind::Pass;
    bundle.targets[1]
        .content
        .push_str("\n// synthetic next contract version");
    bundle.targets[1].interface_hash = bundle.targets[1].hash(&bundle.source.id).to_string();
    import(&bundle);
    assert!(
        evaluate(&receiver)["blame"]
            .to_string()
            .contains("stale_approval")
    );
    approve(&receiver, &bundle);
    assert_eq!(evaluate(&receiver)["established"], 1);
    fs::write(receiver.join("evidence/ci.json"), "not JSON").unwrap();
    Command::cargo_bin("inka")
        .unwrap()
        .args(["eval", "--json"])
        .arg(&receiver)
        .assert()
        .failure()
        .stdout("");
    fs::remove_dir_all(work).unwrap();
}
