//! The calc emission guarantee: whatever `CalcEvidenceBuilder::finish`
//! returns, the shared `calc::parse` accepts and the repository schema
//! admits, so the veriq adapter cannot produce a file the consumer rejects.
//! Invalid inputs fail at `add_target` time, and the builder, the parser and
//! the JSON Schema agree on what is invalid.

use inka_evidence::calc::{
    CalcEvidenceBuilder, CalcParseError, Definition, DefinitionKind, InvalidCalcEvidence, Leaf,
    Target, parse,
};
use proptest::prelude::*;

const TARGET: &str = "Power::?verify_margin";

fn definition(path: &str, kind: DefinitionKind, source: &str) -> Definition {
    Definition::new(path, kind, source)
}

/// A representative target: three definitions, one assumption, two leaves.
fn representative() -> Target {
    Target {
        path: TARGET.to_owned(),
        definitions: vec![
            definition(
                "Power::$Config",
                DefinitionKind::Model,
                "class Config(BaseModel): ...",
            ),
            definition(
                "Power::@budget",
                DefinitionKind::Calculation,
                "def budget(config: Config) -> PowerBudget: ...",
            ),
            definition(
                TARGET,
                DefinitionKind::Verification,
                "def verify_margin(budget: PowerBudget) -> Table[Phase, bool]: ...",
            ),
        ],
        assumes: vec!["power::eclipse_model_valid".to_owned()],
        leaves: vec![
            Leaf::new("Power::?verify_margin.daylight", Some(true)),
            Leaf::new("Power::?verify_margin.eclipse", None),
        ],
        valid: true,
        xfail: false,
    }
}

/// The smallest valid target: itself as the only definition, one leaf.
fn minimal(path: &str) -> Target {
    Target {
        path: path.to_owned(),
        definitions: vec![definition(path, DefinitionKind::Verification, "s")],
        assumes: Vec::new(),
        leaves: vec![Leaf::new(path, Some(true))],
        valid: true,
        xfail: false,
    }
}

/// The repository schema, compiled once for every conformance check.
static SCHEMA: std::sync::LazyLock<jsonschema::Validator> = std::sync::LazyLock::new(|| {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../schemas/calc-evidence-v1.schema.json");
    let text = std::fs::read_to_string(path).expect("the repository schema should be readable");
    let schema: serde_json::Value =
        serde_json::from_str(&text).expect("the repository schema should be valid JSON");
    jsonschema::validator_for(&schema).expect("the repository schema should compile")
});

fn assert_conforms(text: &str) {
    let value: serde_json::Value = serde_json::from_str(text).expect("valid JSON");
    assert!(
        SCHEMA.is_valid(&value),
        "emitted evidence violates the repository schema: {:?}",
        SCHEMA.iter_errors(&value).next()
    );
}

#[test]
fn built_evidence_parses_back_into_the_same_targets() {
    // Arrange
    let mut builder = CalcEvidenceBuilder::new();
    builder
        .provenance("source_revision", "4f2a1c9")
        .expect("a nonblank entry is accepted");
    builder.capsula_run_id(Some("01K8WSYC91YAE21R7CWHQ4KYN2".to_owned()));
    builder
        .add_target(representative())
        .expect("a valid target is accepted");
    let mut expected_failure = minimal("Power::?verify_xfail");
    expected_failure.xfail = true;
    expected_failure.valid = false;
    builder
        .add_target(expected_failure.clone())
        .expect("a valid target is accepted");

    // Act
    let emitted = builder.finish();
    let parsed = parse(&emitted).expect("emitted evidence must always parse");

    // Assert
    assert_eq!(parsed, [representative(), expected_failure]);
    assert_conforms(&emitted);
}

#[test]
fn the_version_is_stamped_and_the_output_ends_with_a_newline() {
    let mut builder = CalcEvidenceBuilder::new();
    builder
        .add_target(minimal(TARGET))
        .expect("a valid target is accepted");

    let emitted = builder.finish();

    let value: serde_json::Value = serde_json::from_str(&emitted).expect("valid JSON");
    assert_eq!(value["v"], 1);
    assert!(emitted.ends_with('\n'));
}

#[test]
fn defaults_are_omitted_keys_not_nulls_or_falses() {
    // The schema spells a missing result as an absent `result` and an
    // ordinary target as an absent `xfail`.
    let mut builder = CalcEvidenceBuilder::new();
    let mut target = minimal(TARGET);
    target.leaves[0].result = None;
    builder
        .add_target(target)
        .expect("a declared-but-missing result is valid input");

    let emitted = builder.finish();

    assert!(!emitted.contains("result"), "got: {emitted}");
    assert!(!emitted.contains("null"), "got: {emitted}");
    assert!(!emitted.contains("xfail"), "got: {emitted}");
    assert!(!emitted.contains("provenance"), "got: {emitted}");
    assert_conforms(&emitted);
}

