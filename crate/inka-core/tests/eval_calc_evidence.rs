//! Bind evaluation against imported calc evidence.
//!
//! Each test mirrors a row of the evaluation tables in
//! `docs/en/design/calc-evidence.md`: a calc bind is established only when a
//! valid approval, passing evidence and established assumptions exist
//! together; approval staleness, evidence failure and an undischarged
//! assumption are independent causes; the assumed nodes' *descriptions*, not
//! their ids, enter the bind hash; and a target absent from the evidence is
//! the per-bind missing-evidence outcome.

use inka_core::calc_evidence::{self, CalcEvidenceSet};
use inka_core::eval::{BlameReason, EvalError, Evaluation, evaluate};
use inka_core::evidence::EvidenceSet;
use inka_core::hash::LocalView;
use inka_core::manifest::PhasesSection;
use inka_core::record::{Record, RecordFields, Target as RecordTarget};
use inka_core::view::{ArgumentView, Evidence, EvidenceKind, Mode, NodeComments, ViewNode};
use inka_evidence::calc::{Definition, DefinitionKind, Leaf, Target};

/// The bind under test and its contract fixtures.
const BIND_ID: &str = "margin_ok";
const DESCRIPTION: &str = "the power margin stays positive in every phase";
const TARGET: &str = "Power::?verify_margin";
const VERIFY_SOURCE: &str =
    "@power.verification()\ndef verify_margin(b: Budget) -> bool:\n    return b.margin_w > 0\n";
const BUDGET_A: &str = "@power.calculation()\ndef budget(c: Config) -> Budget:\n    return Budget(margin_w=c.generated_w - c.consumed_w)\n";
const BUDGET_B: &str = "@power.calculation()\ndef budget(c: Config) -> Budget:\n    return Budget(margin_w=c.generated_w * 0.9 - c.consumed_w)\n";

/// The assumed fiat and its contract text.
const ASSUMED_ID: &str = "eclipse_model_valid";
const ASSUMED_DESCRIPTION: &str = "the eclipse model envelopes the actual eclipse durations";

fn node(id: &str, mode: Mode, description: &str, children: &[&str]) -> ViewNode {
    ViewNode {
        id: id.to_owned(),
        mode,
        namespace: "power".to_owned(),
        description: description.to_owned(),
        tags: Vec::new(),
        evidence: (mode == Mode::Bind).then(|| Evidence {
            kind: EvidenceKind::Calc,
            target: TARGET.to_owned(),
        }),
        deadline: None,
        children: children.iter().map(|&child| child.to_owned()).collect(),
        comments: NodeComments::default(),
    }
}

/// An objective over the calc bind and the fiat its evidence may assume.
fn view_with(assumed_id: &str, assumed_description: &str) -> ArgumentView {
    ArgumentView {
        nodes: vec![
            node(assumed_id, Mode::Fiat, assumed_description, &[]),
            node(BIND_ID, Mode::Bind, DESCRIPTION, &[]),
            node(
                "root",
                Mode::Objective,
                "the mission succeeds",
                &[BIND_ID, assumed_id],
            ),
        ],
        edges: Vec::new(),
        roots: vec!["root".to_owned()],
    }
}

fn view() -> ArgumentView {
    view_with(ASSUMED_ID, ASSUMED_DESCRIPTION)
}

fn phases() -> PhasesSection {
    PhasesSection {
        order: vec!["M1".to_owned()],
        current: "M1".to_owned(),
    }
}

/// One calc target: the verification over `budget_source`, with the given
/// assumed ids and leaves.
fn target(budget_source: &str, assumes: &[&str], leaves: Vec<Leaf>) -> Target {
    Target {
        path: TARGET.to_owned(),
        definitions: vec![
            Definition::new(TARGET, DefinitionKind::Verification, VERIFY_SOURCE),
            Definition::new("Power::@budget", DefinitionKind::Calculation, budget_source),
        ],
        assumes: assumes.iter().map(|&id| id.to_owned()).collect(),
        leaves,
        valid: true,
        xfail: false,
    }
}

fn passing(budget_source: &str, assumes: &[&str]) -> Target {
    target(budget_source, assumes, vec![Leaf::new(TARGET, Some(true))])
}

fn calcs(target: Target) -> CalcEvidenceSet {
    CalcEvidenceSet::from_targets([target]).expect("one target is a valid set")
}

/// The interface hash of the verification over `budget_source`.
fn interface(budget_source: &str) -> inka_core::hash::InterfaceHash {
    calc_evidence::interface_hash(
        TARGET,
        &[
            calc_evidence::Definition {
                path: TARGET,
                source: VERIFY_SOURCE,
            },
            calc_evidence::Definition {
                path: "Power::@budget",
                source: budget_source,
            },
        ],
        &[TARGET],
    )
}

