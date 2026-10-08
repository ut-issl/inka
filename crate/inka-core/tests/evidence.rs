//! Behavior of the test evidence consumer against the v1 contract.
//!
//! The contract is `docs/en/design/test-evidence.md` with the wire format in
//! `schemas/test-evidence-v1.schema.json`: strict loading of configured
//! files, semantic checks the schema cannot express, and the interface hash
//! each bind approval is bound to. Loading failures are loud input errors;
//! a test-id absent from valid evidence is evaluation's "missing evidence",
//! which simply reads as `None` here.

use std::fs;
use std::path::{Path, PathBuf};

use inka_core::evidence::{self, EvidenceError, EvidenceSet, Outcome};
use proptest::prelude::*;
use serde::Deserialize;

mod common;
use common::golden_dir;

/// A scratch directory unique to one test, cleaned up on drop.
struct Scratch {
    dir: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("inka-evidence-test-{}-{name}", std::process::id()));
        fs::create_dir_all(&dir).expect("the scratch directory should be creatable");
        Self { dir }
    }

    fn write(&self, file: &str, contents: &str) -> PathBuf {
        let path = self.dir.join(file);
        fs::write(&path, contents).expect("the fixture file should be writable");
        PathBuf::from(file)
    }

    fn load(&self, files: &[PathBuf]) -> Result<EvidenceSet, EvidenceError> {
        evidence::load_files(&self.dir, files)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// A minimal valid evidence file with one passing test.
const VALID: &str = r##"{
  "v": 1,
  "provenance": {
    "source_revision": "4f2a1c9",
    "capsula_run_id": "01K8WSYC91YAE21R7CWHQ4KYN2",
    "an_unknown_key": {"nested": true}
  },
  "tests": [
    {
      "id": "example-repo/tests/hash_props.rs::roundtrip",
      "body": "#[test]\nfn roundtrip() {}\n",
      "executions": [{ "case": "default", "outcome": "pass" }]
    }
  ]
}"##;

#[test]
fn loads_a_valid_file_and_exposes_the_test_by_id() {
    // Arrange
    let scratch = Scratch::new("valid");
    let file = scratch.write("evidence.json", VALID);

    // Act
    let set = scratch.load(&[file]).expect("valid evidence should load");

    // Assert
    let test = set
        .get("example-repo/tests/hash_props.rs::roundtrip")
        .expect("the loaded test should be addressable by its id");
    assert_eq!(test.body, "#[test]\nfn roundtrip() {}\n");
    assert_eq!(test.executions.len(), 1);
    assert!(test.establishes());
    assert!(set.get("not-a-known-id").is_none());
}

#[test]
fn provenance_is_optional_and_its_unknown_keys_are_ignored() {
    let scratch = Scratch::new("no-provenance");
    let file = scratch.write(
        "evidence.json",
        r#"{"v": 1, "tests": [{"id": "t", "body": "b", "executions": [{"case": "e"}]}]}"#,
    );

    let set = scratch
        .load(&[file])
        .expect("provenance should be optional");

    assert_eq!(set.iter().count(), 1);
}

#[test]
fn establishes_only_when_every_required_execution_passes() {
    // Arrange: one file with every nonpass shape next to an all-pass test.
    let scratch = Scratch::new("outcomes");
    let file = scratch.write(
        "evidence.json",
        r#"{
          "v": 1,
          "tests": [
            {"id": "all-pass", "body": "b", "executions": [
              {"case": "a", "outcome": "pass"}, {"case": "b", "outcome": "pass"}]},
            {"id": "one-fail", "body": "b", "executions": [
              {"case": "a", "outcome": "pass"}, {"case": "b", "outcome": "fail"}]},
            {"id": "one-skip", "body": "b", "executions": [
              {"case": "a", "outcome": "pass"}, {"case": "b", "outcome": "skip"}]},
            {"id": "one-cancelled", "body": "b", "executions": [
              {"case": "a", "outcome": "pass"}, {"case": "b", "outcome": "cancelled"}]},
            {"id": "one-missing", "body": "b", "executions": [
              {"case": "a", "outcome": "pass"}, {"case": "b"}]}
          ]
        }"#,
    );

    // Act
    let set = scratch.load(&[file]).expect("the file should load");

    // Assert
    assert!(set.get("all-pass").expect("loaded").establishes());
    for id in ["one-fail", "one-skip", "one-cancelled", "one-missing"] {
        assert!(
            !set.get(id).expect("loaded").establishes(),
            "`{id}` must not establish a bind"
        );
    }
    let missing = &set.get("one-missing").expect("loaded").executions;
    assert_eq!(
        missing[1].outcome, None,
        "a declared execution without an outcome is a missing result"
    );
    assert_eq!(
        set.get("one-fail").expect("loaded").executions[1].outcome,
        Some(Outcome::Fail)
    );
}

