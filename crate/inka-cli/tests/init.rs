//! Subprocess tests for `inka init`: what it creates, that the result passes
//! the other commands as it stands, and that it never overwrites anything.

use std::fs;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn inka() -> Command {
    Command::cargo_bin("inka").expect("the inka binary should build")
}

#[test]
fn init_creates_a_project_that_checks_and_is_formatted() {
    // Arrange
    let dir = TempDir::new().expect("create a temp dir");

    // Act & Assert: every created path is listed, directories first.
    inka()
        .current_dir(dir.path())
        .args(["init", "demo"])
        .assert()
        .success()
        .stdout(
            "demo\ndemo/argument\ndemo/inka.toml\ndemo/values.toml\ndemo/argument/objective.inka\n",
        )
        .stderr("");

    let project = dir.path().join("demo");
    inka()
        .arg("check")
        .arg(&project)
        .assert()
        .success()
        .stdout("")
        .stderr("");
    inka()
        .args(["fmt", "--check"])
        .arg(&project)
        .assert()
        .success()
        .stdout("")
        .stderr("");
}

#[test]
fn the_name_defaults_to_the_directory_and_phases_are_taken_in_order() {
    // Arrange
    let dir = TempDir::new().expect("create a temp dir");
    let project = dir.path().join("aocs-demo");
    fs::create_dir(&project).expect("create the project dir");

    // Act
    inka()
        .current_dir(&project)
        .args(["init", "--phases", "BBM,EM,FM"])
        .assert()
        .success()
        .stdout("./argument\n./inka.toml\n./values.toml\n./argument/objective.inka\n");

    // Assert
    let manifest = fs::read_to_string(project.join("inka.toml")).expect("read the manifest");
    assert!(manifest.contains("name = \"aocs-demo\"\n"), "{manifest}");
    assert!(
        manifest.contains("order = [\"BBM\", \"EM\", \"FM\"]\n"),
        "{manifest}"
    );
    assert!(manifest.contains("current = \"BBM\"\n"), "{manifest}");
}

#[test]
fn an_explicit_name_is_written_as_given() {
    // Arrange
    let dir = TempDir::new().expect("create a temp dir");

    // Act
    inka()
        .current_dir(dir.path())
        .args(["init", "--name", "My \"Project\""])
        .assert()
        .success();

    // Assert
    let manifest = fs::read_to_string(dir.path().join("inka.toml")).expect("read the manifest");
    assert!(manifest.contains("name = 'My \"Project\"'\n"), "{manifest}");
    inka().arg("check").arg(dir.path()).assert().success();
}

#[test]
fn an_existing_file_is_never_overwritten() {
    // Arrange: a project with only its starter argument already in place.
    let dir = TempDir::new().expect("create a temp dir");
    let argument = dir.path().join("argument");
    fs::create_dir(&argument).expect("create argument/");
    fs::write(argument.join("objective.inka"), "mine").expect("write the argument");

    // Act & Assert
    inka()
        .current_dir(dir.path())
        .arg("init")
        .assert()
        .failure()
        .stdout("")
        .stderr(predicate::str::contains("objective.inka already exists"));
    assert_eq!(
        fs::read_to_string(argument.join("objective.inka")).expect("read back"),
        "mine"
    );
    assert!(!dir.path().join("inka.toml").exists());
    assert!(!dir.path().join("values.toml").exists());
}

#[test]
fn initialising_twice_fails_the_second_time() {
    // Arrange
    let dir = TempDir::new().expect("create a temp dir");
    inka()
        .current_dir(dir.path())
        .arg("init")
        .assert()
        .success();

    // Act & Assert
    inka()
        .current_dir(dir.path())
        .arg("init")
        .assert()
        .failure()
        .stderr(predicate::str::contains("inka.toml already exists"));
}

#[test]
fn invalid_phases_are_rejected_before_anything_is_written() {
    for phases in ["M 0", "M0,M0", "0M"] {
        // Arrange
        let dir = TempDir::new().expect("create a temp dir");

        // Act & Assert
        inka()
            .current_dir(dir.path())
            .args(["init", "--phases", phases])
            .assert()
            .failure()
            .stdout("");
        assert_eq!(
            fs::read_dir(dir.path()).expect("list the dir").count(),
            0,
            "--phases {phases} wrote something"
        );
    }
}
