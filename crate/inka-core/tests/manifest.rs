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
}
