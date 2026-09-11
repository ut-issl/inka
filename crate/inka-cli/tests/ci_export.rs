//! Generation must bind results to prepared content without executing tests.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use assert_cmd::Command;
use inka_core::evidence::Bundle;
use serde_json::{Value, json};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture {
    path: PathBuf,
    vector: Value,
}
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "inka-producer-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        let vectors: Vec<Value> = serde_json::from_str(include_str!(
            "../../../tests/golden/evidence/v2/contracts.json"
        ))
        .unwrap();
        let vector = vectors[0]["bundle"].clone();
        fs::write(
            path.join("source.rs"),
            vector["targets"][0]["content"].as_str().unwrap(),
        )
        .unwrap();
        let plan = json!({"v":1,"source":vector["source"],"execution":vector["execution"],"targets":[{"id":vector["targets"][0]["id"],"file":"source.rs","required":vector["targets"][0]["required"]}]});
        fs::write(path.join("plan.json"), plan.to_string()).unwrap();
        Self { path, vector }
    }
    fn prepare(&self) {
        Command::cargo_bin("inka")
            .unwrap()
            .current_dir(&self.path)
            .args([
                "ci-export",
                "prepare",
                "--plan",
                "plan.json",
                "--out",
                "receipt.json",
            ])
            .assert()
            .success();
    }
    fn results(&self) -> Value {
        json!({"v":1,"source":self.vector["source"],"execution":self.vector["execution"],"targets":[{"id":self.vector["targets"][0]["id"],"interface_hash":self.vector["targets"][0]["interface_hash"],"results":self.vector["targets"][0]["results"]}]})
    }
    fn finish(&self, results: &Value, passed: bool) {
        fs::write(self.path.join("results.json"), results.to_string()).unwrap();
        let mut command = Command::cargo_bin("inka").unwrap();
        let assertion = command
            .current_dir(&self.path)
            .args([
                "ci-export",
                "finish",
                "--receipt",
                "receipt.json",
                "--results",
                "results.json",
                "--out",
                "bundle.json",
            ])
            .assert();
        if passed {
            assertion.success();
        } else {
            assertion.failure();
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[test]
fn prepare_and_finish_reproduce_the_independent_file_vector() {
    let fixture = Fixture::new();
    fixture.prepare();
    fixture.finish(&fixture.results(), true);
    let output: Value =
        serde_json::from_str(&fs::read_to_string(fixture.path.join("bundle.json")).unwrap())
            .unwrap();
    assert_eq!(output, fixture.vector);
    Command::cargo_bin("inka")
        .unwrap()
        .args(["ci-export", "validate"])
        .arg(fixture.path.join("bundle.json"))
        .assert()
        .success();
}

#[test]
fn failures_and_unreported_targets_are_published_but_finish_exits_nonzero() {
    for outcome in ["fail", "skip", "cancelled", "missing"] {
        let fixture = Fixture::new();
        fixture.prepare();
        let mut results = fixture.results();
        if outcome == "missing" {
            results["targets"] = json!([]);
        } else {
            results["targets"][0]["results"][0]["result"] = outcome.into();
        }
        fixture.finish(&results, false);
        let bundle =
            Bundle::parse(&fs::read_to_string(fixture.path.join("bundle.json")).unwrap()).unwrap();
        assert_eq!(bundle.targets[0].failures().len(), 1);
        Command::cargo_bin("inka")
            .unwrap()
            .args(["ci-export", "validate"])
            .arg(fixture.path.join("bundle.json"))
            .assert()
            .success();
    }
}

#[test]
fn changed_source_mismatched_attempt_and_unprepared_results_are_rejected() {
    for mutation in [
        "source",
        "revision",
        "attempt",
        "digest",
        "duplicate",
        "unknown",
        "case",
    ] {
        let fixture = Fixture::new();
        fixture.prepare();
        let mut results = fixture.results();
        match mutation {
            "source" => {
                fs::write(fixture.path.join("source.rs"), "changed after preparation").unwrap();
            }
            "revision" => results["source"]["revision"] = "another revision".into(),
            "attempt" => results["execution"]["attempt"] = 2.into(),
            "digest" => results["targets"][0]["interface_hash"] = "sha256:wrong".into(),
            "duplicate" => {
                results["targets"] = json!([results["targets"][0], results["targets"][0]]);
            }
            "unknown" => results["targets"][0]["id"] = "another target".into(),
            _ => results["targets"][0]["results"][0]["case"] = "not selected".into(),
        }
        fixture.finish(&results, false);
        assert!(!fixture.path.join("bundle.json").exists(), "{mutation}");
    }
}
