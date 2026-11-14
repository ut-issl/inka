"""Behavioral tests for the calc evidence part of the `inka_evidence` binding.

The binding wraps the Rust `inka-evidence` crate's `calc` module, so these
tests pin the contract the veriq adapter relies on — the v1 envelope, the
add-time rejections and the argument conventions — not the validation logic
itself, which the Rust crate proves against the JSON schema and the parser.
"""

import json
from pathlib import Path

import jsonschema
import pytest

from inka_evidence import CalcEvidenceBuilder, Definition, InvalidEvidenceError, Leaf

SCHEMA = json.loads(
    (Path(__file__).parents[3] / "schemas" / "calc-evidence-v1.schema.json").read_text()
)

TARGET = "Power::?verify_margin"


def definitions() -> list[Definition]:
    return [
        Definition("Power::$Config", "model", "class Config(BaseModel): ..."),
        Definition("Power::@budget", "calculation", "def budget(config): ..."),
        Definition(TARGET, "verification", "def verify_margin(budget): ..."),
    ]


def build_representative() -> CalcEvidenceBuilder:
    builder = CalcEvidenceBuilder()
    builder.provenance("source_revision", "0123abc")
    builder.add_target(
        TARGET,
        definitions(),
        assumes=["power::eclipse_model_valid"],
        leaves=[Leaf(f"{TARGET}.daylight", True), Leaf(f"{TARGET}.eclipse")],
        valid=True,
    )
    builder.add_target(
        "Power::?verify_xfail",
        [Definition("Power::?verify_xfail", "verification", "def verify_xfail(): ...")],
        assumes=[],
        leaves=[Leaf("Power::?verify_xfail", False)],
        valid=False,
        xfail=True,
    )
    return builder


def test_finish_stamps_v1_and_ends_with_one_newline():
    text = build_representative().finish()

    assert json.loads(text)["v"] == 1
    assert text.endswith("\n") and not text.endswith("\n\n")


def test_emitted_file_conforms_to_the_json_schema():
    text = build_representative().finish()

    jsonschema.validate(json.loads(text), SCHEMA)


def test_emitted_targets_carry_what_was_added():
    targets = json.loads(build_representative().finish())["targets"]

    assert [t["path"] for t in targets] == [TARGET, "Power::?verify_xfail"]
    assert targets[0]["definitions"][1] == {
        "path": "Power::@budget",
        "kind": "calculation",
        "source": "def budget(config): ...",
    }
    assert targets[0]["assumes"] == ["power::eclipse_model_valid"]
    assert targets[0]["valid"] is True
    assert targets[1]["valid"] is False and targets[1]["xfail"] is True


def test_defaults_are_absent_keys():
    target = json.loads(build_representative().finish())["targets"][0]

    assert target["leaves"][1] == {"id": f"{TARGET}.eclipse"}
    assert "xfail" not in target


def test_definition_and_leaf_expose_their_fields():
    definition = Definition("P::@c", "calculation", "src")
    leaf = Leaf("P::?v", False)

    assert (definition.path, definition.kind, definition.source) == ("P::@c", "calculation", "src")
    assert (leaf.id, leaf.result) == ("P::?v", False)
    assert Leaf("P::?v").result is None
    assert repr(leaf) == 'Leaf("P::?v", False)'
    assert repr(definition) == 'Definition("P::@c", "calculation", "src")'


def test_an_unknown_definition_kind_is_rejected():
    with pytest.raises(InvalidEvidenceError, match="definition kind"):
        Definition("P::@c", "function", "src")


@pytest.mark.parametrize(
    ("path", "defs", "assumes", "leaves", "message"),
    [
        ("P:: ?v", None, [], None, "blank or contains whitespace"),
        (TARGET, [], [], None, "declares no definitions"),
        (TARGET, [Definition("Power::@budget", "calculation", "s")], [], None, "not among its own"),
        (TARGET, [Definition(TARGET, "verification", "")], [], None, "empty source"),
        (TARGET, None, ["power.eclipse"], None, "not an inka node id"),
        (TARGET, None, ["a", "a"], None, "more than once"),
        (TARGET, None, [], [], "declares no leaves"),
        (TARGET, None, [], [Leaf(TARGET), Leaf(TARGET)], "more than once"),
    ],
)
def test_invalid_targets_are_rejected_at_add_time(path, defs, assumes, leaves, message):
    builder = CalcEvidenceBuilder()

    with pytest.raises(InvalidEvidenceError, match=message):
        builder.add_target(
            path,
            definitions() if defs is None else defs,
            assumes=assumes,
            leaves=[Leaf(TARGET, True)] if leaves is None else leaves,
            valid=True,
        )
    assert json.loads(builder.finish())["targets"] == []


def test_a_duplicate_target_path_is_rejected():
    builder = build_representative()
    before = builder.finish()

    with pytest.raises(InvalidEvidenceError, match="more than once"):
        builder.add_target(TARGET, definitions(), assumes=[], leaves=[Leaf(TARGET)], valid=True)

    assert builder.finish() == before


def test_blank_provenance_is_rejected():
    builder = CalcEvidenceBuilder()

    with pytest.raises(InvalidEvidenceError):
        builder.provenance("source_revision", " ")

    assert "provenance" not in json.loads(builder.finish())


def test_capsula_run_id_fills_only_when_absent(monkeypatch):
    monkeypatch.setenv("CAPSULA_RUN_ID", "from-env")
    filled = CalcEvidenceBuilder()
    filled.capsula_env()
    preset = CalcEvidenceBuilder()
    preset.provenance("capsula_run_id", "explicit")
    preset.capsula_env()

    assert json.loads(filled.finish())["provenance"] == {"capsula_run_id": "from-env"}
    assert json.loads(preset.finish())["provenance"] == {"capsula_run_id": "explicit"}
