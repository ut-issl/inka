//! Subprocess tests for `inka check`: exit status and rendered output against
//! the checked-in fixtures.
//!
//! Broken fixtures are run with the fixture directory as the working directory
//! and `.` as the path argument, so the paths ariadne prints are relative and
//! stable across machines — a precondition for the snapshot below.

use assert_cmd::Command;
use predicates::prelude::*;

mod common;
use common::{local_fixture, workspace_fixture};

/// A fresh `inka` command bound to the crate's built binary.
fn inka() -> Command {
    Command::cargo_bin("inka").expect("the inka binary should build")
}

#[test]
fn dogfood_project_checks_clean_with_no_output() {
    // Arrange & Act & Assert: the dogfood closing condition — a clean run with
    // exit 0 and nothing written to either stream.
    inka()
        .arg("check")
        .arg(workspace_fixture("dogfood"))
        .assert()
        .success()
        .stdout("")
        .stderr("");
}

#[test]
fn aocs_project_checks_clean() {
    inka()
        .arg("check")
        .arg(workspace_fixture("aocs"))
        .assert()
        .success()
        .stdout("")
        .stderr("");
}

#[test]
fn path_defaults_to_the_current_directory() {
    // Arrange & Act & Assert: `check` with no path argument checks the cwd.
    inka()
        .current_dir(workspace_fixture("dogfood"))
        .arg("check")
        .assert()
        .success()
        .stderr("");
}

#[test]
fn undefined_reference_fails_and_renders_a_snippet() {
    // Act
    let assert = inka()
        .current_dir(local_fixture("undefined-reference"))
        .args(["check", "."])
        .assert()
        .failure()
        .code(1);

    // Assert: freeze the rendered snippet so display drift is caught.
    let stderr = String::from_utf8(assert.get_output().stderr.clone())
        .expect("diagnostic output should be valid UTF-8");
    insta::assert_snapshot!(stderr);
}

#[test]
fn unreachable_node_is_a_warning_and_still_exits_zero() {
    // Assert: a warning-only project is well-formed, so the exit status is 0
    // even though a diagnostic is printed.
    inka()
        .current_dir(local_fixture("unreachable"))
        .args(["check", "."])
        .assert()
        .success()
        .stderr(predicate::str::contains("unreachable"));
}

#[test]
fn errors_and_warnings_interleave_in_source_order() {
    // Act: the fixture combines a build error with independently detectable
    // shape findings. The early unreachable warning must still render first
    // despite being the lesser finding — order follows the source, not lane or
    // validation stage.
    let assert = inka()
        .current_dir(local_fixture("mixed"))
        .args(["check", "."])
        .assert()
        .failure()
        .code(1);

    // Assert: freeze the interleaved rendering so ordering drift is caught.
    let stderr = String::from_utf8(assert.get_output().stderr.clone())
        .expect("diagnostic output should be valid UTF-8");
    insta::assert_snapshot!(stderr);
}

#[test]
fn missing_objective_reports_an_unanchored_error() {
    inka()
        .current_dir(local_fixture("no-objective"))
        .args(["check", "."])
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("no objective block found"));
}

#[test]
fn missing_manifest_reports_a_structural_error() {
    // Arrange: a directory that exists but holds no inka.toml.
    let without_manifest = local_fixture("no-objective").join("argument");

    // Act & Assert
    inka()
        .arg("check")
        .arg(without_manifest)
        .assert()
        .failure()
        .code(1)
        .stderr(predicate::str::contains("inka.toml"));
}

#[test]
fn no_subcommand_is_a_usage_error() {
    // clap rejects a missing subcommand with its own exit code 2.
    inka().assert().failure().code(2);
}

/// The ANSI Control Sequence Introducer that every colour escape begins with.
const ANSI_ESCAPE: &str = "\u{1b}[";

#[test]
fn color_always_emits_ansi_even_when_piped() {
    // A subprocess pipe is not a terminal, so only `--color always` forces the
    // coloured path here.
    inka()
        .current_dir(local_fixture("undefined-reference"))
        .args(["check", "--color", "always", "."])
        .assert()
        .failure()
        .stderr(predicate::str::contains(ANSI_ESCAPE));
}

#[test]
fn color_never_suppresses_ansi() {
    inka()
        .current_dir(local_fixture("undefined-reference"))
        .args(["check", "--color", "never", "."])
        .assert()
        .failure()
        .stderr(predicate::str::contains(ANSI_ESCAPE).not());
}

#[test]
fn explicit_color_overrides_no_color_env() {
    // An explicit `--color always` wins over a `NO_COLOR` request.
    inka()
        .current_dir(local_fixture("undefined-reference"))
        .env("NO_COLOR", "1")
        .args(["check", "--color", "always", "."])
        .assert()
        .failure()
        .stderr(predicate::str::contains(ANSI_ESCAPE));
}
