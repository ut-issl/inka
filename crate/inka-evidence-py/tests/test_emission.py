"""Behavioral tests for the `inka_evidence` Python binding.

The binding wraps the Rust `inka-evidence` crate, so these tests pin the
contract adapters rely on — the v1 envelope, the add-time rejections and the
provenance rules — not the validation logic itself, which the Rust crate
already proves against the JSON schema and the parser.
"""

import json
from pathlib import Path

import jsonschema
import pytest

from inka_evidence import EvidenceBuilder, Execution, InvalidEvidenceError, VERSION

SCHEMA = json.loads(
    (Path(__file__).parents[3] / "schemas" / "test-evidence-v1.schema.json").read_text()
)


def build_representative() -> EvidenceBuilder:
    builder = EvidenceBuilder()
    builder.provenance("source_revision", "0123abc")
    builder.add_test(
        "crate::case::passes",
        "#[test]\nfn passes() {}\n",
        [Execution("passes", "pass"), Execution("declared_only")],
    )
    builder.add_test(
        "crate::case::mixed",
        "body",
        [
            Execution("failing", "fail"),
            Execution("skipped", "skip"),
            Execution("cancelled", "cancelled"),
        ],
    )
    return builder


def test_finish_stamps_v1_and_ends_with_one_newline():
    builder = EvidenceBuilder()
    builder.add_test("t", "b", [Execution("e", "pass")])

    text = builder.finish()

    assert json.loads(text)["v"] == VERSION == 1
    assert text.endswith("\n") and not text.endswith("\n\n")


def test_emitted_file_conforms_to_the_json_schema():
    text = build_representative().finish()

    jsonschema.validate(json.loads(text), SCHEMA)


def test_missing_outcome_is_an_absent_key_not_null():
    builder = EvidenceBuilder()
    builder.add_test("t", "b", [Execution("declared_only")])

    execution = json.loads(builder.finish())["tests"][0]["executions"][0]

    assert execution == {"case": "declared_only"}


def test_empty_provenance_is_omitted_entirely():
    builder = EvidenceBuilder()
    builder.add_test("t", "b", [Execution("e")])

    assert "provenance" not in json.loads(builder.finish())


def test_provenance_replaces_an_earlier_value_under_the_same_key():
    builder = EvidenceBuilder()
    builder.provenance("source_revision", "old")
    builder.provenance("source_revision", "new")
    builder.add_test("t", "b", [Execution("e")])

    assert json.loads(builder.finish())["provenance"] == {"source_revision": "new"}


@pytest.mark.parametrize("key, value", [("", "value"), ("  ", "value"), ("key", ""), ("key", " ")])
def test_blank_provenance_key_or_value_is_rejected(key, value):
    builder = EvidenceBuilder()

    with pytest.raises(InvalidEvidenceError, match="blank key or value"):
        builder.provenance(key, value)


def test_capsula_run_id_fills_only_when_nonblank_and_unclaimed():
    filled = EvidenceBuilder()
    filled.capsula_run_id("run-1")
    filled.add_test("t", "b", [Execution("e")])
    assert json.loads(filled.finish())["provenance"] == {"capsula_run_id": "run-1"}

    untouched = EvidenceBuilder()
    untouched.capsula_run_id(None)
    untouched.capsula_run_id("   ")
    untouched.add_test("t", "b", [Execution("e")])
    assert "provenance" not in json.loads(untouched.finish())

    claimed = EvidenceBuilder()
    claimed.provenance("capsula_run_id", "explicit")
    claimed.capsula_run_id("run-2")
    claimed.add_test("t", "b", [Execution("e")])
    assert json.loads(claimed.finish())["provenance"] == {"capsula_run_id": "explicit"}


def test_capsula_env_reads_the_capsula_run_id_variable(monkeypatch):
    monkeypatch.setenv("CAPSULA_RUN_ID", "run-from-env")
    from_env = EvidenceBuilder()
    from_env.capsula_env()
    from_env.add_test("t", "b", [Execution("e")])
    assert json.loads(from_env.finish())["provenance"] == {"capsula_run_id": "run-from-env"}

    monkeypatch.delenv("CAPSULA_RUN_ID")
    unset = EvidenceBuilder()
    unset.capsula_env()
    unset.add_test("t", "b", [Execution("e")])
    assert "provenance" not in json.loads(unset.finish())


@pytest.mark.parametrize(
    "test_id, body, executions, message",
    [
        ("  ", "b", [Execution("e")], "test id is blank"),
        ("t", "", [Execution("e")], "empty contract body"),
        ("t", "b", [], "declares no executions"),
        ("t", "b", [Execution("  ")], "blank case id"),
        ("t", "b", [Execution("e"), Execution("e")], "more than once"),
    ],
)
def test_invalid_tests_are_rejected_at_add_time(test_id, body, executions, message):
    builder = EvidenceBuilder()

    with pytest.raises(InvalidEvidenceError, match=message):
        builder.add_test(test_id, body, executions)

    builder.add_test("clean", "b", [Execution("e")])
    assert [test["id"] for test in json.loads(builder.finish())["tests"]] == ["clean"]


def test_a_duplicate_test_id_across_calls_is_rejected():
    builder = EvidenceBuilder()
    builder.add_test("t", "b", [Execution("e")])

    with pytest.raises(InvalidEvidenceError, match="more than once"):
        builder.add_test("t", "other body", [Execution("other")])

    assert len(json.loads(builder.finish())["tests"]) == 1


def test_invalid_evidence_error_is_a_value_error():
    assert issubclass(InvalidEvidenceError, ValueError)


def test_an_unknown_outcome_is_rejected_by_name():
    with pytest.raises(ValueError, match="unknown outcome `passed`"):
        Execution("e", "passed")


def test_execution_exposes_its_case_and_outcome():
    assert (Execution("e", "pass").case, Execution("e", "pass").outcome) == ("e", "pass")
    assert Execution("e").outcome is None
