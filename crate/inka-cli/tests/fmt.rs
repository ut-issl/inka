//! Subprocess tests for `inka fmt`: what it writes, what `--check` reports,
//! and the exit status of each, run against scratch projects so the
//! checked-in files are never touched.

use std::fs;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

mod common;
use common::{local_fixture, workspace_fixture};

fn inka() -> Command {
    Command::cargo_bin("inka").expect("the inka binary should build")
}

/// A scratch project: the `mixed` fixture's manifest, listing `values_files`
/// as its values files, with the given argument files.
fn project_with_values(values_files: &[&str], files: &[(&str, &str)]) -> TempDir {
    let dir = TempDir::new().expect("create a temp dir");
    let manifest =
        fs::read_to_string(local_fixture("mixed").join("inka.toml")).expect("read the manifest");
    let listed: Vec<String> = values_files
        .iter()
        .map(|file| format!("{file:?}"))
        .collect();
    let manifest = manifest.replace("files = []", &format!("files = [{}]", listed.join(", ")));
    fs::write(dir.path().join("inka.toml"), manifest).expect("write the manifest");
    let argument = dir.path().join("argument");
    fs::create_dir(&argument).expect("create argument/");
    for (name, text) in files {
        fs::write(argument.join(name), text).expect("write an argument file");
    }
    dir
}

fn project(files: &[(&str, &str)]) -> TempDir {
    project_with_values(&[], files)
}

fn read(dir: &TempDir, name: &str) -> String {
    fs::read_to_string(dir.path().join("argument").join(name)).expect("read back")
}

const UNFORMATTED: &str = "fiat leaf {\n  ```\n  taken as given\n  ```\n}\n";
const FORMATTED: &str = "fiat leaf {\n    ```\n    taken as given\n    ```\n}\n";

#[test]
fn the_shared_fixtures_are_formatted() {
    for name in ["aocs", "dogfood"] {
        inka()
            .args(["fmt", "--check"])
            .arg(workspace_fixture(name))
            .assert()
            .success()
            .stdout("")
            .stderr("");
    }
}

#[test]
fn check_lists_an_unformatted_file_fails_and_writes_nothing() {
    // Arrange
    let dir = project(&[("leaf.inka", UNFORMATTED), ("done.inka", FORMATTED)]);

    // Act & Assert
    inka()
        .current_dir(dir.path())
        .args(["fmt", "--check"])
        .assert()
        .failure()
        .stdout("./argument/leaf.inka\n")
        .stderr("");
    assert_eq!(read(&dir, "leaf.inka"), UNFORMATTED);
}

#[test]
fn fmt_rewrites_unformatted_files_and_then_check_passes() {
    // Arrange
    let dir = project(&[("leaf.inka", UNFORMATTED)]);

    // Act
    inka()
        .arg("fmt")
        .arg(dir.path())
        .assert()
        .success()
        .stdout("")
        .stderr("");

    // Assert
    assert_eq!(read(&dir, "leaf.inka"), FORMATTED);
    inka()
        .args(["fmt", "--check"])
        .arg(dir.path())
        .assert()
        .success();
}

#[cfg(unix)]
#[test]
fn a_rewritten_file_keeps_its_permissions_and_leaves_no_temporary_file() {
    use std::os::unix::fs::PermissionsExt;

    // Arrange
    let dir = project(&[("leaf.inka", UNFORMATTED)]);
    let path = dir.path().join("argument/leaf.inka");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).expect("set permissions");

    // Act
    inka().arg("fmt").arg(dir.path()).assert().success();

    // Assert
    assert_eq!(read(&dir, "leaf.inka"), FORMATTED);
    let mode = fs::metadata(&path).expect("stat").permissions().mode();
    assert_eq!(mode & 0o777, 0o640);
    let names: Vec<_> = fs::read_dir(dir.path().join("argument"))
        .expect("list argument/")
        .map(|entry| entry.expect("entry").file_name())
        .collect();
    assert_eq!(names, ["leaf.inka"]);
}

#[test]
fn a_file_that_does_not_parse_is_reported_untouched_while_the_others_are_formatted() {
    // Arrange
    let broken = "fiat broken {\n  ```\n  x\n  ```\n  reason: gone\n}\n";
    let dir = project(&[("broken.inka", broken), ("leaf.inka", UNFORMATTED)]);

    // Act & Assert
    inka()
        .current_dir(dir.path())
        .args(["--color", "never", "fmt"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown property `reason`"));
    assert_eq!(read(&dir, "broken.inka"), broken);
    assert_eq!(read(&dir, "leaf.inka"), FORMATTED);
}

#[test]
fn check_reports_a_file_that_does_not_parse() {
    // Arrange
    let dir = project(&[("broken.inka", "fiat {\n")]);

    // Act & Assert
    inka()
        .current_dir(dir.path())
        .args(["--color", "never", "fmt", "--check"])
        .assert()
        .failure()
        .stdout("")
        .stderr(predicate::str::contains("expected a block identifier"));
}

#[test]
fn a_project_whose_values_cannot_load_is_not_formatted() {
    // Arrange
    let dir = project_with_values(&["missing.toml"], &[("leaf.inka", UNFORMATTED)]);

    // Act & Assert
    inka()
        .arg("fmt")
        .arg(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("missing.toml"));
    assert_eq!(read(&dir, "leaf.inka"), UNFORMATTED);
}

#[test]
fn a_fence_is_reindented_only_when_its_contract_survives() {
    // Arrange: `list` spans lines, so moving a fence that interpolates it
    // would shift the rendered text's indent and change its view hash.
    let source = concat!(
        "fiat listed {\n",
        "```\n",
        "{$list}\n",
        "```\n",
        "}\n",
        "\n",
        "fiat limited {\n",
        "```\n",
        "at most {$limit}\n",
        "```\n",
        "}\n",
    );
    let dir = project_with_values(&["values.toml"], &[("main.inka", source)]);
    fs::write(
        dir.path().join("values.toml"),
        "list = \"- a\\n- b\"\nlimit = \"5 m\"\n",
    )
    .expect("write the values");

    // Act
    inka().arg("fmt").arg(dir.path()).assert().success();

    // Assert
    assert_eq!(
        read(&dir, "main.inka"),
        concat!(
            "fiat listed {\n",
            "    ```\n",
            "{$list}\n",
            "    ```\n",
            "}\n",
            "\n",
            "fiat limited {\n",
            "    ```\n",
            "    at most {$limit}\n",
            "    ```\n",
            "}\n",
        )
    );
}
