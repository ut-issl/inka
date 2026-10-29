//! End-to-end tests for the inka-evidence-merge binary.

use std::path::{Path, PathBuf};

use inka_evidence::{EvidenceBuilder, Execution, Outcome};

/// Writes one single-test evidence file with one `case` execution and
/// returns its path.
fn evidence_file(name: &str, case: &str, outcome: Option<Outcome>) -> PathBuf {
    let mut builder = EvidenceBuilder::new();
    builder
        .provenance("source_revision", "abc123")
        .expect("nonblank");
    builder
        .add_test(
            "suite::test",
            "the body",
            vec![Execution::new(case, outcome)],
        )
        .expect("valid");
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::write(&path, builder.finish()).expect("the fixture file writes");
    path
}

fn merge_command() -> assert_cmd::Command {
    let mut command =
        assert_cmd::Command::cargo_bin("inka-evidence-merge").expect("the binary builds");
    command.env_remove("CAPSULA_RUN_ID");
    command
}

#[test]
fn merges_labeled_files_to_standard_output() {
    let linux = evidence_file("merge-linux.json", "default", Some(Outcome::Pass));
    let macos = evidence_file("merge-macos.json", "default", Some(Outcome::Skip));

    let assert = merge_command()
        .arg(format!("linux={}", linux.display()))
        .arg(format!("macos={}", macos.display()))
        .env("CAPSULA_RUN_ID", "run-9")
        .assert()
        .success();

    let text = String::from_utf8(assert.get_output().stdout.clone()).expect("UTF-8");
    let tests = inka_evidence::parse(&text).expect("the merge emits consumer-valid evidence");
    assert_eq!(tests.len(), 1);
    assert_eq!(
        tests[0].executions,
        vec![
            Execution::new("linux/default", Some(Outcome::Pass)),
            Execution::new("macos/default", Some(Outcome::Skip)),
        ]
    );
    let raw: serde_json::Value = serde_json::from_str(&text).expect("JSON");
    assert_eq!(raw["provenance"]["source_revision"], "abc123");
    assert_eq!(raw["provenance"]["capsula_run_id"], "run-9");
}

#[test]
fn writes_the_merged_file_and_takes_provenance_flags() {
    let linux = evidence_file("merge-out-linux.json", "linux", Some(Outcome::Pass));
    let macos = evidence_file("merge-out-macos.json", "macos", Some(Outcome::Pass));
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("merged.json");

    merge_command()
        .arg("--output")
        .arg(&out)
        .arg("--provenance")
        .arg("runner=matrix-ci")
        .arg(linux.display().to_string())
        .arg(macos.display().to_string())
        .assert()
        .success();

    let text = std::fs::read_to_string(&out).expect("the merged file was written");
    let raw: serde_json::Value = serde_json::from_str(&text).expect("JSON");
    assert_eq!(raw["provenance"]["runner"], "matrix-ci");
    assert_eq!(raw["tests"][0]["executions"][1]["case"], "macos");
}

#[test]
fn a_case_collision_fails_explicitly() {
    let one = evidence_file("merge-dup-one.json", "default", Some(Outcome::Pass));
    let two = evidence_file("merge-dup-two.json", "default", Some(Outcome::Pass));

    merge_command()
        .arg(one.display().to_string())
        .arg(two.display().to_string())
        .assert()
        .failure()
        .stderr(predicates::str::contains("more than one input"));
}

#[test]
fn a_blank_label_fails_explicitly() {
    let file = evidence_file("merge-blank-label.json", "default", Some(Outcome::Pass));

    merge_command()
        .arg(format!("={}", file.display()))
        .assert()
        .failure()
        .stderr(predicates::str::contains("has a blank label"));
}

#[test]
fn an_empty_file_name_fails_explicitly() {
    merge_command()
        .arg("linux=")
        .assert()
        .failure()
        .stderr(predicates::str::contains("empty file name"));
}

#[test]
fn a_malformed_provenance_entry_is_rejected() {
    merge_command()
        .arg("--provenance")
        .arg("nokey")
        .arg("whatever.json")
        .assert()
        .failure()
        .stderr(predicates::str::contains("key=value"));
}

#[test]
fn a_body_mismatch_fails_explicitly() {
    let mut builder = EvidenceBuilder::new();
    builder
        .add_test(
            "suite::test",
            "another body",
            vec![Execution::new("macos", None)],
        )
        .expect("valid");
    let other = Path::new(env!("CARGO_TARGET_TMPDIR")).join("merge-other-body.json");
    std::fs::write(&other, builder.finish()).expect("writes");
    let one = evidence_file("merge-body-one.json", "linux", Some(Outcome::Pass));

    merge_command()
        .arg(one.display().to_string())
        .arg(other.display().to_string())
        .assert()
        .failure()
        .stderr(predicates::str::contains("different bodies"));
}

#[test]
fn a_missing_input_file_is_a_clear_error() {
    merge_command()
        .arg("linux=does-not-exist.json")
        .assert()
        .failure()
        .stderr(predicates::str::contains("cannot read does-not-exist.json"));
}
