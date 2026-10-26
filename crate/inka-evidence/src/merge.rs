//! Merging several run configurations into one evidence file.
//!
//! Each input is one run's evidence; the merge unions the executions of
//! each test-id into one declared set, so a bind can require every
//! configuration to pass. Anything ambiguous is an explicit error: bodies
//! must match byte for byte across inputs (a mismatch means the source
//! changed between configuration runs), and no case may be declared twice.
//!
//! An input may carry a label, which prefixes every one of its case ids as
//! `label/case` — for adapters whose case axis is already taken (pytest's
//! parametrizations). Adapters with a free case axis (the Rust and Vitest
//! adapters' `--case`/`case` option) label their runs at emission time and
//! merge unlabeled.

use std::collections::HashMap;

use crate::builder::EvidenceBuilder;
use crate::schema::{self, Execution, InvalidEvidence, ParseError};

/// The provenance key the merge carries over when every input agrees.
const SOURCE_REVISION_KEY: &str = "source_revision";

/// One input to [`merge`]: an evidence file's text and an optional label
/// prefixed onto its case ids as `label/case`.
#[derive(Debug, Clone)]
pub struct MergeInput {
    pub label: Option<String>,
    pub text: String,
}

/// A reason several evidence files could not merge into one.
#[derive(Debug, thiserror::Error)]
pub enum MergeError {
    /// Merging nothing would declare nothing.
    #[error("no inputs to merge")]
    NoInputs,

    /// A label was given but blank; `input` is 1-based.
    #[error("input {input} has a blank label")]
    BlankLabel { input: usize },

    /// An input did not read as a v1 evidence file; `input` is 1-based.
    #[error("input {input}: {source}")]
    Parse {
        input: usize,
        #[source]
        source: ParseError,
    },

    /// One test-id carries different bodies across inputs — the source
    /// changed between configuration runs.
    #[error("test `{test_id}` has different bodies across inputs")]
    BodyMismatch { test_id: String },

    /// One (test-id, case) pair is declared by more than one input.
    #[error("test `{test_id}` declares case `{case}` in more than one input")]
    DuplicateCase { test_id: String, case: String },

    /// Inputs disagree on `source_revision`: they come from different
    /// source states and must not merge.
    #[error("inputs disagree on source_revision: `{left}` and `{right}`")]
    SourceRevisionMismatch { left: String, right: String },

