//! Parses the checked-in `inka.toml` project files and asserts their content.

use std::fs;
use std::path::Path;

use inka_core::manifest::parse_manifest;

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
    assert_eq!(
        manifest.evidence, None,
        "dogfood imports no test evidence yet"
    );
}

/// A minimal manifest body the evidence tests extend.
const MINIMAL: &str = "[project]\nname = \"p\"\n\n[phases]\norder = [\"M0\"]\ncurrent = \"M0\"\n\n[values]\nfiles = []\n";

#[test]
fn parses_an_evidence_section_with_manifest_relative_files() {
    // Arrange
    let text = format!("{MINIMAL}\n[evidence]\nfiles = [\"evidence/tests.json\"]\n");

    // Act
    let manifest = parse_manifest(&text).expect("an evidence section should parse");

    // Assert
    let evidence = manifest.evidence.expect("the section should be present");
    assert_eq!(evidence.files, [Path::new("evidence/tests.json")]);
}

#[test]
fn rejects_unknown_keys_in_the_evidence_section() {
    // Arrange: a typo'd key must fail loudly, like everywhere else in the manifest.
    let text = format!("{MINIMAL}\n[evidence]\nfile = [\"evidence/tests.json\"]\n");

    // Act & Assert
    assert!(
        parse_manifest(&text).is_err(),
        "`file` is not a known evidence key"
    );
}