fn record(kind: Mode, id: &str, view_hash: inka_core::hash::ViewHash) -> Record {
    Record::approve(
        RecordFields {
            target: RecordTarget::new(kind, id).expect("a valid target"),
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

/// An approval of the bind bound to the interface of `budget_source` and the
/// given assumed descriptions, the way a reviewer approves what is shown.
fn bind_approval(budget_source: &str, assumptions: &[&str]) -> Record {
    let hash = LocalView::bind(
        DESCRIPTION,
        None,
        interface(budget_source),
        assumptions.iter().copied(),
    )
    .with_comments(&NodeComments::default())
    .view_hash();
    record(Mode::Bind, BIND_ID, hash)
}

/// An approval of the fiat as the view describes it.
fn fiat_approval(view: &ArgumentView, id: &str) -> Record {
    let node = view.node(id).expect("the view defines the fiat");
    let hash = LocalView::of_node(view, node, None)
        .expect("a fiat hashes")
        .view_hash();
    record(Mode::Fiat, id, hash)
}

fn run(view: &ArgumentView, records: &[Record], calcs: &CalcEvidenceSet) -> Evaluation {
    evaluate(view, &phases(), records, &EvidenceSet::default(), calcs).expect("evaluation runs")
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

fn stale() -> BlameReason {
    BlameReason::StaleApproval {
        approvers: vec!["iwata".to_owned()],
    }
}

#[test]
fn matching_approval_passing_leaves_and_established_assumptions_establish_the_bind() {
    // Table row: sources A, matching approval, all leaves pass, assumptions
    // established.
    let view = view();
    let records = [
        bind_approval(BUDGET_A, &[ASSUMED_DESCRIPTION]),
        fiat_approval(&view, ASSUMED_ID),
    ];

    let evaluation = run(&view, &records, &calcs(passing(BUDGET_A, &[ASSUMED_ID])));

    assert!(bind_established(&evaluation));
    assert_eq!(bind_reasons(&evaluation), []);
}

#[test]
fn a_false_leaf_blames_the_evidence_not_the_approval() {
    // Table rows: matching approval, a leaf fails — and a new failing
    // evaluation of the same sources keeps the approval valid.
    let view = view();
    let evidence = calcs(target(
        BUDGET_A,
        &[ASSUMED_ID],
        vec![
            Leaf::new("Power::?verify_margin[daylight]", Some(true)),
            Leaf::new("Power::?verify_margin[eclipse]", Some(false)),
        ],
    ));
    // The leaf set is part of the interface; approve the two-leaf one.
    let two_leaf_interface = calc_evidence::interface_hash(
        TARGET,
        &[
            calc_evidence::Definition {
                path: TARGET,
                source: VERIFY_SOURCE,
            },
            calc_evidence::Definition {
                path: "Power::@budget",
                source: BUDGET_A,
            },
        ],
        &[
            "Power::?verify_margin[daylight]",
            "Power::?verify_margin[eclipse]",
        ],
    );
    let records = [
        record(
            Mode::Bind,
            BIND_ID,
            LocalView::bind(DESCRIPTION, None, two_leaf_interface, [ASSUMED_DESCRIPTION])
                .with_comments(&NodeComments::default())
                .view_hash(),
        ),
        fiat_approval(&view, ASSUMED_ID),
    ];

    let evaluation = run(&view, &records, &evidence);

    assert!(!bind_established(&evaluation));
    assert_eq!(
        bind_reasons(&evaluation),
        [BlameReason::LeavesFailed {
            leaves: vec!["Power::?verify_margin[eclipse]".to_owned()]
        }],
        "a false leaf must not read as a stale or missing approval"
    );
}

#[test]
fn a_leaf_without_a_result_does_not_establish() {
    // Table row: a required leaf has no result.
    let view = view();
    let records = [
        bind_approval(BUDGET_A, &[ASSUMED_DESCRIPTION]),
        fiat_approval(&view, ASSUMED_ID),
    ];
    let evidence = calcs(target(
        BUDGET_A,
        &[ASSUMED_ID],
        vec![Leaf::new(TARGET, None)],
    ));

    let evaluation = run(&view, &records, &evidence);

    assert!(!bind_established(&evaluation));
    assert_eq!(
        bind_reasons(&evaluation),
        [BlameReason::LeavesMissing {
            leaves: vec![TARGET.to_owned()]
        }]
    );
}

#[test]
fn an_invalid_result_does_not_establish_even_when_the_leaf_is_true() {
    // Table row: `valid` is false — the producer's own assumption failed, so
    // the returned `true` is not evidence.
    let view = view();
    let records = [
        bind_approval(BUDGET_A, &[ASSUMED_DESCRIPTION]),
        fiat_approval(&view, ASSUMED_ID),
    ];
    let mut target = passing(BUDGET_A, &[ASSUMED_ID]);
    target.valid = false;

    let evaluation = run(&view, &records, &calcs(target));

    assert!(!bind_established(&evaluation));
    assert_eq!(bind_reasons(&evaluation), [BlameReason::EvidenceInvalid]);
}

#[test]
fn an_expected_failure_does_not_establish() {
    // Table row: `xfail` is true.
    let view = view();
    let records = [
        bind_approval(BUDGET_A, &[ASSUMED_DESCRIPTION]),
        fiat_approval(&view, ASSUMED_ID),
    ];
    let mut target = target(
        BUDGET_A,
        &[ASSUMED_ID],
        vec![Leaf::new(TARGET, Some(false))],
    );
    target.xfail = true;

    let evaluation = run(&view, &records, &calcs(target));

    assert!(!bind_established(&evaluation));
    assert_eq!(
        bind_reasons(&evaluation),
        [
            BlameReason::LeavesFailed {
                leaves: vec![TARGET.to_owned()]
            },
            BlameReason::ExpectedFailure,
        ]
    );
}

#[test]
fn an_absent_target_is_missing_evidence_not_an_input_error() {
    // Table row: the referenced target path is absent from the evidence.
    let view = view();

    let evaluation = run(&view, &[], &CalcEvidenceSet::default());

    assert!(!bind_established(&evaluation));
    assert_eq!(
        bind_reasons(&evaluation),
        [BlameReason::MissingEvidence {
            kind: EvidenceKind::Calc,
            target: TARGET.to_owned()
        }]
    );
}

#[test]
fn passing_evidence_without_approval_blames_the_missing_approval_only() {
    // Table row: passing evidence, no approval of the bind.
    let view = view();
    let records = [fiat_approval(&view, ASSUMED_ID)];

    let evaluation = run(&view, &records, &calcs(passing(BUDGET_A, &[ASSUMED_ID])));

    assert!(!bind_established(&evaluation));
    assert_eq!(bind_reasons(&evaluation), [BlameReason::MissingApproval]);
}

#[test]
fn an_unapproved_assumed_fiat_leaves_the_bind_unestablished_naming_the_node() {
    // Table row: same sources, an assumed fiat not yet approved.
    let view = view();
    let records = [bind_approval(BUDGET_A, &[ASSUMED_DESCRIPTION])];

    let evaluation = run(&view, &records, &calcs(passing(BUDGET_A, &[ASSUMED_ID])));

    assert!(!bind_established(&evaluation));
    assert_eq!(
        bind_reasons(&evaluation),
        [BlameReason::AssumptionNotEstablished {
            node: ASSUMED_ID.to_owned()
        }]
    );
    // The fiat is blamed for itself; the bind's blame names it as the cause.
    let fiat = evaluation
        .blame
        .iter()
        .find(|blame| blame.node == ASSUMED_ID)
        .expect("the fiat is blamed");
    assert_eq!(fiat.reasons, [BlameReason::MissingApproval]);
}

#[test]
fn editing_an_upstream_calculation_stales_the_approval() {
    // Table row: an upstream calculation edited; only the old sources were
    // approved. A new passing run does not help.
    let view = view();
    let records = [
        bind_approval(BUDGET_A, &[ASSUMED_DESCRIPTION]),
        fiat_approval(&view, ASSUMED_ID),
    ];

    let evaluation = run(&view, &records, &calcs(passing(BUDGET_B, &[ASSUMED_ID])));

    assert!(!bind_established(&evaluation));
    assert_eq!(
        bind_reasons(&evaluation),
        [stale()],
        "a stale approval must not read as an evidence failure"
    );
}

#[test]
fn editing_an_assumed_nodes_description_stales_the_bind_approval() {
    // Table row: same sources, an assumed node's description edited. The
    // fiat is re-approved; the bind's approval covered the old wording.
    let reworded = view_with(
        ASSUMED_ID,
        "the eclipse model envelopes every eclipse, margins included",
    );
    let records = [
        bind_approval(BUDGET_A, &[ASSUMED_DESCRIPTION]),
        fiat_approval(&reworded, ASSUMED_ID),
    ];

    let evaluation = run(
        &reworded,
        &records,
        &calcs(passing(BUDGET_A, &[ASSUMED_ID])),
    );

    assert!(!bind_established(&evaluation));
    assert_eq!(bind_reasons(&evaluation), [stale()]);
}

#[test]
fn renaming_an_assumed_node_keeps_the_bind_approval_valid() {
    // Table row: same sources, an assumed node renamed — ids never enter
    // the hash. The evidence names the new id; the fiat is re-approved under
    // its new target id.
    let renamed = view_with("eclipse_model_envelopes", ASSUMED_DESCRIPTION);
    let records = [
        bind_approval(BUDGET_A, &[ASSUMED_DESCRIPTION]),
        fiat_approval(&renamed, "eclipse_model_envelopes"),
    ];

    let evaluation = run(
        &renamed,
        &records,
        &calcs(passing(BUDGET_A, &["eclipse_model_envelopes"])),
    );

    assert!(bind_established(&evaluation));
}

#[test]
fn a_new_assumption_upstream_stales_the_bind_approval() {
    // Table row: same sources, a new `assumes` entry added upstream. The
    // set — as descriptions — is contract content.
    let mut view = view();
    view.nodes.insert(
        0,
        node(
            "battery_model_valid",
            Mode::Fiat,
            "the battery model is conservative",
            &[],
        ),
    );
    let records = [
        bind_approval(BUDGET_A, &[ASSUMED_DESCRIPTION]),
        fiat_approval(&view, ASSUMED_ID),
        fiat_approval(&view, "battery_model_valid"),
    ];

    let evaluation = run(
        &view,
        &records,
        &calcs(passing(BUDGET_A, &[ASSUMED_ID, "battery_model_valid"])),
    );

    assert!(!bind_established(&evaluation));
    assert_eq!(bind_reasons(&evaluation), [stale()]);
}

#[test]
fn the_assumption_order_the_producer_lists_is_not_content() {
    // The approval was bound with the assumptions in one order; the producer
    // lists them in another.
    let mut view = view();
    view.nodes.insert(
        0,
        node(
            "battery_model_valid",
            Mode::Fiat,
            "the battery model is conservative",
            &[],
        ),
    );
    let records = [
        bind_approval(
            BUDGET_A,
            &["the battery model is conservative", ASSUMED_DESCRIPTION],
        ),
        fiat_approval(&view, ASSUMED_ID),
        fiat_approval(&view, "battery_model_valid"),
    ];

    let evaluation = run(
        &view,
        &records,
        &calcs(passing(BUDGET_A, &[ASSUMED_ID, "battery_model_valid"])),
    );

    assert!(bind_established(&evaluation));
}

#[test]
fn the_same_sources_under_a_different_target_path_stale_the_approval() {
    // Table row: same sources under a different target path.
    let mut view = view();
    view.nodes[1].evidence = Some(Evidence {
        kind: EvidenceKind::Calc,
        target: "Power::?verify_margin_v2".to_owned(),
    });
    let mut target = passing(BUDGET_A, &[ASSUMED_ID]);
    target.path = "Power::?verify_margin_v2".to_owned();
    target.definitions[0].path = "Power::?verify_margin_v2".to_owned();
    target.leaves = vec![Leaf::new("Power::?verify_margin_v2", Some(true))];
    let records = [
        bind_approval(BUDGET_A, &[ASSUMED_DESCRIPTION]),
        fiat_approval(&view, ASSUMED_ID),
    ];

    let evaluation = run(&view, &records, &calcs(target));

    assert!(!bind_established(&evaluation));
    assert_eq!(bind_reasons(&evaluation), [stale()]);
}

#[test]
fn an_assumption_the_view_does_not_define_is_an_input_error() {
    // A checked project cannot reach this: `inka check` resolves every
    // assumption at load time. A hand-built view can.
    let view = view();

    let error = evaluate(
        &view,
        &phases(),
        &[],
        &EvidenceSet::default(),
        &calcs(passing(BUDGET_A, &["ghost"])),
    )
    .expect_err("ghost is not a node");

    assert_eq!(
        error,
        EvalError::UnknownAssumption {
            bind: BIND_ID.to_owned(),
            id: "ghost".to_owned(),
        }
    );
}

#[test]
fn a_bind_assuming_its_own_ancestor_is_a_cycle() {
    // The assumption edge closes a cycle through `root`; a checked project
    // cannot contain one, so evaluation refuses rather than guesses.
    let view = view();

    let error = evaluate(
        &view,
        &phases(),
        &[],
        &EvidenceSet::default(),
        &calcs(passing(BUDGET_A, &["root"])),
    )
    .expect_err("a cycle cannot be evaluated");

    assert_eq!(error, EvalError::Cycle);
}
