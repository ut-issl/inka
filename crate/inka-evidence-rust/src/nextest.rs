//! Driving cargo-nextest: planning the run and executing it.
//!
//! The required execution set is declared from `cargo nextest list`, never
//! inferred from results: a run that aborts leaves declared tests with a
//! missing outcome. Scope is determined by listing twice — once as the run
//! will happen and once with `--run-ignored all` — because a plain listing
//! reports an `#[ignore]`d test as `mismatch (ignored)` whether or not the
//! user's filters select it. A test selected only by the wider listing is
//! in scope but skipped, and must itself be marked ignored: any other
//! difference between the listings is an explicit error.

use std::collections::{BTreeMap, BTreeSet};
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
    /// Whether the run reports a result for this test.
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
    // When the user already chose how ignored tests are treated, the first
    // listing is the whole truth; otherwise widen it to find the tests that
    // are in scope but skipped as ignored.
    let scoped = if user_set_run_ignored(cli) {
        narrow.rust_suites.clone()
    } else {
        list(cli, true)?.rust_suites
    };
    let tests = derive_tests(&narrow, scoped)?;
    Ok(Plan {
        target_directory: narrow.rust_build_meta.target_directory,
        tests,
    })
}

/// Merges the two listings into the planned tests.
fn derive_tests(
    narrow: &ListOutput,
    scoped: BTreeMap<String, Suite>,
) -> Result<Vec<PlannedTest>, Error> {
    let runs: BTreeSet<(&str, &str)> = narrow
        .rust_suites
        .iter()
        .flat_map(|(binary_id, suite)| {
            suite
                .testcases
                .iter()
                .filter(|(_, case)| matches!(case.filter_match, FilterMatch::Matches))
                .map(|(name, _)| (binary_id.as_str(), name.as_str()))
        })
        .collect();

    let mut tests = Vec::new();
    for (binary_id, suite) in scoped {
        for (name, case) in suite.testcases {
            if !matches!(case.filter_match, FilterMatch::Matches) {
                continue;
            }
            let scope = if runs.contains(&(binary_id.as_str(), name.as_str())) {
                Scope::Runs
            } else if case.ignored {
                Scope::Ignored
            } else {
                // The widened listing selected something `#[ignore]` cannot
                // explain; classifying it as skipped would be a guess.
                return Err(Error::ScopeMismatch { binary_id, name });
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
    Ok(tests)
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
    // The adapter's own standard output may carry the evidence (`-`), so
    // nothing nextest prints is allowed to land there.
    command.stdout(stderr_stdio());
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

/// A `Stdio` that forwards to this process's standard error, so a child's
/// stdout stays visible without touching the evidence stream. Falls back
/// to discarding the output if the handle cannot be duplicated.
fn stderr_stdio() -> Stdio {
    #[cfg(unix)]
    {
        use std::os::fd::AsFd;
        if let Ok(fd) = std::io::stderr().as_fd().try_clone_to_owned() {
            return Stdio::from(fd);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsHandle;
        if let Ok(handle) = std::io::stderr().as_handle().try_clone_to_owned() {
            return Stdio::from(handle);
        }
    }
    Stdio::null()
}

/// The forwarded arguments that are nextest's own: everything up to the
/// first `--`, after which arguments belong to the test binaries.
fn nextest_own_args(cli: &Cli) -> impl Iterator<Item = &String> {
    cli.nextest_args.iter().take_while(|arg| *arg != "--")
}

/// The nextest profile the run will use: the last `--profile`/`-P` among
/// nextest's own forwarded arguments, else `NEXTEST_PROFILE`, else
/// `default`. The `JUnit` report lands in the profile's store directory,
/// so the adapter must mirror nextest's own resolution.
fn profile_name(cli: &Cli) -> String {
    let mut profile = None;
    let mut args = nextest_own_args(cli);
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
    nextest_own_args(cli).any(|arg| arg == "--run-ignored" || arg.starts_with("--run-ignored="))
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
    ignored: bool,
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

    fn listing(raw: &str) -> ListOutput {
        serde_json::from_str(raw).expect("the fixture JSON parses")
    }

    const NARROW: &str = r#"{
        "rust-build-meta": { "target-directory": "/work/target" },
        "rust-suites": {
            "sample": {
                "package-name": "sample",
                "binary-name": "sample",
                "package-id": "path+file:///work#0.1.0",
                "kind": "lib",
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

    const WIDE: &str = r#"{
        "rust-build-meta": { "target-directory": "/work/target" },
        "rust-suites": {
            "sample": {
                "package-name": "sample",
                "binary-name": "sample",
                "package-id": "path+file:///work#0.1.0",
                "kind": "lib",
                "testcases": {
                    "tests::passes": {
                        "ignored": false,
                        "filter-match": { "status": "matches" }
                    },
                    "tests::ignored": {
                        "ignored": true,
                        "filter-match": { "status": "matches" }
                    }
                }
            }
        }
    }"#;

    #[test]
    fn parses_the_list_shape_nextest_emits() {
        let parsed = listing(NARROW);

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
        assert!(suite.testcases["tests::ignored"].ignored);
        assert!(matches!(
            &suite.testcases["tests::ignored"].filter_match,
            FilterMatch::Mismatch { reason } if reason == "ignored"
        ));
    }

    #[test]
    fn a_wide_only_ignored_test_is_declared_as_skipped() {
        let narrow = listing(NARROW);
        let scoped = listing(WIDE).rust_suites;

        let tests = derive_tests(&narrow, scoped).expect("the listings are consistent");

        let classified: Vec<(&str, Scope)> = tests
            .iter()
            .map(|test| (test.name.as_str(), test.scope))
            .collect();
        assert_eq!(
            classified,
            [
                ("tests::ignored", Scope::Ignored),
                ("tests::passes", Scope::Runs)
            ]
        );
    }

    #[test]
    fn a_wide_only_unignored_test_is_an_explicit_error() {
        let narrow = listing(NARROW);
        let inconsistent = WIDE.replace(
            r#""tests::ignored": {
                        "ignored": true,"#,
            r#""tests::ignored": {
                        "ignored": false,"#,
        );
        let scoped = listing(&inconsistent).rust_suites;

        let error = derive_tests(&narrow, scoped).expect_err("the listings disagree");

        assert!(error.to_string().contains("tests::ignored"));
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
    fn arguments_after_a_double_dash_belong_to_the_test_binaries() {
        let cli = cli_with_args(&["--", "--profile", "boom", "--run-ignored=all"]);
        assert_eq!(profile_name(&cli), "default");
        assert!(!user_set_run_ignored(&cli));
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
