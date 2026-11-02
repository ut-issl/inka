//! Bind evaluation against imported test evidence.
//!
//! Each test mirrors a row of the evaluation-example table in
//! `docs/en/design/test-evidence.md`: a bind is established only when a
//! valid approval and all-required-pass evidence exist together, approval
//! staleness and evidence failure are independent causes, and a reference
//! without imported evidence is the per-bind missing-evidence outcome.

use std::fs;
use std::path::PathBuf;

use inka_core::eval::{BlameReason, Evaluation, evaluate};
use inka_core::evidence::{self, EvidenceSet};
use inka_core::hash::LocalView;
use inka_core::manifest::PhasesSection;
use inka_core::record::{Record, RecordFields, Target};
use inka_core::view::{ArgumentView, Evidence, EvidenceKind, Mode, ViewNode};

/// The bind under test and its contract fixtures.
const BIND_ID: &str = "hash_claim";
const DESCRIPTION: &str = "the hash survives a round trip";
const TEST_ID: &str = "inka/tests/hash_props.rs::roundtrip";
const BODY_A: &str = "#[test]\nfn roundtrip() { assert!(true); }\n";
const BODY_B: &str = "#[test]\nfn roundtrip() { assert!(really(true)); }\n";

/// A scratch directory holding one evidence file, cleaned up on drop.
struct Scratch {
    dir: PathBuf,
}

