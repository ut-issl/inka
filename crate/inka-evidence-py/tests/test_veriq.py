"""Behavioral tests for the veriq adapter (`inka_evidence.veriq`).

They run against the fixture project under ``veriq_fixture/`` and skip
when veriq is not installed (it is an optional extra requiring Python 3.14).
"""

import json
from pathlib import Path

import jsonschema
import pytest

vq = pytest.importorskip("veriq")

from inka_evidence import veriq as adapter  # noqa: E402

FIXTURE = Path(__file__).parent / "veriq_fixture"
SCHEMA = json.loads(
    (Path(__file__).parents[3] / "schemas" / "calc-evidence-v1.schema.json").read_text()
)


@pytest.fixture(scope="module")
def evidence() -> dict:
    from veriq._cli.discover import load_project_from_script

    project = load_project_from_script(FIXTURE / "power.py")
    model_data = vq.load_model_data_from_toml(project, FIXTURE / "power.in.toml")
    builder = adapter.collect(project, model_data)
    return json.loads(builder.finish())


def target(evidence: dict, path: str) -> dict:
    (found,) = [t for t in evidence["targets"] if t["path"] == path]
    return found


def test_emitted_file_conforms_to_the_json_schema(evidence):
    jsonschema.validate(evidence, SCHEMA)


def test_every_verification_becomes_one_target_in_path_order(evidence):
    assert [t["path"] for t in evidence["targets"]] == [
        "Power::?verify_assuming",
        "Power::?verify_margin",
        "Power::?verify_reserve",
    ]


def test_the_upstream_subgraph_is_definition_level_with_the_diamond_collapsed(evidence):
    margin = target(evidence, "Power::?verify_margin")

    # One entry per definition, in veriq's deterministic path order; the
    # shared @budget appears once although it is read directly and via @reserve.
    assert [(d["path"], d["kind"]) for d in margin["definitions"]] == [
        ("Power::$", "model"),
        ("Power::?verify_margin", "verification"),
        ("Power::@budget", "calculation"),
        ("Power::@reserve", "calculation"),
    ]


def test_sources_are_raw_and_include_nested_classes(evidence):
    margin = target(evidence, "Power::?verify_margin")
    by_path = {d["path"]: d["source"] for d in margin["definitions"]}

    model = by_path["Power::$"]
    assert model.startswith("@power.root_model(")
    assert "class Config(BaseModel):" in model
    assert "class Battery(BaseModel):" in model, "a nested model class is part of the root model's source"
    assert "class Phase(StrEnum):" in model, "a table key enum is part of the root model's source"
    assert "class Budget" not in model, "an output model belongs to the calculation that returns it"

    budget = by_path["Power::@budget"]
    assert budget.startswith("@power.calculation(")
    assert "class Budget(BaseModel):" in budget
    assert "Phase" in budget

    assert by_path["Power::?verify_margin"].startswith("@power.verification()")


def test_assumptions_are_unioned_over_the_subgraph_and_sorted(evidence):
    # Declared on the root model, on @budget, and (twice, once repeated) on @reserve.
    assert target(evidence, "Power::?verify_margin")["assumes"] == [
        "power::battery_model_valid",
        "power::eclipse_model_valid",
        "power::loads_complete",
    ]
    # ?verify_reserve does not read @reserve's assumptions twice either.
    assert target(evidence, "Power::?verify_reserve")["assumes"] == [
        "power::battery_model_valid",
        "power::eclipse_model_valid",
        "power::loads_complete",
    ]


def test_table_valued_verifications_emit_one_leaf_per_cell(evidence):
    margin = target(evidence, "Power::?verify_margin")

    assert margin["leaves"] == [
        {"id": "Power::?verify_margin[daylight]", "result": True},
        {"id": "Power::?verify_margin[eclipse]", "result": False},
    ]
    assert margin["valid"] is True
    assert "xfail" not in margin


def test_an_expected_failure_is_marked_xfail(evidence):
    reserve = target(evidence, "Power::?verify_reserve")

    assert reserve["leaves"] == [{"id": "Power::?verify_reserve", "result": False}]
    assert reserve["xfail"] is True


def test_a_failed_assumed_verification_makes_the_target_invalid(evidence):
    assuming = target(evidence, "Power::?verify_assuming")

    # veriq overrides the value of an invalid verification to False itself;
    # the adapter reports both the overridden result and the validity.
    assert assuming["leaves"] == [{"id": "Power::?verify_assuming", "result": False}]
    assert assuming["valid"] is False
    # The assumed verification is part of the upstream subgraph.
    assert "Power::?verify_reserve" in [d["path"] for d in assuming["definitions"]]


def test_targets_can_be_restricted_and_unknown_ones_are_rejected():
    from veriq._cli.discover import load_project_from_script

    project = load_project_from_script(FIXTURE / "power.py")
    model_data = vq.load_model_data_from_toml(project, FIXTURE / "power.in.toml")

    only = json.loads(adapter.collect(project, model_data, targets=["Power::?verify_reserve"]).finish())
    assert [t["path"] for t in only["targets"]] == ["Power::?verify_reserve"]

    with pytest.raises(adapter.AdapterError, match="not a verification"):
        adapter.collect(project, model_data, targets=["Power::?missing"])
    with pytest.raises(adapter.AdapterError, match="not a verification"):
        adapter.collect(project, model_data, targets=["Power::@budget"])


def test_malformed_inka_metadata_is_reported_with_the_node():
    bad_project = vq.Project("Bad")
    scope = vq.Scope("S")
    bad_project.add_scope(scope)

    from pydantic import BaseModel

    @scope.root_model()
    class M(BaseModel):
        x: float

    from typing import Annotated

    @scope.verification(metadata={"inka": {"assumes": "s::not_a_list"}})
    def verify_x(x: Annotated[float, vq.Ref("$.x")]) -> bool:
        return x > 0

    model_data = {"S": M(x=1.0)}

    with pytest.raises(adapter.AdapterError, match="S::\\?verify_x"):
        adapter.collect(bad_project, model_data)


def test_the_cli_writes_schema_valid_evidence_with_provenance(tmp_path, monkeypatch):
    monkeypatch.setenv("CAPSULA_RUN_ID", "01K8WSYC91YAE21R7CWHQ4KYN2")
    output = tmp_path / "calc-evidence.json"

    adapter.main(
        [
            str(FIXTURE / "power.py"),
            "--input",
            str(FIXTURE / "power.in.toml"),
            "--output",
            str(output),
            "--provenance",
            "source_revision=4f2a1c9",
            "--target",
            "Power::?verify_margin",
        ]
    )

    written = json.loads(output.read_text())
    jsonschema.validate(written, SCHEMA)
    assert written["provenance"] == {
        "source_revision": "4f2a1c9",
        "capsula_run_id": "01K8WSYC91YAE21R7CWHQ4KYN2",
    }
    assert [t["path"] for t in written["targets"]] == ["Power::?verify_margin"]


def test_the_cli_requires_an_input(tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)  # no pyproject.toml with [tool.veriq] in reach

    with pytest.raises(SystemExit, match="input TOML is required"):
        adapter.main([str(FIXTURE / "power.py"), "--output", "-"])
