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