    /// The merged file would violate the wire contract (for example a
    /// blank provenance entry).
    #[error(transparent)]
    Invalid(#[from] InvalidEvidence),
}

/// Merges the inputs into one evidence file's text.
///
/// The executions of one test-id union across inputs in input order; a
/// test present in only some inputs keeps exactly its declared cases. The
/// merged provenance is `provenance` plus `capsula_run_id` filled from
/// `capsula_run_id` (unless an explicit entry claimed it), with
/// `source_revision` carried over automatically when every input states
/// the same value; an explicit `provenance` entry overrides the carry.
///
/// # Errors
///
/// Returns a [`MergeError`] when there are no inputs, a label is blank, an
/// input fails to parse, a test-id's bodies differ, a (test-id, case) pair
/// repeats, the inputs disagree on `source_revision`, or the merged file
/// would violate the wire contract.
pub fn merge(
    inputs: &[MergeInput],
    provenance: &[(String, String)],
    capsula_run_id: Option<String>,
) -> Result<String, MergeError> {
    if inputs.is_empty() {
        return Err(MergeError::NoInputs);
    }

    let mut order: Vec<String> = Vec::new();
    let mut tests: HashMap<String, (String, Vec<Execution>)> = HashMap::new();
    let mut revisions: Vec<Option<String>> = Vec::new();

    for (index, input) in inputs.iter().enumerate() {
        let input_number = index + 1;
        if input
            .label
            .as_ref()
            .is_some_and(|label| label.trim().is_empty())
        {
            return Err(MergeError::BlankLabel {
                input: input_number,
            });
        }
        let (file_provenance, file_tests) =
            schema::parse_with_provenance(&input.text).map_err(|source| MergeError::Parse {
                input: input_number,
                source,
            })?;
        revisions.push(
            file_provenance
                .get(SOURCE_REVISION_KEY)
                .and_then(|value| value.as_str())
                .map(str::to_owned),
        );

        for test in file_tests {
            let executions: Vec<Execution> = test
                .executions
                .into_iter()
                .map(|execution| {
                    let case = match &input.label {
                        Some(label) => format!("{label}/{}", execution.case),
                        None => execution.case,
                    };
                    Execution::new(case, execution.outcome)
                })
                .collect();
            match tests.get_mut(&test.id) {
                None => {
                    order.push(test.id.clone());
                    tests.insert(test.id, (test.body, executions));
                }
                Some((body, merged)) => {
                    if *body != test.body {
                        return Err(MergeError::BodyMismatch { test_id: test.id });
                    }
                    for execution in executions {
                        if merged.iter().any(|seen| seen.case == execution.case) {
                            return Err(MergeError::DuplicateCase {
                                test_id: test.id,
                                case: execution.case,
                            });
                        }
                        merged.push(execution);
                    }
                }
            }
        }
    }

    let mut builder = EvidenceBuilder::new();
    if let Some(revision) = shared_revision(&revisions)? {
        builder.provenance(SOURCE_REVISION_KEY, revision)?;
    }
    for (key, value) in provenance {
        builder.provenance(key, value)?;
    }
    builder.capsula_run_id(capsula_run_id);
    for id in order {
        let (body, executions) = tests
            .remove(&id)
            .unwrap_or_else(|| unreachable!("every ordered id was inserted"));
        builder.add_test(id, body, executions)?;
    }
    Ok(builder.finish())
}

/// The `source_revision` every input states, if they all state one and
/// agree; disagreement is an error, partial presence carries nothing.
fn shared_revision(revisions: &[Option<String>]) -> Result<Option<&String>, MergeError> {
    let mut stated = revisions.iter().flatten();
    let Some(first) = stated.next() else {
        return Ok(None);
    };
    if let Some(other) = stated.find(|revision| *revision != first) {
        return Err(MergeError::SourceRevisionMismatch {
            left: first.clone(),
            right: other.clone(),
        });
    }
    Ok(revisions.iter().all(Option::is_some).then_some(first))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::schema::{Outcome, parse};

    /// One fixture test: id, body, and its (case, outcome) executions.
    type TestSpec<'a> = (&'a str, &'a str, &'a [(&'a str, Option<Outcome>)]);

    fn evidence(revision: Option<&str>, tests: &[TestSpec<'_>]) -> String {
        let mut builder = EvidenceBuilder::new();
        if let Some(revision) = revision {
            builder
                .provenance(SOURCE_REVISION_KEY, revision)
                .expect("nonblank");
        }
        for (id, body, executions) in tests {
            let executions = executions
                .iter()
                .map(|(case, outcome)| Execution::new(*case, *outcome))
                .collect();
            builder.add_test(*id, *body, executions).expect("valid");
        }
        builder.finish()
    }

    fn input(label: Option<&str>, text: String) -> MergeInput {
        MergeInput {
            label: label.map(str::to_owned),
            text,
        }
    }

    #[test]
    fn unions_executions_of_one_test_across_inputs() {
        let linux = evidence(None, &[("t", "body", &[("linux", Some(Outcome::Pass))])]);
        let macos = evidence(None, &[("t", "body", &[("macos", Some(Outcome::Fail))])]);

        let merged = merge(&[input(None, linux), input(None, macos)], &[], None)
            .expect("disjoint cases merge");

        let tests = parse(&merged).expect("the merge emits consumer-valid evidence");
        assert_eq!(tests.len(), 1);
        assert_eq!(
            tests[0].executions,
            vec![
                Execution::new("linux", Some(Outcome::Pass)),
                Execution::new("macos", Some(Outcome::Fail)),
            ]
        );
    }

    #[test]
    fn labels_prefix_every_case_of_their_input() {
        let linux = evidence(None, &[("t", "body", &[("one", Some(Outcome::Pass))])]);
        let macos = evidence(None, &[("t", "body", &[("one", None)])]);

        let merged = merge(
            &[input(Some("linux"), linux), input(Some("macos"), macos)],
            &[],
            None,
        )
        .expect("labeled inputs merge");

        let tests = parse(&merged).expect("valid");
        assert_eq!(
            tests[0].executions,
            vec![
                Execution::new("linux/one", Some(Outcome::Pass)),
                Execution::new("macos/one", None),
            ]
        );
    }

    #[test]
    fn a_test_in_only_one_input_keeps_its_declared_cases() {
        let linux = evidence(
            None,
            &[
                ("shared", "body", &[("linux", Some(Outcome::Pass))]),
                ("linux_only", "body", &[("default", Some(Outcome::Pass))]),
            ],
        );
        let macos = evidence(
            None,
            &[("shared", "body", &[("macos", Some(Outcome::Pass))])],
        );

        let merged = merge(&[input(None, linux), input(None, macos)], &[], None).expect("merges");

        let tests = parse(&merged).expect("valid");
        assert_eq!(tests.len(), 2);
        assert_eq!(tests[1].id, "linux_only");
        assert_eq!(tests[1].executions.len(), 1);
    }

    #[test]
    fn different_bodies_for_one_test_are_an_error() {
        let one = evidence(None, &[("t", "body a", &[("linux", None)])]);
        let two = evidence(None, &[("t", "body b", &[("macos", None)])]);

        let error =
            merge(&[input(None, one), input(None, two)], &[], None).expect_err("bodies differ");

        assert!(matches!(error, MergeError::BodyMismatch { test_id } if test_id == "t"));
    }

    #[test]
    fn a_case_declared_twice_is_an_error() {
        let one = evidence(None, &[("t", "body", &[("default", Some(Outcome::Pass))])]);
        let two = evidence(None, &[("t", "body", &[("default", Some(Outcome::Pass))])]);

        let error =
            merge(&[input(None, one), input(None, two)], &[], None).expect_err("cases collide");

        assert!(
            matches!(error, MergeError::DuplicateCase { test_id, case } if test_id == "t" && case == "default")
        );
    }

    #[test]
    fn agreeing_source_revisions_carry_over() {
        let one = evidence(Some("abc"), &[("t", "body", &[("linux", None)])]);
        let two = evidence(Some("abc"), &[("t", "body", &[("macos", None)])]);

        let merged = merge(&[input(None, one), input(None, two)], &[], None).expect("merges");

        assert!(merged.contains("\"source_revision\": \"abc\""));
    }

    #[test]
    fn disagreeing_source_revisions_are_an_error() {
        let one = evidence(Some("abc"), &[("t", "body", &[("linux", None)])]);
        let two = evidence(Some("def"), &[("t", "body", &[("macos", None)])]);

        let error =
            merge(&[input(None, one), input(None, two)], &[], None).expect_err("revisions differ");

        assert!(matches!(error, MergeError::SourceRevisionMismatch { .. }));
    }

    #[test]
    fn a_partially_stated_source_revision_carries_nothing() {
        let one = evidence(Some("abc"), &[("t", "body", &[("linux", None)])]);
        let two = evidence(None, &[("t", "body", &[("macos", None)])]);

        let merged = merge(&[input(None, one), input(None, two)], &[], None).expect("merges");

        assert!(!merged.contains("source_revision"));
    }

    #[test]
    fn explicit_provenance_overrides_the_carried_revision() {
        let one = evidence(Some("abc"), &[("t", "body", &[("linux", None)])]);

        let merged = merge(
            &[input(None, one)],
            &[(String::from("source_revision"), String::from("xyz"))],
            Some(String::from("run-1")),
        )
        .expect("merges");

        assert!(merged.contains("\"source_revision\": \"xyz\""));
        assert!(merged.contains("\"capsula_run_id\": \"run-1\""));
    }

    #[test]
    fn input_provenance_other_than_the_revision_is_not_carried() {
        let mut builder = EvidenceBuilder::new();
        builder.provenance("runner", "ci-7").expect("nonblank");
        builder
            .add_test("t", "body", vec![Execution::new("linux", None)])
            .expect("valid");
        let one = builder.finish();

        let merged = merge(&[input(None, one)], &[], None).expect("merges");

        assert!(!merged.contains("runner"));
    }

    #[test]
    fn no_inputs_is_an_error() {
        assert!(matches!(merge(&[], &[], None), Err(MergeError::NoInputs)));
    }

    #[test]
    fn a_blank_label_is_an_error() {
        let one = evidence(None, &[("t", "body", &[("linux", None)])]);

        let error = merge(&[input(Some("  "), one)], &[], None).expect_err("blank label");

        assert!(matches!(error, MergeError::BlankLabel { input: 1 }));
    }

    #[test]
    fn an_unreadable_input_names_its_position() {
        let error =
            merge(&[input(None, String::from("not json"))], &[], None).expect_err("not evidence");

        assert!(error.to_string().starts_with("input 1:"));
    }
}
