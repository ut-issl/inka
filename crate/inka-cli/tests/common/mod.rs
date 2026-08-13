//! Shared helpers for the CLI integration-test binaries.
//!
//! Two fixture roots are in play: the workspace fixtures (`dogfood`, `aocs`)
//! that every crate shares, and the CLI's own broken fixtures used to exercise
//! diagnostic rendering and exit codes.

use std::path::{Path, PathBuf};

/// Directory of a shared workspace fixture, such as `dogfood` or `aocs`.
pub fn workspace_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures")
        .join(name)
}

/// Directory of a CLI-local fixture under `tests/fixtures/`.
pub fn local_fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}
