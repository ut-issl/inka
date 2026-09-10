//! Shared helpers for the integration-test binaries.
//!
//! Each test binary compiles this module independently, so a helper unused
//! by one binary is expected.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

/// Root of the checked-in fixture projects.
pub fn fixtures_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures")
}

/// Directory of one named fixture project.
pub fn fixture_dir(name: &str) -> PathBuf {
    fixtures_root().join(name)
}

/// Root of the frozen golden vectors, which are append-only.
pub fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden")
}