#[test]
fn invalid_targets_are_rejected_at_add_time_and_leave_the_builder_unchanged() {
    let mut builder = CalcEvidenceBuilder::new();
    let target = || TARGET.to_owned();

    let mut spaced = minimal(TARGET);
    spaced.path = "Power:: ?verify_margin".to_owned();
    assert_eq!(
        builder.add_target(spaced),
        Err(InvalidCalcEvidence::InvalidTargetPath)
    );

    let mut undefined = minimal(TARGET);
    undefined.definitions.clear();
    assert_eq!(
        builder.add_target(undefined),
        Err(InvalidCalcEvidence::NoDefinitions { target: target() })
    );

    let mut blank_path = minimal(TARGET);
    blank_path.definitions[0].path = String::new();
    assert_eq!(
        builder.add_target(blank_path),
        Err(InvalidCalcEvidence::InvalidDefinitionPath { target: target() })
    );

    let mut twice = minimal(TARGET);
    twice.definitions.push(twice.definitions[0].clone());
    assert_eq!(
        builder.add_target(twice),
        Err(InvalidCalcEvidence::DuplicateDefinition {
            target: target(),
            path: target()
        })
    );

    let mut empty_source = minimal(TARGET);
    empty_source.definitions[0].source = String::new();
    assert_eq!(
        builder.add_target(empty_source),
        Err(InvalidCalcEvidence::EmptySource {
            target: target(),
            path: target()
        })
    );

    let mut elsewhere = minimal(TARGET);
    elsewhere.definitions[0].path = "Power::@budget".to_owned();
    assert_eq!(
        builder.add_target(elsewhere),
        Err(InvalidCalcEvidence::TargetNotDefined { target: target() })
    );

    let mut dotted = minimal(TARGET);
    dotted.assumes.push("power.eclipse".to_owned());
    assert_eq!(
        builder.add_target(dotted),
        Err(InvalidCalcEvidence::InvalidAssumptionId {
            target: target(),
            id: "power.eclipse".to_owned()
        })
    );

    let mut assumed_twice = minimal(TARGET);
    assumed_twice.assumes = vec!["a".to_owned(), "a".to_owned()];
    assert_eq!(
        builder.add_target(assumed_twice),
        Err(InvalidCalcEvidence::DuplicateAssumption {
            target: target(),
            id: "a".to_owned()
        })
    );

    let mut leafless = minimal(TARGET);
    leafless.leaves.clear();
    assert_eq!(
        builder.add_target(leafless),
        Err(InvalidCalcEvidence::NoLeaves { target: target() })
    );

    let mut blank_leaf = minimal(TARGET);
    blank_leaf.leaves[0].id = " ".to_owned();
    assert_eq!(
        builder.add_target(blank_leaf),
        Err(InvalidCalcEvidence::InvalidLeafId { target: target() })
    );

    let mut leaf_twice = minimal(TARGET);
    leaf_twice.leaves.push(Leaf::new(TARGET, Some(false)));
    assert_eq!(
        builder.add_target(leaf_twice),
        Err(InvalidCalcEvidence::DuplicateLeaf {
            target: target(),
            id: target()
        })
    );

    // Nothing invalid leaked in: the file is still emittable and empty.
    assert_eq!(parse(&builder.finish()).expect("an empty file parses"), []);
}

#[test]
fn a_duplicate_target_path_across_calls_is_rejected() {
    let mut builder = CalcEvidenceBuilder::new();
    builder
        .add_target(minimal(TARGET))
        .expect("the first target is accepted");
    let before = builder.finish();

    let error = builder
        .add_target(minimal(TARGET))
        .expect_err("the same path twice is a routing conflict");

    assert_eq!(
        error,
        InvalidCalcEvidence::DuplicateTarget {
            target: TARGET.to_owned()
        }
    );
    assert_eq!(builder.finish(), before);
}

#[test]
fn the_parser_rejects_what_the_builder_rejects() {
    // A hand-assembled file with a duplicate target path.
    let one = serde_json::to_value(minimal(TARGET)).expect("serializable");
    let text = serde_json::json!({"v": 1, "targets": [one.clone(), one]}).to_string();

    let error = parse(&text).expect_err("a duplicate path is a routing conflict");

    assert!(matches!(
        error,
        CalcParseError::Invalid(InvalidCalcEvidence::DuplicateTarget { .. })
    ));
}

#[test]
fn blank_provenance_entries_are_rejected() {
    let mut builder = CalcEvidenceBuilder::new();

    assert_eq!(
        builder.provenance("source_revision", "  "),
        Err(InvalidCalcEvidence::BlankProvenance {
            key: "source_revision".to_owned()
        })
    );
    assert!(!builder.finish().contains("provenance"));
}