impl Scratch {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "inka-eval-evidence-test-{}-{name}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("the scratch directory should be creatable");
        Self { dir }
    }

    /// Loads an evidence set whose one test carries `body` and `executions`,
    /// written as raw JSON entries like `{"case": "default", "outcome": "pass"}`.
    fn evidence(&self, body: &str, executions: &[&str]) -> EvidenceSet {
        self.evidence_for(TEST_ID, body, executions)
    }

    fn evidence_for(&self, test_id: &str, body: &str, executions: &[&str]) -> EvidenceSet {
        let body_json = serde_json::to_string(body).expect("a JSON string");
        let entries = executions.join(", ");
        let contents = format!(
            r#"{{"v": 1, "tests": [{{"id": "{test_id}", "body": {body_json}, "executions": [{entries}]}}]}}"#
        );
        let path = self.dir.join("evidence.json");
        fs::write(&path, contents).expect("the evidence file should be writable");
        evidence::load_files(
            &self.dir,
            &[PathBuf::from("evidence.json")],
            &inka_core::read::DiskText,
        )
        .expect("the fixture evidence should load")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// A single-node argument: one bind referencing [`TEST_ID`].
fn bind_view() -> ArgumentView {
    let node = ViewNode {
        id: BIND_ID.to_owned(),
        mode: Mode::Bind,
        namespace: "test".to_owned(),
        description: DESCRIPTION.to_owned(),
        tags: Vec::new(),
        evidence: Some(Evidence {
            kind: EvidenceKind::Test,
            target: TEST_ID.to_owned(),
        }),
        deadline: None,
        children: Vec::new(),
    };
    ArgumentView {
        nodes: vec![node],
        edges: Vec::new(),
        roots: Vec::new(),
    }
}

fn phases() -> PhasesSection {
    PhasesSection {
        order: vec!["M1".to_owned()],
        current: "M1".to_owned(),
    }
}

/// An approval bound to the interface of `body` + `cases`, the way a human
/// approves the reviewed contract.
fn approval_for(body: &str, cases: &[&str]) -> Record {
    let interface = evidence::interface_hash(TEST_ID, body, cases);
    let view_hash = LocalView::bind(DESCRIPTION, None, interface).view_hash();
    Record::approve(
        RecordFields {
            target: Target::new(Mode::Bind, BIND_ID).expect("a valid target"),
            view_hash,
            approver: "iwata".to_owned(),
            key_id: "iwata-2026".to_owned(),
            timestamp: "2026-09-10T08:30:00Z".parse().expect("a valid timestamp"),
            phase: "M1".to_owned(),
            until: None,
            policy_hash: None,
        },
        None,
    )
    .expect("a valid record")
}

fn run(records: &[Record], evidence: &EvidenceSet) -> Evaluation {
    evaluate(&bind_view(), &phases(), records, evidence).expect("evaluation runs")
}

fn bind_reasons(evaluation: &Evaluation) -> Vec<BlameReason> {
    evaluation
        .blame
        .iter()
        .find(|blame| blame.node == BIND_ID)
        .map(|blame| blame.reasons.clone())
        .unwrap_or_default()
}

fn bind_established(evaluation: &Evaluation) -> bool {
    evaluation
        .nodes
        .iter()
        .find(|node| node.id == BIND_ID)
        .expect("the bind has a verdict")
        .established
}

#[test]
fn matching_approval_and_all_required_passes_establish_the_bind() {
    // Table row: contract body A, matching approval, all required pass.
    let scratch = Scratch::new("established");
    let evidence = scratch.evidence(
        BODY_A,
        &[
            r#"{"case": "n=1", "outcome": "pass"}"#,
            r#"{"case": "n=2", "outcome": "pass"}"#,
        ],
    );
    let records = [approval_for(BODY_A, &["n=1", "n=2"])];

    let evaluation = run(&records, &evidence);

    assert!(bind_established(&evaluation));
    assert_eq!(bind_reasons(&evaluation), []);
}

#[test]
fn a_failing_required_execution_blames_the_evidence_not_the_approval() {
    // Table rows: matching approval + required fail; approval stays valid.
    let scratch = Scratch::new("failed");
    let evidence = scratch.evidence(
        BODY_A,
        &[
            r#"{"case": "n=1", "outcome": "pass"}"#,
            r#"{"case": "n=2", "outcome": "fail"}"#,
        ],
    );
    let records = [approval_for(BODY_A, &["n=1", "n=2"])];

    let evaluation = run(&records, &evidence);

    assert!(!bind_established(&evaluation));
    assert_eq!(
        bind_reasons(&evaluation),
        [BlameReason::EvidenceFailed {
            cases: vec!["n=2".to_owned()]
        }],
        "a test failure must not read as a stale or missing approval"
    );
}

#[test]
fn skipped_cancelled_and_missing_results_blame_incomplete_evidence() {
    // Table row: required case missing or skipped — insufficient, nonpass.
    let scratch = Scratch::new("incomplete");
    let evidence = scratch.evidence(
        BODY_A,
        &[
            r#"{"case": "a", "outcome": "skip"}"#,
            r#"{"case": "b", "outcome": "cancelled"}"#,
            r#"{"case": "c"}"#,
        ],
    );
    let records = [approval_for(BODY_A, &["a", "b", "c"])];

    let evaluation = run(&records, &evidence);

    assert!(!bind_established(&evaluation));
    assert_eq!(
        bind_reasons(&evaluation),
        [BlameReason::EvidenceIncomplete {
            cases: vec!["a".to_owned(), "b".to_owned(), "c".to_owned()]
        }]
    );
}

#[test]
fn passing_evidence_without_approval_blames_the_missing_approval_only() {
    // Table row: passing evidence, no approval.
    let scratch = Scratch::new("no-approval");
    let evidence = scratch.evidence(BODY_A, &[r#"{"case": "default", "outcome": "pass"}"#]);

    let evaluation = run(&[], &evidence);

    assert!(!bind_established(&evaluation));
    assert_eq!(bind_reasons(&evaluation), [BlameReason::MissingApproval]);
}

#[test]
fn an_approval_of_the_old_body_is_stale_even_when_the_new_run_passes() {
    // Table row: body B imported; only A was approved.
    let scratch = Scratch::new("stale-body");
    let evidence = scratch.evidence(BODY_B, &[r#"{"case": "default", "outcome": "pass"}"#]);
    let records = [approval_for(BODY_A, &["default"])];

    let evaluation = run(&records, &evidence);

    assert!(!bind_established(&evaluation));
    assert_eq!(
        bind_reasons(&evaluation),
        [BlameReason::StaleApproval {
            approvers: vec!["iwata".to_owned()]
        }],
        "a stale approval must not read as a test failure"
    );
}

#[test]
fn changing_the_required_case_set_stales_the_approval() {
    // The required set is part of the interface: silently adding a case (or
    // dropping a failing one) demands reapproval even with the same body.
    let scratch = Scratch::new("stale-cases");
    let evidence = scratch.evidence(
        BODY_A,
        &[
            r#"{"case": "n=1", "outcome": "pass"}"#,
            r#"{"case": "n=2", "outcome": "pass"}"#,
        ],
    );
    let records = [approval_for(BODY_A, &["n=1"])];

    let evaluation = run(&records, &evidence);

    assert!(!bind_established(&evaluation));
    assert_eq!(
        bind_reasons(&evaluation),
        [BlameReason::StaleApproval {
            approvers: vec!["iwata".to_owned()]
        }]
    );
}

#[test]
fn a_new_failing_run_keeps_the_approval_valid_and_reports_both_causes_independently() {
    // Table row: same body A, new failing execution — approval stays valid,
    // and only the evidence cause is reported.
    let scratch = Scratch::new("fail-after-approve");
    let evidence = scratch.evidence(BODY_A, &[r#"{"case": "default", "outcome": "fail"}"#]);
    let records = [approval_for(BODY_A, &["default"])];

    let evaluation = run(&records, &evidence);

    assert!(!bind_established(&evaluation));
    assert_eq!(
        bind_reasons(&evaluation),
        [BlameReason::EvidenceFailed {
            cases: vec!["default".to_owned()]
        }]
    );
}

#[test]
fn an_absent_test_id_is_missing_evidence_not_an_input_error() {
    // Table row: referenced evidence is absent.
    let evaluation = run(&[], &EvidenceSet::default());

    assert!(!bind_established(&evaluation));
    assert_eq!(
        bind_reasons(&evaluation),
        [BlameReason::MissingEvidence {
            target: TEST_ID.to_owned()
        }]
    );
}

#[test]
fn a_veriq_bind_stays_unevaluated() {
    let mut view = bind_view();
    view.nodes[0].evidence = Some(Evidence {
        kind: EvidenceKind::Veriq,
        target: "aocs.verify_detumble".to_owned(),
    });

    let evaluation =
        evaluate(&view, &phases(), &[], &EvidenceSet::default()).expect("evaluation runs");

    assert!(!bind_established(&evaluation));
    assert_eq!(bind_reasons(&evaluation), [BlameReason::VeriqNotEvaluated]);
}

#[test]
fn the_same_body_under_a_different_test_id_stales_the_approval() {
    // Table row: same body under a different selected test-id — the identity
    // is part of the interface, so re-pointing the bind demands reapproval.
    let scratch = Scratch::new("different-id");
    let renamed = "inka/tests/hash_props.rs::renamed";
    let evidence = scratch.evidence_for(
        renamed,
        BODY_A,
        &[r#"{"case": "default", "outcome": "pass"}"#],
    );
    let mut view = bind_view();
    view.nodes[0].evidence = Some(Evidence {
        kind: EvidenceKind::Test,
        target: renamed.to_owned(),
    });
    let records = [approval_for(BODY_A, &["default"])];

    let evaluation = evaluate(&view, &phases(), &records, &evidence).expect("evaluation runs");

    assert!(!bind_established(&evaluation));
    assert_eq!(
        bind_reasons(&evaluation),
        [BlameReason::StaleApproval {
            approvers: vec!["iwata".to_owned()]
        }]
    );
}
