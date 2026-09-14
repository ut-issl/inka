//! Generation must bind results to prepared content without executing tests.

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use assert_cmd::Command;
use inka_core::evidence::Bundle;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

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
                "test-export",
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
                "test-export",
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
        .args(["test-export", "validate"])
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
            .args(["test-export", "validate"])
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

fn automatic_vectors() -> Vec<Value> {
    serde_json::from_str(include_str!(
        "../../../tests/golden/evidence/v2/automatic-catalog.json"
    ))
    .unwrap()
}

fn extracted_fixture(vector: &Value) -> Fixture {
    let mut fixture = Fixture::new();
    fixture.vector = vector["bundle"].clone();
    let target = &fixture.vector["targets"][0];
    // The collector has extracted a function from a larger source file.
    let source = format!(
        "# fixture/helper outside the contract\n{}",
        target["content"].as_str().unwrap()
    );
    fs::write(fixture.path.join("source.rs"), &source).unwrap();
    let plan = json!({
        "v": 1,
        "source": fixture.vector["source"],
        "execution": fixture.vector["execution"],
        "targets": [{
            "id": target["id"], "kind": target["kind"], "profile": target["profile"],
            "content": target["content"], "required": target["required"],
            "source_guards": [{"path": "source.rs", "sha256": format!("sha256:{:x}", Sha256::digest(source.as_bytes()))}]
        }]
    });
    fs::write(fixture.path.join("plan.json"), plan.to_string()).unwrap();
    fixture
}

#[test]
fn collector_supplied_functions_reproduce_all_automatic_catalog_vectors() {
    for vector in automatic_vectors() {
        let fixture = extracted_fixture(&vector);
        fixture.prepare();
        fixture.finish(&fixture.results(), vector["status"] == "pass");
        let output: Value =
            serde_json::from_str(&fs::read_to_string(fixture.path.join("bundle.json")).unwrap())
                .unwrap();
        assert_eq!(output, fixture.vector, "{}", vector["name"]);
    }
}

#[test]
fn extracted_content_requires_the_same_source_at_extraction_preparation_and_finish() {
    for mutation in [
        "before",
        "after",
        "missing-guard",
        "wrong-guard",
        "unknown-field",
        "mixed-input",
    ] {
        let fixture = extracted_fixture(&automatic_vectors()[0]);
        if mutation == "after" {
            fixture.prepare();
            fs::write(fixture.path.join("source.rs"), "changed after prepare").unwrap();
            fixture.finish(&fixture.results(), false);
            assert!(!fixture.path.join("bundle.json").exists());
            continue;
        }
        let path = fixture.path.join("plan.json");
        let mut plan: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        match mutation {
            "before" => {
                fs::write(fixture.path.join("source.rs"), "changed since extraction").unwrap();
            }
            "missing-guard" => plan["targets"][0]["source_guards"] = json!([]),
            "wrong-guard" => {
                plan["targets"][0]["source_guards"][0]["sha256"] = "sha256:wrong".into();
            }
            "unknown-field" => plan["targets"][0]["ignored"] = true.into(),
            _ => plan["targets"][0]["file"] = "source.rs".into(),
        }
        fs::write(&path, plan.to_string()).unwrap();
        Command::cargo_bin("inka")
            .unwrap()
            .current_dir(&fixture.path)
            .args([
                "test-export",
                "prepare",
                "--plan",
                "plan.json",
                "--out",
                "receipt.json",
            ])
            .assert()
            .failure();
        assert!(!fixture.path.join("receipt.json").exists(), "{mutation}");
    }
}

#[test]
fn validate_obeys_all_shared_vectors_without_requiring_success() {
    let fixture = Fixture::new();
    let mut vectors: Vec<Value> = serde_json::from_str(include_str!(
        "../../../tests/golden/evidence/v2/contracts.json"
    ))
    .unwrap();
    vectors.extend(automatic_vectors());
    for vector in vectors {
        fs::write(
            fixture.path.join("bundle.json"),
            vector["bundle"].to_string(),
        )
        .unwrap();
        let assertion = Command::cargo_bin("inka")
            .unwrap()
            .current_dir(&fixture.path)
            .args(["test-export", "validate", "bundle.json"])
            .assert();
        if vector["valid"] == true {
            assertion.success();
        } else {
            assertion.failure();
        }
    }
}
