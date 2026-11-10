//! Parses the checked-in `inka.toml` project files and asserts their content.

use std::fs;
use std::path::{Path, PathBuf};

use inka_core::manifest::{ManifestError, parse_manifest};

mod common;

#[test]
fn dogfood_manifest_deserializes_into_manifest() {
    // Arrange
    let path = common::fixture_dir("dogfood").join("inka.toml");
    let text = fs::read_to_string(&path).expect("dogfood inka.toml should read");

    // Act
    let manifest = parse_manifest(&text).expect("dogfood inka.toml should parse");

    // Assert
    assert_eq!(manifest.project.name, "inka-dogfood");
    assert_eq!(
        manifest.phases.order,
        [
            "M0", "M1", "M2", "M3", "M4", "M5", "M6", "M7", "M8", "M9", "M10"
        ]
    );
    assert_eq!(manifest.phases.current, "M0");
    assert_eq!(manifest.values.files, [Path::new("values.toml")]);
    let evidence = manifest
        .evidence
        .expect("dogfood imports its capsula-delivered test evidence (#75)");
    assert_eq!(evidence.tests, [Path::new("evidence/test-evidence.json")]);
    assert_eq!(evidence.calcs, Vec::<PathBuf>::new());
}

/// A minimal manifest body the evidence tests extend.
const MINIMAL: &str = "[project]\nname = \"p\"\n\n[phases]\norder = [\"M0\"]\ncurrent = \"M0\"\n\n[values]\nfiles = []\n";

#[test]
fn parses_an_evidence_section_with_manifest_relative_files() {
    // Arrange
    let text = format!(
        "{MINIMAL}\n[evidence]\ntests = [\"evidence/tests.json\"]\ncalcs = [\"evidence/calcs.json\"]\n"
    );

    // Act
    let manifest = parse_manifest(&text).expect("an evidence section should parse");

    // Assert
    let evidence = manifest.evidence.expect("the section should be present");
    assert_eq!(evidence.tests, [Path::new("evidence/tests.json")]);
    assert_eq!(evidence.calcs, [Path::new("evidence/calcs.json")]);
}

#[test]
fn either_evidence_list_may_be_left_out() {
    // Arrange: a project with calc evidence only.
    let text = format!("{MINIMAL}\n[evidence]\ncalcs = [\"evidence/calcs.json\"]\n");

    // Act
    let manifest = parse_manifest(&text).expect("a calcs-only section should parse");

    // Assert
    let evidence = manifest.evidence.expect("the section should be present");
    assert_eq!(evidence.tests, Vec::<PathBuf>::new());
    assert_eq!(evidence.calcs, [Path::new("evidence/calcs.json")]);
}

#[test]
fn rejects_unknown_keys_in_the_evidence_section() {
    // Arrange: a typo'd key must fail loudly, like everywhere else in the manifest.
    let text = format!("{MINIMAL}\n[evidence]\ntest = [\"evidence/tests.json\"]\n");

    // Act
    let error = parse_manifest(&text).expect_err("`test` is not a known evidence key");

    // Assert: the message names the keys that exist.
    let message = error.to_string();
    assert!(message.contains("tests"), "got: {message}");
    assert!(message.contains("calcs"), "got: {message}");
}

#[test]
fn the_retired_evidence_files_key_is_reported_as_renamed_to_tests() {
    // Arrange: `files` was the only evidence key before calc evidence. A
    // manifest written then must be told what to change, not just "unknown".
    let text = format!("{MINIMAL}\n[evidence]\nfiles = [\"evidence/tests.json\"]\n");

    // Act
    let error = parse_manifest(&text).expect_err("`files` is no longer an evidence key");

    // Assert
    assert_eq!(error, ManifestError::RetiredEvidenceFiles);
    assert_eq!(
        error.to_string(),
        "`[evidence] files` is now `[evidence] tests`; rename the key \
         (calc evidence files go under `[evidence] calcs`)"
    );
}

#[test]
fn a_files_key_elsewhere_is_not_mistaken_for_the_retired_evidence_key() {
    // Arrange: `[values] files` is current, so an unrelated mistake must
    // keep its own message.
    let text = format!("{MINIMAL}\n[evidence]\ntsets = []\n");

    // Act
    let error = parse_manifest(&text).expect_err("`tsets` is a typo");

    // Assert
    assert!(matches!(error, ManifestError::Toml(_)), "got: {error}");
}