#[test]
fn a_missing_configured_file_is_an_input_error() {
    let scratch = Scratch::new("missing-file");

    let error = scratch
        .load(&[PathBuf::from("not-there.json")])
        .expect_err("a configured file that is absent breaks the delivery premise");

    assert!(
        matches!(error, EvidenceError::Unreadable { .. }),
        "got: {error}"
    );
}

#[test]
fn malformed_json_is_an_input_error_distinct_from_unreadable() {
    let scratch = Scratch::new("malformed");
    let file = scratch.write("evidence.json", "{ not json");

    let error = scratch.load(&[file]).expect_err("must fail loudly");

    assert!(
        matches!(error, EvidenceError::Malformed { .. }),
        "got: {error}"
    );
}

#[test]
fn an_unsupported_version_is_rejected_by_name() {
    let scratch = Scratch::new("version");
    let file = scratch.write("evidence.json", r#"{"v": 2, "tests": []}"#);

    let error = scratch
        .load(&[file])
        .expect_err("v2 is not this consumer's format");

    assert!(
        matches!(error, EvidenceError::UnsupportedVersion { found: 2, .. }),
        "got: {error}"
    );
}

#[test]
fn unknown_fields_outside_provenance_are_rejected() {
    let scratch = Scratch::new("unknown-field");
    let file = scratch.write(
        "evidence.json",
        r#"{"v": 1, "tests": [{"id": "t", "body": "b", "interface_hash": "sha256:00",
            "executions": [{"case": "e"}]}]}"#,
    );

    let error = scratch
        .load(&[file])
        .expect_err("a declared digest is not part of v1");

    assert!(
        matches!(error, EvidenceError::Malformed { .. }),
        "got: {error}"
    );
}

#[test]
fn blank_identifiers_and_empty_shapes_are_rejected() {
    let scratch = Scratch::new("blanks");
    let cases = [
        (
            "blank-test-id.json",
            r#"{"v": 1, "tests": [{"id": "  ", "body": "b", "executions": [{"case": "e"}]}]}"#,
        ),
        (
            "empty-body.json",
            r#"{"v": 1, "tests": [{"id": "t", "body": "", "executions": [{"case": "e"}]}]}"#,
        ),
        (
            "no-executions.json",
            r#"{"v": 1, "tests": [{"id": "t", "body": "b", "executions": []}]}"#,
        ),
        (
            "blank-case.json",
            r#"{"v": 1, "tests": [{"id": "t", "body": "b", "executions": [{"case": " "}]}]}"#,
        ),
    ];

    for (file, contents) in cases {
        let relative = scratch.write(file, contents);
        let error = scratch.load(&[relative]).expect_err(file);
        assert!(
            matches!(error, EvidenceError::Invalid { .. }),
            "`{file}` should be a semantic input error, got: {error}"
        );
    }
}

#[test]
fn duplicate_cases_within_a_test_are_rejected() {
    let scratch = Scratch::new("dup-execution");
    let file = scratch.write(
        "evidence.json",
        r#"{"v": 1, "tests": [{"id": "t", "body": "b",
            "executions": [{"case": "e", "outcome": "pass"}, {"case": "e", "outcome": "fail"}]}]}"#,
    );

    let error = scratch
        .load(&[file])
        .expect_err("conflicting records for one execution");

    assert!(
        matches!(error, EvidenceError::Invalid { .. }),
        "got: {error}"
    );
}

#[test]
fn duplicate_test_ids_are_rejected_within_and_across_files() {
    // Arrange: the same test-id twice in one file, and once in each of two files.
    let scratch = Scratch::new("dup-test");
    let twice = scratch.write(
        "twice.json",
        r#"{"v": 1, "tests": [
            {"id": "t", "body": "b", "executions": [{"case": "e"}]},
            {"id": "t", "body": "b", "executions": [{"case": "e"}]}]}"#,
    );
    let first = scratch.write(
        "first.json",
        r#"{"v": 1, "tests": [{"id": "t", "body": "b", "executions": [{"case": "e"}]}]}"#,
    );
    let second = scratch.write(
        "second.json",
        r#"{"v": 1, "tests": [{"id": "t", "body": "other", "executions": [{"case": "e"}]}]}"#,
    );

    // Act & Assert
    let within = scratch.load(&[twice]).expect_err("a duplicate in one file");
    assert!(
        matches!(within, EvidenceError::Invalid { .. }),
        "got: {within}"
    );
    let across = scratch
        .load(&[first, second])
        .expect_err("a duplicate across files");
    assert!(
        matches!(across, EvidenceError::Invalid { .. }),
        "got: {across}"
    );
}

