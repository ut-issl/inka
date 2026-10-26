//! End-to-end tests driving the adapter binary against fixture crates.
//!
//! These run real `cargo nextest` builds, so cargo-nextest must be
//! installed; a missing installation fails loudly instead of skipping
//! silently, because a skipped suite would hide real breakage.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use inka_evidence::{Outcome, Test};

/// Serializes the tests that share the `sample` fixture's target directory
/// and therefore its single `JUnit` report path.
static SAMPLE: Mutex<()> = Mutex::new(());

fn lock_sample() -> MutexGuard<'static, ()> {
    SAMPLE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn fixture_manifest(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
        .join("Cargo.toml")
}

/// The adapter under test, with the capsula and nextest environment
/// scrubbed so each test states its own inputs.
fn adapter() -> assert_cmd::Command {
    let mut command =
        assert_cmd::Command::cargo_bin("inka-evidence-rust").expect("the adapter binary builds");
    command.env_remove("CAPSULA_RUN_ID");
    command.env_remove("NEXTEST_PROFILE");
    command
}

fn require_nextest() {
    let status = std::process::Command::new("cargo")
        .args(["nextest", "--version"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();
    assert!(
        status.is_ok_and(|status| status.success()),
        "these tests need cargo-nextest (https://nexte.st); install it and rerun"
    );
}

#[test]
fn emits_schema_valid_evidence_for_a_real_run() {
    require_nextest();
    let _guard = lock_sample();
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("sample-evidence.json");

    // The fixture contains a failing test on purpose: failures are
    // evidence, so the adapter still exits successfully.
    adapter()
        .arg("--manifest-path")
        .arg(fixture_manifest("sample"))
        .arg("--output")
        .arg(&out)
        .arg("--case")
        .arg("linux-stable")
        .arg("--provenance")
        .arg("source_revision=fixture-rev")
        .assert()
        .success();

    let text = std::fs::read_to_string(&out).expect("the evidence file was written");
    let tests = inka_evidence::parse(&text).expect("the adapter emits consumer-valid evidence");
    let by_id: BTreeMap<&str, &Test> = tests.iter().map(|test| (test.id.as_str(), test)).collect();

    let outcomes: BTreeMap<&str, Option<Outcome>> = by_id
        .iter()
        .map(|(id, test)| (*id, test.executions[0].outcome))
        .collect();
    let expected: BTreeMap<&str, Option<Outcome>> = [
        ("sample::tests::passes", Some(Outcome::Pass)),
        ("sample::tests::fails", Some(Outcome::Fail)),
        ("sample::tests::ignored", Some(Outcome::Skip)),
        ("sample::tests::nested::deep_passes", Some(Outcome::Pass)),
        ("sample::util::tests::doubles", Some(Outcome::Pass)),
        ("sample::props::add_commutes", Some(Outcome::Pass)),
        ("sample::props::plain_integration", Some(Outcome::Pass)),
        ("sample::aliased::sub::tests::via_path", Some(Outcome::Pass)),
        (
            "sample::aliased::sub::extra::tests::via_sibling_path",
            Some(Outcome::Pass),
        ),
    ]
    .into_iter()
    .collect();
    assert_eq!(outcomes, expected);

    // The `#[path]` fixtures carry decoy files at the wrong resolutions;
    // real bodies prove the resolver followed rustc.
    assert!(
        !by_id["sample::aliased::sub::tests::via_path"]
            .body
            .contains("DECOY")
    );
    assert!(
        !by_id["sample::aliased::sub::extra::tests::via_sibling_path"]
            .body
            .contains("DECOY")
    );

    for test in &tests {
        assert_eq!(test.executions.len(), 1, "one case per run: {}", test.id);
        assert_eq!(test.executions[0].case, "linux-stable");
    }

    assert_eq!(
        by_id["sample::tests::passes"].body,
        "#[test]\n    fn passes() {\n        assert_eq!(add(1, 2), 3);\n    }"
    );
    assert_eq!(
        by_id["sample::props::add_commutes"].body,
        "#[test]\n    fn add_commutes(a in 0u64..1000, b in 0u64..1000) {\n        \
         prop_assert_eq!(sample::add(a, b), sample::add(b, a));\n    }"
    );

    let raw: serde_json::Value = serde_json::from_str(&text).expect("the evidence is JSON");
    assert_eq!(raw["v"], 1);
    assert_eq!(raw["provenance"]["source_revision"], "fixture-rev");
    assert!(
        raw["provenance"].get("capsula_run_id").is_none(),
        "no capsula run id was in the environment"
    );
}

#[test]
fn respects_filters_and_the_capsula_environment() {
    require_nextest();
    let _guard = lock_sample();

    let assert = adapter()
        .arg("--manifest-path")
        .arg(fixture_manifest("sample"))
        .env("CAPSULA_RUN_ID", "run-123")
        .arg("--")
        .arg("passes")
        .assert()
        .success();

    let text = String::from_utf8(assert.get_output().stdout.clone())
        .expect("the evidence on standard output is UTF-8");
    let tests = inka_evidence::parse(&text).expect("filtered evidence is still valid");
    let mut ids: Vec<&str> = tests.iter().map(|test| test.id.as_str()).collect();
    ids.sort_unstable();
    // The string filter also drops the ignored test from scope: it is not
    // declared, rather than declared-and-skipped.
    assert_eq!(
        ids,
        [
            "sample::tests::nested::deep_passes",
            "sample::tests::passes"
        ]
    );
    for test in &tests {
        assert_eq!(test.executions[0].case, "default");
        assert_eq!(test.executions[0].outcome, Some(Outcome::Pass));
    }

    let raw: serde_json::Value = serde_json::from_str(&text).expect("the evidence is JSON");
    assert_eq!(raw["provenance"]["capsula_run_id"], "run-123");
}

#[test]
fn emits_skip_evidence_when_only_ignored_tests_match() {
    require_nextest();
    let _guard = lock_sample();

    // The filter selects only the `#[ignore]`d test: there is nothing to
    // run, yet the declared-and-skipped evidence must still be emitted.
    let assert = adapter()
        .arg("--manifest-path")
        .arg(fixture_manifest("sample"))
        .arg("--")
        .arg("ignored")
        .assert()
        .success();

    let text = String::from_utf8(assert.get_output().stdout.clone())
        .expect("the evidence on standard output is UTF-8");
    let tests = inka_evidence::parse(&text).expect("skip-only evidence is still valid");
    assert_eq!(tests.len(), 1);
    assert_eq!(tests[0].id, "sample::tests::ignored");
    assert_eq!(tests[0].executions[0].outcome, Some(Outcome::Skip));
}

#[test]
fn a_user_run_ignored_policy_really_runs_the_ignored_test() {
    require_nextest();
    let _guard = lock_sample();

    // With --run-ignored the single listing is the whole truth and the
    // ignored test executes; its unreachable!() body fails, which is
    // evidence, not an adapter error.
    let assert = adapter()
        .arg("--manifest-path")
        .arg(fixture_manifest("sample"))
        .arg("--")
        .arg("--run-ignored")
        .arg("all")
        .arg("ignored")
        .assert()
        .success();

    let text = String::from_utf8(assert.get_output().stdout.clone())
        .expect("the evidence on standard output is UTF-8");
    let tests = inka_evidence::parse(&text).expect("the evidence is valid");
    assert_eq!(tests.len(), 1);
    assert_eq!(tests[0].id, "sample::tests::ignored");
    assert_eq!(tests[0].executions[0].outcome, Some(Outcome::Fail));
}

#[test]
fn rejects_colliding_test_ids() {
    require_nextest();

    let assert = adapter()
        .arg("--manifest-path")
        .arg(fixture_manifest("clash"))
        .assert()
        .failure();

    let stderr =
        String::from_utf8(assert.get_output().stderr.clone()).expect("the diagnostics are UTF-8");
    assert!(
        stderr.contains("clash::props::x") && stderr.contains("appears more than once"),
        "the collision is explicit: {stderr}"
    );
}

#[test]
fn fails_explicitly_on_a_macro_generated_test() {
    require_nextest();
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("opaque-evidence.json");
    let _ = std::fs::remove_file(&out);

    let assert = adapter()
        .arg("--manifest-path")
        .arg(fixture_manifest("opaque"))
        .arg("--output")
        .arg(&out)
        .assert()
        .failure();

    let stderr =
        String::from_utf8(assert.get_output().stderr.clone()).expect("the diagnostics are UTF-8");
    assert!(
        stderr.contains("opaque::tests::generated"),
        "the failing test is named: {stderr}"
    );
    assert!(
        stderr.contains("no `fn generated`"),
        "the unsupported shape is explained: {stderr}"
    );
    assert!(!out.exists(), "no evidence may be written on failure");
}

#[test]
fn rejects_a_provenance_entry_without_equals() {
    adapter()
        .arg("--provenance")
        .arg("nokey")
        .assert()
        .failure()
        .stderr(predicates::str::contains("key=value"));
}
