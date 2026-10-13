//! The emission guarantee: whatever `EvidenceBuilder::finish` returns, the
//! shared `parse` accepts, so an adapter built on this layer cannot produce a
//! file the consumer rejects. Invalid inputs fail at `add_test` time.

use inka_evidence::{EvidenceBuilder, Execution, InvalidEvidence, Outcome, parse};
use proptest::prelude::*;

fn execution(case: &str, outcome: Option<Outcome>) -> Execution {
    Execution {
        case: case.to_owned(),
        outcome,
    }
}

#[test]
fn built_evidence_parses_back_into_the_same_tests() {
    // Arrange
    let mut builder = EvidenceBuilder::new();
    builder.provenance("source_revision", "4f2a1c9");
    builder
        .add_test(
            "tests/hash_props.rs::roundtrip",
            "#[test]\nfn roundtrip() {}\n",
            vec![execution("default", Some(Outcome::Pass))],
        )
        .expect("a valid test is accepted");
    builder
        .add_test(
            "tests/matrix.py::test_cases",
            "def test_cases(n): ...",
            vec![
                execution("n=1", Some(Outcome::Fail)),
                execution("n=2", Some(Outcome::Skip)),
                execution("n=3", Some(Outcome::Cancelled)),
                execution("n=4", None),
            ],
        )
        .expect("a valid test is accepted");

    // Act
    let emitted = builder.finish();
    let parsed = parse(&emitted).expect("emitted evidence must always parse");

    // Assert
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].id, "tests/hash_props.rs::roundtrip");
    assert_eq!(parsed[0].body, "#[test]\nfn roundtrip() {}\n");
    assert_eq!(
        parsed[0].executions,
        [execution("default", Some(Outcome::Pass))]
    );
    assert_eq!(parsed[1].executions.len(), 4);
    assert_eq!(parsed[1].executions[3].outcome, None);
}

#[test]
fn the_version_field_is_stamped_and_the_output_ends_with_a_newline() {
    // Arrange
    let mut builder = EvidenceBuilder::new();
    builder
        .add_test("t", "b", vec![execution("e", Some(Outcome::Pass))])
        .expect("a valid test is accepted");

    // Act
    let emitted = builder.finish();

    // Assert
    let value: serde_json::Value = serde_json::from_str(&emitted).expect("valid JSON");
    assert_eq!(value["v"], 1);
    assert!(
        emitted.ends_with('\n'),
        "a written file should end with a newline"
    );
}

#[test]
fn a_missing_result_is_an_omitted_key_not_a_null() {
    // Arrange: the schema spells a missing result as an absent `outcome`;
    // `"outcome": null` would be rejected by its enum.
    let mut builder = EvidenceBuilder::new();
    builder
        .add_test("t", "b", vec![execution("e", None)])
        .expect("a declared-but-missing result is valid input");

    // Act
    let emitted = builder.finish();

    // Assert
    assert!(!emitted.contains("outcome"), "got: {emitted}");
    assert!(!emitted.contains("null"), "got: {emitted}");
}

#[test]
fn empty_provenance_is_omitted_entirely() {
    let mut builder = EvidenceBuilder::new();
    builder
        .add_test("t", "b", vec![execution("e", Some(Outcome::Pass))])
        .expect("a valid test is accepted");

    let emitted = builder.finish();

    assert!(!emitted.contains("provenance"), "got: {emitted}");
}

#[test]
fn invalid_inputs_are_rejected_at_add_time() {
    let mut builder = EvidenceBuilder::new();
    let pass = || vec![execution("e", Some(Outcome::Pass))];

    assert_eq!(
        builder.add_test("  ", "b", pass()),
        Err(InvalidEvidence::BlankTestId)
    );
    assert_eq!(
        builder.add_test("t", "", pass()),
        Err(InvalidEvidence::EmptyBody {
            test_id: "t".to_owned()
        })
    );
    assert_eq!(
        builder.add_test("t", "b", Vec::new()),
        Err(InvalidEvidence::NoExecutions {
            test_id: "t".to_owned()
        })
    );
    assert_eq!(
        builder.add_test("t", "b", vec![execution(" ", None)]),
        Err(InvalidEvidence::BlankCase {
            test_id: "t".to_owned()
        })
    );
    assert_eq!(
        builder.add_test(
            "t",
            "b",
            vec![
                execution("e", Some(Outcome::Pass)),
                execution("e", Some(Outcome::Fail))
            ]
        ),
        Err(InvalidEvidence::DuplicateCase {
            test_id: "t".to_owned(),
            case: "e".to_owned()
        })
    );

    // Nothing invalid leaked into the builder: the file is still emittable
    // and empty.
    assert_eq!(parse(&builder.finish()).expect("an empty file parses"), []);
}

#[test]
fn a_duplicate_test_id_across_calls_is_rejected() {
    let mut builder = EvidenceBuilder::new();
    builder
        .add_test("t", "b", vec![execution("e", Some(Outcome::Pass))])
        .expect("the first test is accepted");

    let error = builder
        .add_test("t", "other", vec![execution("e", Some(Outcome::Pass))])
        .expect_err("the same id twice is a routing conflict");

    assert_eq!(
        error,
        InvalidEvidence::DuplicateTest {
            test_id: "t".to_owned()
        }
    );
}

#[test]
fn capsula_run_id_fills_only_when_absent() {
    // Arrange: an explicit provenance entry wins over the environment value.
    let mut preset = EvidenceBuilder::new();
    preset.provenance("capsula_run_id", "explicit");
    preset.capsula_run_id(Some("from-env".to_owned()));

    let mut filled = EvidenceBuilder::new();
    filled.capsula_run_id(Some("from-env".to_owned()));

    let mut blank = EvidenceBuilder::new();
    blank.capsula_run_id(Some("   ".to_owned()));
    blank.capsula_run_id(None);

    // Act & Assert
    assert!(preset.finish().contains("explicit"));
    assert!(!preset.finish().contains("from-env"));
    assert!(filled.finish().contains(r#""capsula_run_id": "from-env""#));
    assert!(
        !blank.finish().contains("capsula_run_id"),
        "blank or absent env values fill nothing"
    );
}

proptest! {
    /// Every file the builder emits is accepted by the shared parser, with
    /// the same number of tests coming back out.
    #[test]
    fn emitted_evidence_always_parses(
        entries in proptest::collection::btree_map(
            "[a-zA-Z0-9:/_.-]{1,30}",
            ("\\PC{1,60}", proptest::collection::btree_map("[a-z0-9=_-]{1,12}", proptest::option::of(0usize..4), 1..4)),
            1..5,
        ),
    ) {
        let outcomes = [Outcome::Pass, Outcome::Fail, Outcome::Skip, Outcome::Cancelled];
        let mut builder = EvidenceBuilder::new();
        let mut expected = 0;
        for (id, (body, cases)) in &entries {
            let executions: Vec<Execution> = cases
                .iter()
                .map(|(case, outcome)| Execution {
                    case: case.clone(),
                    outcome: outcome.map(|index| outcomes[index]),
                })
                .collect();
            if builder.add_test(id.clone(), body.clone(), executions).is_ok() {
                expected += 1;
            }
        }

        let parsed = parse(&builder.finish()).expect("emitted evidence must always parse");
        prop_assert_eq!(parsed.len(), expected);
    }
}
