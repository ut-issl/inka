//! Locating each suite's root source file through `cargo metadata`.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Deserialize;

use crate::cli::Cli;
use crate::error::Error;

/// The workspace's packages and targets, for mapping a nextest suite to
/// the root file body extraction starts from.
#[derive(Debug)]
pub struct Workspace {
    packages: Vec<Package>,
}

/// Runs `cargo metadata --no-deps` for the workspace under test.
pub fn load(cli: &Cli) -> Result<Workspace, Error> {
    let mut command = Command::new("cargo");
    command.args(["metadata", "--format-version", "1", "--no-deps"]);
    if let Some(path) = &cli.manifest_path {
        command.arg("--manifest-path").arg(path);
    }
    command.stdout(Stdio::piped()).stderr(Stdio::inherit());
    let output = command.output().map_err(|source| Error::Spawn {
        command: String::from("cargo metadata"),
        source,
    })?;
    if !output.status.success() {
        return Err(Error::CommandFailed {
            command: String::from("cargo metadata"),
            status: output.status,
        });
    }
    let metadata: Metadata =
        serde_json::from_slice(&output.stdout).map_err(|source| Error::Format {
            what: "cargo metadata",
            source,
        })?;
    Ok(Workspace {
        packages: metadata.packages,
    })
}

impl Workspace {
    /// The root source file of the cargo target a nextest suite was built
    /// from. `kind` is nextest's suite kind (`lib`, `test`, `bin`, ...),
    /// matched against cargo's target kinds.
    pub fn root_file(
        &self,
        binary_id: &str,
        package_id: &str,
        kind: &str,
        binary_name: &str,
    ) -> Result<&Path, Error> {
        self.packages
            .iter()
            .filter(|package| package.id == package_id)
            .flat_map(|package| &package.targets)
            .find(|target| {
                names_match(&target.name, binary_name) && kind_matches(kind, &target.kind)
            })
            .map(|target| target.src_path.as_path())
            .ok_or_else(|| Error::TargetNotFound {
                binary_id: binary_id.to_owned(),
                kind: kind.to_owned(),
                name: binary_name.to_owned(),
            })
    }
}

/// Target names compare with `-` and `_` unified: cargo keeps the dashed
/// name while the built binary is underscored.
fn names_match(target: &str, binary: &str) -> bool {
    target.len() == binary.len()
        && target
            .chars()
            .zip(binary.chars())
            .all(|(a, b)| a == b || (a == '-' || a == '_') && (b == '-' || b == '_'))
}

/// Maps nextest's suite kind onto cargo's target kinds. A library suite
/// may be built from any lib-flavored target; the other kinds are named
/// identically on both sides.
fn kind_matches(nextest_kind: &str, cargo_kinds: &[String]) -> bool {
    match nextest_kind {
        "lib" => cargo_kinds.iter().any(|kind| {
            matches!(
                kind.as_str(),
                "lib" | "rlib" | "dylib" | "cdylib" | "staticlib"
            )
        }),
        other => cargo_kinds.iter().any(|kind| kind == other),
    }
}

/// The subset of `cargo metadata` this adapter reads.
#[derive(Debug, Deserialize)]
struct Metadata {
    packages: Vec<Package>,
}

#[derive(Debug, Deserialize)]
struct Package {
    id: String,
    targets: Vec<Target>,
}

#[derive(Debug, Deserialize)]
struct Target {
    name: String,
    kind: Vec<String>,
    src_path: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace() -> Workspace {
        let raw = r#"{
            "packages": [{
                "id": "path+file:///work#sample@0.1.0",
                "name": "sample",
                "targets": [
                    { "name": "sample", "kind": ["lib"], "src_path": "/work/src/lib.rs" },
                    { "name": "props", "kind": ["test"], "src_path": "/work/tests/props.rs" }
                ]
            }]
        }"#;
        let metadata: Metadata = serde_json::from_str(raw).expect("the fixture JSON parses");
        Workspace {
            packages: metadata.packages,
        }
    }

    #[test]
    fn finds_the_lib_target_for_a_lib_suite() {
        let workspace = workspace();
        let root = workspace
            .root_file("sample", "path+file:///work#sample@0.1.0", "lib", "sample")
            .expect("the lib target exists");
        assert_eq!(root, Path::new("/work/src/lib.rs"));
    }

    #[test]
    fn finds_an_integration_test_target() {
        let workspace = workspace();
        let root = workspace
            .root_file(
                "sample::props",
                "path+file:///work#sample@0.1.0",
                "test",
                "props",
            )
            .expect("the test target exists");
        assert_eq!(root, Path::new("/work/tests/props.rs"));
    }

    #[test]
    fn reports_a_suite_without_a_target() {
        let error = workspace()
            .root_file(
                "sample::gone",
                "path+file:///work#sample@0.1.0",
                "test",
                "gone",
            )
            .expect_err("no such target");
        assert!(error.to_string().contains("sample::gone"));
    }

    #[test]
    fn unifies_dashes_and_underscores_in_target_names() {
        assert!(names_match("my-target", "my_target"));
        assert!(names_match("my_target", "my-target"));
        assert!(!names_match("my-target", "my-other"));
    }
}
