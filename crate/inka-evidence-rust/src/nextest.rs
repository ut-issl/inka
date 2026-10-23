//! Driving cargo-nextest: planning the run and executing it.
//!
//! The required execution set is declared from `cargo nextest list`, never
//! inferred from results: a run that aborts leaves declared tests with a
//! missing outcome. Scope is determined by listing twice — once as the run
//! will happen and once with `--run-ignored all` — because a plain listing
//! reports an `#[ignore]`d test as `mismatch (ignored)` whether or not the
//! user's filters select it. A test selected only by the wider listing is
//! in scope but skipped.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Deserialize;

use crate::cli::Cli;
use crate::error::Error;
use crate::junit;

/// Whether a planned test runs or is skipped as ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Selected by the run's filters: the run reports its result.
    Runs,
    /// In scope but `#[ignore]`d under this run's flags: declared with a
    /// `skip` outcome.
    Ignored,
}

/// One test in scope, with everything needed to find its source and match
/// its result.
#[derive(Debug)]
pub struct PlannedTest {
    /// nextest's suite identifier, for example `sample::props`.
    pub binary_id: String,
    /// The owning package's `cargo metadata` id.
    pub package_id: String,
    /// nextest's suite kind: `lib`, `test`, `bin`, ...
    pub kind: String,
    /// The cargo target name the suite was built from.
    pub binary_name: String,
    /// The test's path inside the binary, for example `tests::passes`.
    pub name: String,
    pub scope: Scope,
}

/// The planned run: where cargo builds, and every test in scope.
#[derive(Debug)]
pub struct Plan {
    pub target_directory: PathBuf,
    pub tests: Vec<PlannedTest>,
}

/// Lists the run and derives each test's scope.
pub fn plan(cli: &Cli) -> Result<Plan, Error> {
    let narrow = list(cli, false)?;
    let runs: std::collections::BTreeSet<(String, String)> = narrow
        .rust_suites
        .iter()
        .flat_map(|(binary_id, suite)| {
            suite
                .testcases
                .iter()
                .filter(|(_, case)| matches!(case.filter_match, FilterMatch::Matches))
                .map(|(name, _)| (binary_id.clone(), name.clone()))
        })
        .collect();

    // When the user already chose how ignored tests are treated, the first
    // listing is the whole truth; otherwise widen it to find the tests that
    // are in scope but skipped as ignored.
    let scoped = if user_set_run_ignored(cli) {
        narrow.rust_suites.clone()
    } else {
        list(cli, true)?.rust_suites
    };

    let mut tests = Vec::new();
    for (binary_id, suite) in scoped {
        for (name, case) in suite.testcases {
            if !matches!(case.filter_match, FilterMatch::Matches) {
                continue;
            }
            let scope = if runs.contains(&(binary_id.clone(), name.clone())) {
                Scope::Runs
            } else {
                Scope::Ignored
            };
            tests.push(PlannedTest {
                binary_id: binary_id.clone(),
                package_id: suite.package_id.clone(),
                kind: suite.kind.clone(),
                binary_name: suite.binary_name.clone(),
                name,
                scope,
            });
        }
    }
    Ok(Plan {
        target_directory: narrow.rust_build_meta.target_directory,
        tests,
    })
}

/// nextest's exit code for a run that completed with test failures.
const TESTS_FAILED: i32 = 100;

/// Runs the tests with `JUnit` output enabled and returns the parsed report.
///
/// Exit code 100 (tests failed) is a successful run: the failures are the
/// evidence. Any other nonzero status is an adapter error.
pub fn run(cli: &Cli, plan: &Plan) -> Result<junit::Report, Error> {
    let junit_path = plan
        .target_directory
        .join("nextest")
        .join(profile_name(cli))
        .join("junit.xml");
    // A leftover report from an earlier run must never masquerade as this
    // run's results.
    if let Err(source) = std::fs::remove_file(&junit_path)
        && source.kind() != std::io::ErrorKind::NotFound
    {
        return Err(Error::Write {
            path: junit_path,
            source,
        });
    }

    let tool_config = write_tool_config(&plan.target_directory)?;
    let mut command = cargo_nextest(cli, "run");
    command.arg("--no-fail-fast");
    command
        .arg("--tool-config-file")
        .arg(format!("inka-evidence:{}", tool_config.display()));
    command.args(&cli.nextest_args);
    let status = command
        .status()
        .map_err(|source| spawn_error("cargo nextest run", source))?;
    if !matches!(status.code(), Some(0 | TESTS_FAILED)) {
        return Err(Error::CommandFailed {
            command: String::from("cargo nextest run"),
            status,
        });
    }
    junit::parse(&junit_path)
}

/// The nextest profile the run will use: the last `--profile`/`-P` in the
/// forwarded arguments, else `NEXTEST_PROFILE`, else `default`. The `JUnit`
/// report lands in the profile's store directory, so the adapter must
/// mirror nextest's own resolution.
fn profile_name(cli: &Cli) -> String {
    let mut profile = None;
    let mut args = cli.nextest_args.iter();
    while let Some(arg) = args.next() {
        if arg == "--profile" || arg == "-P" {
            profile = args.next().cloned();
        } else if let Some(value) = arg.strip_prefix("--profile=") {
            profile = Some(value.to_owned());
        } else if let Some(value) = arg.strip_prefix("-P").filter(|value| !value.is_empty()) {
            profile = Some(value.trim_start_matches('=').to_owned());
        }
    }
    profile
        .or_else(|| std::env::var("NEXTEST_PROFILE").ok())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| String::from("default"))
}

