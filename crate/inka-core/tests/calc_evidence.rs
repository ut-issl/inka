//! Replays the frozen calc-evidence interface hash vectors.
//!
//! A `calc:` bind approval names a view hash that covers the interface hash
//! computed here, so the layout under `tests/golden/hash/calc-evidence-v1/`
//! is part of every such approval's meaning. The directory is append-only; a
//! failing vector is never fixed by editing it.

use std::fs;
use std::path::{Path, PathBuf};

use inka_core::calc_evidence::{self, Definition};
use inka_core::normalize;
use serde::Deserialize;

mod common;
use common::golden_dir;

/// One frozen vector: the producer's output and the hash it reduces to.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Vector {
    #[allow(dead_code)]
    comment: String,
    target: String,
    /// The definitions in the order the producer listed them.
    definitions: Vec<DefinitionVector>,
    /// The required leaf IDs in the order the producer listed them.
    leaves: Vec<String>,
    interface_hash: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DefinitionVector {
    path: String,
    source: String,
    /// What `source` normalizes to, recorded so a reader sees the pipeline.
    normalized_source: String,
}

fn vector_files() -> Vec<PathBuf> {
    let dir = golden_dir().join("hash/calc-evidence-v1");
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .unwrap_or_else(|error| panic!("{} should be readable: {error}", dir.display()))
        .map(|entry| entry.expect("a directory entry should be readable").path())
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
fn every_recorded_calc_evidence_vector_still_hashes_to_its_recorded_value() {
    for path in vector_files() {
        // Arrange
        let vector = read_vector(&path);
        let definitions: Vec<Definition<'_>> = vector
            .definitions
            .iter()
            .map(|definition| Definition {
                path: &definition.path,
                source: &definition.source,
            })
            .collect();
        let leaves: Vec<&str> = vector.leaves.iter().map(String::as_str).collect();

        // Act
        let recomputed = calc_evidence::interface_hash(&vector.target, &definitions, &leaves);

        // Assert
        for definition in &vector.definitions {
            assert_eq!(
                normalize::evidence_body(&definition.source),
                definition.normalized_source,
                "{} records a stale normalized source for `{}`",
                path.display(),
                definition.path
            );
        }
        assert_eq!(
            recomputed.to_string(),
            vector.interface_hash,
            "{} no longer hashes to its frozen value",
            path.display()
        );
    }
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

use inka_core::calc_evidence::{CalcEvidenceError, CalcEvidenceSet, DefinitionKind};
use inka_core::read::DiskText;

/// A scratch directory unique to one test, cleaned up on drop.
struct Scratch {
    dir: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "inka-calc-evidence-test-{}-{name}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("the scratch directory should be creatable");
        Self { dir }
    }

    fn write(&self, file: &str, contents: &str) -> PathBuf {
        fs::write(self.dir.join(file), contents).expect("the fixture file should be writable");
        PathBuf::from(file)
    }

    fn load(&self, files: &[PathBuf]) -> Result<CalcEvidenceSet, CalcEvidenceError> {
        calc_evidence::load_files(&self.dir, files, &DiskText)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// A valid file: a verification over one calculation, assuming two nodes.
const VALID: &str = r#"{
  "v": 1,
  "provenance": {"source_revision": "4f2a1c9", "capsula_run_id": "01K8WSYC91YAE21R7CWHQ4KYN2"},
  "targets": [
    {
      "path": "Power::?verify_margin",
      "definitions": [
        {"path": "Power::?verify_margin", "kind": "verification",
         "source": "@power.verification()\ndef verify_margin(b: Budget) -> bool:\n    return b.margin > 0\n"},
        {"path": "Power::@budget", "kind": "calculation",
         "source": "@power.calculation()\ndef budget(c: Config) -> Budget: ...\n"}
      ],
      "assumes": ["power::loads_complete", "power::eclipse_model_valid"],
      "leaves": [{"id": "Power::?verify_margin", "result": true}],
      "valid": true
    }
  ]
}"#;

#[test]
fn loads_a_valid_file_and_exposes_the_target_by_path() {
    // Arrange
    let scratch = Scratch::new("valid");
    let file = scratch.write("calc-evidence.json", VALID);

    // Act
    let set = scratch.load(&[file]).expect("valid evidence should load");

    // Assert: everything the wire carried, plus the hash the loader adds.
    let target = set
        .get("Power::?verify_margin")
        .expect("the loaded target should be addressable by its path");
    assert_eq!(target.definitions.len(), 2);
    assert_eq!(target.definitions[1].path, "Power::@budget");
    assert_eq!(target.definitions[1].kind, DefinitionKind::Calculation);
    assert_eq!(
        target.assumes,
        ["power::loads_complete", "power::eclipse_model_valid"]
    );
    assert_eq!(target.leaves.len(), 1);
    assert_eq!(target.leaves[0].result, Some(true));
    assert!(target.valid);
    assert!(!target.xfail);
    let definitions: Vec<Definition<'_>> = target
        .definitions
        .iter()
        .map(|definition| Definition {
            path: &definition.path,
            source: &definition.source,
        })
        .collect();
    assert_eq!(
        target.interface_hash,
        calc_evidence::interface_hash(
            "Power::?verify_margin",
            &definitions,
            &["Power::?verify_margin"]
        )
    );
    assert!(set.get("Power::?verify_other").is_none());
    assert_eq!(set.iter().count(), 1);
}

#[test]
fn a_loaded_target_hashes_to_its_golden_vector() {
    // Arrange: each frozen vector, written out as the evidence file a
    // producer would deliver. The loader must reduce it to the recorded hash.
    let scratch = Scratch::new("golden");
    for path in vector_files() {
        let vector = read_vector(&path);
        let definitions: Vec<serde_json::Value> = vector
            .definitions
            .iter()
            .map(|definition| {
                serde_json::json!({
                    "path": definition.path,
                    "kind": "calculation",
                    "source": definition.source,
                })
            })
            .collect();
        let leaves: Vec<serde_json::Value> = vector
            .leaves
            .iter()
            .map(|leaf| serde_json::json!({"id": leaf, "result": true}))
            .collect();
        let file = serde_json::json!({
            "v": 1,
            "targets": [{
                "path": vector.target,
                "definitions": definitions,
                "assumes": [],
                "leaves": leaves,
                "valid": true,
            }],
        });
        let name = format!(
            "{}.json",
            path.file_stem()
                .expect("a vector file has a stem")
                .to_string_lossy()
        );
        let relative = scratch.write(&name, &file.to_string());

        // Act
        let set = scratch
            .load(&[relative])
            .expect("a vector is valid evidence");

        // Assert
        assert_eq!(
            set.get(&vector.target)
                .expect("the target is loaded")
                .interface_hash
                .to_string(),
            vector.interface_hash,
            "{} does not load to its frozen hash",
            path.display()
        );
    }
}

#[test]
fn a_missing_configured_file_is_an_input_error() {
    let scratch = Scratch::new("missing-file");

    let error = scratch
        .load(&[PathBuf::from("not-there.json")])
        .expect_err("a configured file that is absent breaks the delivery premise");

    assert!(
        matches!(error, CalcEvidenceError::Unreadable { .. }),
        "got: {error}"
    );
    assert!(error.to_string().contains("not-there.json"));
}

#[test]
fn malformed_json_and_an_unsupported_version_are_distinct_input_errors() {
    let scratch = Scratch::new("malformed");
    let malformed = scratch.write("malformed.json", "{ not json");
    let versioned = scratch.write("v2.json", r#"{"v": 2, "targets": []}"#);

    let malformed = scratch.load(&[malformed]).expect_err("must fail loudly");
    let versioned = scratch
        .load(&[versioned])
        .expect_err("v2 is not this consumer's format");

    assert!(
        matches!(malformed, CalcEvidenceError::Malformed { .. }),
        "got: {malformed}"
    );
    assert!(
        matches!(
            versioned,
            CalcEvidenceError::UnsupportedVersion { found: 2, .. }
        ),
        "got: {versioned}"
    );
}

#[test]
fn semantic_defects_are_input_errors_naming_the_file() {
    let scratch = Scratch::new("semantic");
    // The target is not among its own definitions.
    let file = scratch.write(
        "undefined-target.json",
        r#"{"v": 1, "targets": [{"path": "P::?v",
            "definitions": [{"path": "P::@c", "kind": "calculation", "source": "x"}],
            "assumes": [], "leaves": [{"id": "P::?v"}], "valid": true}]}"#,
    );

    let error = scratch
        .load(&[file])
        .expect_err("the target must be defined");

    assert!(
        matches!(error, CalcEvidenceError::Invalid { .. }),
        "got: {error}"
    );
    assert!(error.to_string().contains("undefined-target.json"));
}

#[test]
fn duplicate_target_paths_are_rejected_across_files() {
    // Arrange: the same target delivered by two files is a routing conflict.
    let scratch = Scratch::new("dup-target");
    let one = r#"{"v": 1, "targets": [{"path": "P::?v",
        "definitions": [{"path": "P::?v", "kind": "verification", "source": "x"}],
        "assumes": [], "leaves": [{"id": "P::?v"}], "valid": true}]}"#;
    let first = scratch.write("first.json", one);
    let second = scratch.write("second.json", one);

    // Act
    let error = scratch
        .load(&[first, second])
        .expect_err("a duplicate across files");

    // Assert: the second file is the one reported.
    assert!(
        matches!(error, CalcEvidenceError::Invalid { .. }),
        "got: {error}"
    );
    assert!(error.to_string().contains("second.json"), "got: {error}");
}