#[test]
fn the_interface_hash_ignores_execution_order_and_crlf_but_nothing_else() {
    // Arrange
    let lf = evidence::interface_hash("t", "a\nb", &["x", "y"]);

    // Act & Assert: reordering the required set and CRLF line endings are not
    // edits; a lone CR, the id, the body and the required set itself are.
    assert_eq!(evidence::interface_hash("t", "a\nb", &["y", "x"]), lf);
    assert_eq!(evidence::interface_hash("t", "a\r\nb", &["x", "y"]), lf);
    assert_ne!(evidence::interface_hash("t", "a\rb", &["x", "y"]), lf);
    assert_ne!(evidence::interface_hash("u", "a\nb", &["x", "y"]), lf);
    assert_ne!(evidence::interface_hash("t", "a\nb ", &["x", "y"]), lf);
    assert_ne!(evidence::interface_hash("t", "a\nb", &["x"]), lf);
    assert_ne!(evidence::interface_hash("t", "a\nb", &["x", "y", "z"]), lf);
}

proptest! {
    /// Reordering the declared executions never changes the interface hash.
    #[test]
    fn hash_is_invariant_under_execution_reordering(
        test_id in "\\PC{1,20}",
        body in "\\PC{1,40}",
        mut ids in proptest::collection::vec("[a-z]{1,8}", 1..5),
    ) {
        let forward: Vec<&str> = ids.iter().map(String::as_str).collect();
        let before = evidence::interface_hash(&test_id, &body, &forward);
        ids.reverse();
        let reversed: Vec<&str> = ids.iter().map(String::as_str).collect();
        prop_assert_eq!(evidence::interface_hash(&test_id, &body, &reversed), before);
    }

    /// CRLF-only normalization: replacing every LF with CRLF hashes the same.
    #[test]
    fn hash_is_invariant_under_crlf_line_endings(
        test_id in "\\PC{1,20}",
        lines in proptest::collection::vec("[a-zA-Z0-9 ]{0,10}", 1..5),
    ) {
        let lf = lines.join("\n");
        let crlf = lines.join("\r\n");
        prop_assert_eq!(
            evidence::interface_hash(&test_id, &crlf, &["e"]),
            evidence::interface_hash(&test_id, &lf, &["e"])
        );
    }
}

/// One frozen test-evidence interface vector.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Vector {
    /// Why this vector exists, for the reader of the file.
    #[allow(dead_code)]
    comment: Option<String>,
    test_id: String,
    body: String,
    /// What `body` normalizes to before hashing (CRLF→LF only).
    normalized_body: String,
    executions: Vec<String>,
    interface_hash: String,
}

fn vector_files() -> Vec<PathBuf> {
    let dir = golden_dir().join("hash/test-evidence-v1");
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .expect("the test-evidence golden directory should exist")
        .map(|entry| {
            entry
                .expect("the golden directory should be readable")
                .path()
        })
        .filter(|path| path.extension().is_some_and(|e| e == "toml"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "no vectors in {}", dir.display());
    files
}

fn read_vector(path: &Path) -> Vector {
    let text = fs::read_to_string(path).expect("a golden vector should be readable");
    toml::from_str(&text)
        .unwrap_or_else(|error| panic!("{} is not a valid vector: {error}", path.display()))
}

#[test]
fn every_recorded_test_evidence_vector_still_hashes_to_its_recorded_value() {
    for path in vector_files() {
        let vector = read_vector(&path);
        let ids: Vec<&str> = vector.executions.iter().map(String::as_str).collect();

        let recomputed = evidence::interface_hash(&vector.test_id, &vector.body, &ids);

        assert_eq!(
            recomputed.to_string(),
            vector.interface_hash,
            "{} no longer hashes to its frozen value",
            path.display()
        );
        assert_eq!(
            inka_core::normalize::evidence_body(&vector.body),
            vector.normalized_body,
            "{} records a stale normalized body",
            path.display()
        );
    }
}