/// Whether the forwarded arguments already pick a `--run-ignored` policy.
fn user_set_run_ignored(cli: &Cli) -> bool {
    cli.nextest_args
        .iter()
        .any(|arg| arg == "--run-ignored" || arg.starts_with("--run-ignored="))
}

/// Writes the tool config that turns on `JUnit` output, into the target
/// directory so no extra temporary location is needed.
fn write_tool_config(target_directory: &Path) -> Result<PathBuf, Error> {
    let path = target_directory.join("inka-evidence-nextest.toml");
    let config = "[profile.default.junit]\npath = \"junit.xml\"\n";
    std::fs::write(&path, config).map_err(|source| Error::Write {
        path: path.clone(),
        source,
    })?;
    Ok(path)
}

/// Runs `cargo nextest list --message-format json` and parses it.
fn list(cli: &Cli, run_ignored: bool) -> Result<ListOutput, Error> {
    let mut command = cargo_nextest(cli, "list");
    command.args(["--message-format", "json"]);
    if run_ignored {
        command.args(["--run-ignored", "all"]);
    }
    command.args(&cli.nextest_args);
    // The listing compiles the tests, so cargo's own diagnostics stay
    // visible on standard error while the JSON is captured.
    command.stdout(Stdio::piped()).stderr(Stdio::inherit());
    let output = command
        .output()
        .map_err(|source| spawn_error("cargo nextest list", source))?;
    if !output.status.success() {
        return Err(Error::CommandFailed {
            command: String::from("cargo nextest list"),
            status: output.status,
        });
    }
    serde_json::from_slice(&output.stdout).map_err(|source| Error::Format {
        what: "cargo nextest list",
        source,
    })
}

/// A `cargo nextest <subcommand>` invocation with the shared arguments.
fn cargo_nextest(cli: &Cli, subcommand: &str) -> Command {
    let mut command = Command::new("cargo");
    command.arg("nextest").arg(subcommand);
    if let Some(path) = &cli.manifest_path {
        command.arg("--manifest-path").arg(path);
    }
    command
}

/// Maps a spawn failure, pointing at the usual cause.
fn spawn_error(command: &str, source: std::io::Error) -> Error {
    let command = if source.kind() == std::io::ErrorKind::NotFound {
        format!("{command} (is cargo-nextest installed? see https://nexte.st)")
    } else {
        command.to_owned()
    };
    Error::Spawn { command, source }
}

/// The subset of `cargo nextest list --message-format json` this adapter
/// reads; unknown fields are ignored for forward compatibility.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct ListOutput {
    rust_build_meta: BuildMeta,
    rust_suites: BTreeMap<String, Suite>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct BuildMeta {
    target_directory: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct Suite {
    package_id: String,
    kind: String,
    binary_name: String,
    testcases: BTreeMap<String, Case>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct Case {
    filter_match: FilterMatch,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "kebab-case", tag = "status")]
enum FilterMatch {
    Matches,
    Mismatch {
        #[allow(dead_code, reason = "carried for diagnostics and tests")]
        reason: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_list_shape_nextest_emits() {
        let raw = r#"{
            "rust-build-meta": { "target-directory": "/work/target" },
            "test-count": 2,
            "rust-suites": {
                "sample": {
                    "package-name": "sample",
                    "binary-id": "sample",
                    "binary-name": "sample",
                    "package-id": "path+file:///work#0.1.0",
                    "kind": "lib",
                    "binary-path": "/work/target/debug/deps/sample-0000",
                    "testcases": {
                        "tests::passes": {
                            "ignored": false,
                            "filter-match": { "status": "matches" }
                        },
                        "tests::ignored": {
                            "ignored": true,
                            "filter-match": { "status": "mismatch", "reason": "ignored" }
                        }
                    }
                }
            }
        }"#;
        let parsed: ListOutput = serde_json::from_str(raw).expect("the fixture JSON parses");

        assert_eq!(
            parsed.rust_build_meta.target_directory,
            Path::new("/work/target")
        );
        let suite = &parsed.rust_suites["sample"];
        assert_eq!(suite.kind, "lib");
        assert!(matches!(
            suite.testcases["tests::passes"].filter_match,
            FilterMatch::Matches
        ));
        assert!(matches!(
            &suite.testcases["tests::ignored"].filter_match,
            FilterMatch::Mismatch { reason } if reason == "ignored"
        ));
    }

    #[test]
    fn profile_resolution_prefers_the_last_argument() {
        let cli = cli_with_args(&["--profile", "ci", "-Pfinal"]);
        assert_eq!(profile_name(&cli), "final");
    }

    #[test]
    fn profile_resolution_accepts_the_equals_forms() {
        assert_eq!(profile_name(&cli_with_args(&["--profile=ci"])), "ci");
        assert_eq!(profile_name(&cli_with_args(&["-P=ci"])), "ci");
    }

    #[test]
    fn detects_a_user_supplied_run_ignored_policy() {
        assert!(user_set_run_ignored(&cli_with_args(&[
            "--run-ignored",
            "all"
        ])));
        assert!(user_set_run_ignored(&cli_with_args(&[
            "--run-ignored=only"
        ])));
        assert!(!user_set_run_ignored(&cli_with_args(&["--no-capture"])));
    }

    fn cli_with_args(args: &[&str]) -> Cli {
        use clap::Parser;
        let mut command_line = vec!["inka-evidence-rust", "--"];
        command_line.extend(args);
        Cli::parse_from(command_line)
    }
}