#[test]
fn capsula_run_id_fills_only_when_absent() {
    let mut preset = CalcEvidenceBuilder::new();
    preset
        .provenance("capsula_run_id", "explicit")
        .expect("a nonblank entry is accepted");
    preset.capsula_run_id(Some("from-env".to_owned()));

    let mut blank = CalcEvidenceBuilder::new();
    blank.capsula_run_id(Some("   ".to_owned()));
    blank.capsula_run_id(None);

    assert!(preset.finish().contains("explicit"));
    assert!(!preset.finish().contains("from-env"));
    assert!(!blank.finish().contains("capsula_run_id"));
}

/// A stable name for one validation defect, for cross-direction comparison.
fn defect(reason: &InvalidCalcEvidence) -> &'static str {
    match reason {
        InvalidCalcEvidence::InvalidTargetPath => "invalid-target-path",
        InvalidCalcEvidence::NoDefinitions { .. } => "no-definitions",
        InvalidCalcEvidence::InvalidDefinitionPath { .. } => "invalid-definition-path",
        InvalidCalcEvidence::DuplicateDefinition { .. } => "duplicate-definition",
        InvalidCalcEvidence::EmptySource { .. } => "empty-source",
        InvalidCalcEvidence::TargetNotDefined { .. } => "target-not-defined",
        InvalidCalcEvidence::InvalidAssumptionId { .. } => "invalid-assumption-id",
        InvalidCalcEvidence::DuplicateAssumption { .. } => "duplicate-assumption",
        InvalidCalcEvidence::NoLeaves { .. } => "no-leaves",
        InvalidCalcEvidence::InvalidLeafId { .. } => "invalid-leaf-id",
        InvalidCalcEvidence::DuplicateLeaf { .. } => "duplicate-leaf",
        InvalidCalcEvidence::DuplicateTarget { .. } => "duplicate-target",
        InvalidCalcEvidence::BlankProvenance { .. } => "blank-provenance",
    }
}

/// Targets drawn from a small alphabet so that every defect is reachable:
/// blank and spaced paths, missing self-definitions, dotted assumption IDs,
/// repeated leaves.
fn arbitrary_target() -> impl Strategy<Value = Target> {
    let path = "[ P]{0,2}";
    let kinds = [
        DefinitionKind::Model,
        DefinitionKind::Calculation,
        DefinitionKind::Verification,
    ];
    (
        path,
        proptest::collection::vec((path, 0usize..3, "[ s]{0,2}"), 0..3),
        proptest::collection::vec("[a.]{0,2}", 0..3),
        proptest::collection::vec((path, proptest::option::of(any::<bool>())), 0..3),
        any::<bool>(),
        any::<bool>(),
    )
        .prop_map(
            move |(target, definitions, assumes, leaves, valid, xfail)| Target {
                path: target,
                definitions: definitions
                    .into_iter()
                    .map(|(path, kind, source)| Definition::new(path, kinds[kind], source))
                    .collect(),
                assumes,
                leaves: leaves
                    .into_iter()
                    .map(|(id, result)| Leaf::new(id, result))
                    .collect(),
                valid,
                xfail,
            },
        )
}

proptest! {
    /// Builder, parser and JSON Schema agree on arbitrary, possibly invalid
    /// input: the first defect the builder and the parser report is the
    /// same, the schema admits a file exactly when neither objects to it —
    /// except for the one rule the schema cannot express (a target being
    /// among its own definitions) — and whatever the builder emits conforms.
    #[test]
    fn builder_parser_and_schema_agree(
        targets in proptest::collection::vec(arbitrary_target(), 1..4),
    ) {
        let mut builder = CalcEvidenceBuilder::new();
        let mut builder_defect = None;
        for target in &targets {
            if let Err(reason) = builder.add_target(target.clone()) {
                builder_defect = Some(defect(&reason));
                break;
            }
        }

        let text = serde_json::json!({"v": 1, "targets": targets}).to_string();
        let parser_defect = match parse(&text) {
            Ok(parsed) => {
                prop_assert_eq!(parsed, targets.clone());
                None
            }
            Err(CalcParseError::Invalid(reason)) => Some(defect(&reason)),
            Err(other) => return Err(TestCaseError::fail(format!("unexpected: {other}"))),
        };
        prop_assert_eq!(builder_defect, parser_defect);

        let hand_made: serde_json::Value = serde_json::from_str(&text).expect("valid JSON");
        let schema_accepts = SCHEMA.is_valid(&hand_made);
        match builder_defect {
            None => prop_assert!(schema_accepts),
            Some("target-not-defined") => {}
            Some(_) => prop_assert!(!schema_accepts, "schema admitted a file the crate rejects: {text}"),
        }

        let emitted: serde_json::Value =
            serde_json::from_str(&builder.finish()).expect("valid JSON");
        prop_assert!(SCHEMA.is_valid(&emitted));
    }
}
