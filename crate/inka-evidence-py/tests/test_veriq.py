"""Behavioral tests for the veriq adapter (`inka_evidence.veriq`).

They run against the fixture projects under ``veriq_fixture/`` and skip
when veriq is not installed (it is an optional extra requiring Python 3.14).
"""

import importlib
import json
import os
import subprocess
import sys
from pathlib import Path
from typing import Annotated

import jsonschema
import pytest

vq = pytest.importorskip("veriq")

from pydantic import BaseModel  # noqa: E402

from inka_evidence import veriq as adapter  # noqa: E402

FIXTURE = Path(__file__).parent / "veriq_fixture"
SCHEMA = json.loads(
    (Path(__file__).parents[3] / "schemas" / "calc-evidence-v1.schema.json").read_text()
)

#: A one-scope project script without any verification; the CLI tests
#: append what they need. Its input is :data:`PROBE_INPUT`.
PROBE_SCRIPT = """\
from typing import Annotated

from pydantic import BaseModel

import veriq as vq

project = vq.Project("Probe")
scope = vq.Scope("S")
project.add_scope(scope)


@scope.root_model()
class M(BaseModel):
    x: float
"""
PROBE_INPUT = "[S.model]\nx = 1.0\n"


def collect_script(script: Path, input_path: Path) -> dict:
    project = vq.load_project(script)
    model_data = vq.load_model_data_from_toml(project, input_path)
    return json.loads(adapter.collect(project, model_data).finish())


@pytest.fixture(scope="module")
def evidence() -> dict:
    return collect_script(FIXTURE / "power.py", FIXTURE / "power.in.toml")


@pytest.fixture(scope="module")
def class_graph() -> dict:
    return collect_script(FIXTURE / "class_graph.py", FIXTURE / "class_graph.in.toml")


@pytest.fixture
def write_script(tmp_path):
    """Writes a throwaway project module, forgotten again after the test."""
    stems: list[str] = []

    def write(stem: str, source: str) -> Path:
        script = tmp_path / f"{stem}.py"
        script.write_text(source)
        stems.append(stem)
        importlib.invalidate_caches()
        return script

    yield write
    for stem in stems:
        sys.modules.pop(stem, None)


def probe_project(*, returns: object = True, metadata: dict | None = None):
    """A one-scope project whose only verification returns ``returns`` and
    carries ``metadata``, with the model data to evaluate it."""
    project = vq.Project("Probe")
    scope = vq.Scope("S")
    project.add_scope(scope)

    @scope.root_model()
    class M(BaseModel):
        x: float

    @scope.verification(metadata=metadata)
    def verify_x(x: Annotated[float, vq.Ref("$.x")]) -> bool:
        return returns  # veriq does not hold a verification to its annotation

    return project, {"S": M(x=1.0)}


def target(evidence: dict, path: str) -> dict:
    (found,) = [t for t in evidence["targets"] if t["path"] == path]
    return found


def sources(evidence: dict, path: str) -> dict[str, str]:
    return {d["path"]: d["source"] for d in target(evidence, path)["definitions"]}


def class_headers(source: str) -> list[str]:
    return [line for line in source.splitlines() if line.startswith("class ")]


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


def test_base_classes_are_part_of_a_definition_source(class_graph):
    by_path = sources(class_graph, "Bus::?verify_capacity")

    model = by_path["Bus::$"]
    assert "class Margins(BaseModel):" in model, "a pydantic base carries inherited fields and defaults"
    assert "class Derating:" in model, "a plain mixin base is project code too"
    assert "class Report[U](BaseModel):" in by_path["Bus::@capacity"], "so is the base of an output model"


def test_editing_only_a_base_class_changes_the_definition_that_inherits_it(write_script):
    original = (FIXTURE / "class_graph.py").read_text()
    edited = original.replace("gain: float = 2.0", "gain: float = 3.0")
    assert edited != original
    scripts = [write_script("class_graph_original", original), write_script("class_graph_edited", edited)]

    before, after = (
        sources(collect_script(script, FIXTURE / "class_graph.in.toml"), "Bus::?verify_capacity")
        for script in scripts
    )

    assert before["Bus::$"] != after["Bus::$"]
    assert before["Bus::$"].replace("gain: float = 2.0", "gain: float = 3.0") == after["Bus::$"]
    assert before["Bus::@capacity"] == after["Bus::@capacity"], "a definition that does not inherit it is untouched"


def test_a_pep_695_alias_is_followed_to_the_classes_it_names(class_graph):
    model = sources(class_graph, "Bus::?verify_capacity")["Bus::$"]

    assert "class Cell(BaseModel):" in model, "`cells: Cells` with `type Cells = list[Cell]`"
    assert "class Chemistry(vq.StrEnumWithDoc):" in model, "and on through the fields of what it names"


def test_a_typing_extensions_alias_is_followed_like_a_pep_695_one():
    from typing_extensions import TypeAliasType  # a pydantic dependency

    class Cell(BaseModel):
        capacity_wh: float

    Cells = TypeAliasType("Cells", list[Cell])

    def cells() -> Cells:
        return []

    source = adapter.function_source(cells, "calculation S::@cells")

    assert "class Cell(BaseModel):" in source


def test_a_parametrized_generic_base_contributes_its_origin_and_arguments(class_graph):
    capacity = sources(class_graph, "Bus::?verify_capacity")["Bus::@capacity"]

    assert "class Report[U](BaseModel):" in capacity
    assert "class Unit(BaseModel):" in capacity


def test_library_classes_stay_out_of_the_sources(class_graph):
    emitted = "\n".join(sources(class_graph, "Bus::?verify_capacity").values())

    # A veriq or pydantic upgrade must not invalidate every approval.
    assert "class FileRef" not in emitted, "a `vq.FileRef` field"
    assert "class ExternalData" not in emitted, "its veriq base"
    assert "class StrEnumWithDoc" not in emitted, "the veriq base of a user enum"
    assert "class BaseModel" not in emitted
    assert "class StrEnum" not in emitted


def test_a_with_range_enum_is_emitted_as_the_members_veriq_generated():
    from enum import IntEnum

    # `@vq.with_range` rebuilds the class under veriq's own module and without
    # source text, yet how many members it has is the project's decision.
    @vq.with_range(3, prefix="S")
    class Slot(IntEnum):
        SPARE = 9

    def loads() -> vq.Table[Slot, float]:
        raise NotImplementedError

    source = adapter.function_source(loads, "calculation S::@loads")

    assert source.endswith(
        "class Slot(IntEnum):  # members generated by veriq.with_range\n"
        "    S0 = 0\n"
        "    S1 = 1\n"
        "    S2 = 2\n"
        "    SPARE = 9\n"
    )


def test_with_range_enums_of_one_name_are_told_apart_by_their_members():
    from enum import IntEnum

    def slot(count: int) -> type[IntEnum]:
        @vq.with_range(count)
        class Slot(IntEnum):
            pass

        return Slot

    Wide, Narrow = slot(2), slot(1)

    def loads() -> tuple[Wide, Narrow]:
        raise NotImplementedError

    source = adapter.function_source(loads, "calculation S::@loads")

    # Both carry veriq's module and the name `Slot`; neither may shadow the other.
    assert source.count("class Slot(IntEnum):") == 2
    assert source.endswith(
        "class Slot(IntEnum):  # members generated by veriq.with_range\n"
        "    _0 = 0\n"
        "\n"
        "class Slot(IntEnum):  # members generated by veriq.with_range\n"
        "    _0 = 0\n"
        "    _1 = 1\n"
    )


def test_each_class_is_emitted_once_after_its_definition_in_name_order(class_graph):
    by_path = sources(class_graph, "Bus::?verify_capacity")

    assert class_headers(by_path["Bus::$"]) == [
        "class Config(Margins, Derating):",
        "class Cell(BaseModel):",
        "class Chemistry(vq.StrEnumWithDoc):",
        "class Derating:",
        "class Margins(BaseModel):",
    ]
    assert class_headers(by_path["Bus::@capacity"]) == [
        "class Capacity(Report[Unit]):",
        "class Report[U](BaseModel):",
        "class Unit(BaseModel):",
    ]


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

    # The result is what the verification returned (veriq's own value is
    # overridden to False); the failed assumption shows as `valid: false`.
    assert assuming["leaves"] == [{"id": "Power::?verify_assuming", "result": True}]
    assert assuming["valid"] is False
    # The assumed verification is part of the upstream subgraph.
    assert "Power::?verify_reserve" in [d["path"] for d in assuming["definitions"]]


def test_targets_can_be_restricted_and_unknown_ones_are_rejected():
    project = vq.load_project(FIXTURE / "power.py")
    model_data = vq.load_model_data_from_toml(project, FIXTURE / "power.in.toml")

    only = json.loads(adapter.collect(project, model_data, targets=["Power::?verify_reserve"]).finish())
    assert [t["path"] for t in only["targets"]] == ["Power::?verify_reserve"]

    with pytest.raises(adapter.AdapterError, match="not a verification"):
        adapter.collect(project, model_data, targets=["Power::?missing"])
    with pytest.raises(adapter.AdapterError, match="not a verification"):
        adapter.collect(project, model_data, targets=["Power::@budget"])


@pytest.mark.parametrize(
    "assumes",
    [None, 3, {"s::a": True}, {"s::a"}, "s::not_a_list", ["s::a", 3]],
    ids=["none", "int", "dict", "set", "bare-str", "non-str-entry"],
)
def test_malformed_assumes_are_reported_with_the_node(assumes):
    project, model_data = probe_project(metadata={"inka": {"assumes": assumes}})

    with pytest.raises(adapter.AdapterError, match=r"'assumes' of S::\?verify_x must be a list"):
        adapter.collect(project, model_data)


def test_assumes_may_be_a_tuple():
    project, model_data = probe_project(metadata={"inka": {"assumes": ("s::b", "s::a")}})

    emitted = json.loads(adapter.collect(project, model_data).finish())

    assert target(emitted, "S::?verify_x")["assumes"] == ["s::a", "s::b"]


def test_an_unknown_inka_metadata_key_is_reported_with_the_node():
    # The typo would otherwise drop the dependency edge without a word.
    project, model_data = probe_project(metadata={"inka": {"assume": ["s::a"]}})

    with pytest.raises(adapter.AdapterError, match=r"S::\?verify_x has unknown key 'assume'"):
        adapter.collect(project, model_data)


def test_inka_metadata_that_is_not_a_mapping_is_reported_with_the_node():
    project, model_data = probe_project(metadata={"inka": ["s::a"]})

    with pytest.raises(adapter.AdapterError, match=r"S::\?verify_x must be a mapping"):
        adapter.collect(project, model_data)


@pytest.mark.parametrize("returned", [6.0, 1, "yes"], ids=["float", "int", "str"])
def test_a_result_that_is_not_a_bool_is_rejected_with_the_leaf_and_its_type(returned):
    project, model_data = probe_project(returns=returned)

    with pytest.raises(adapter.AdapterError, match=rf"S::\?verify_x .*\b{type(returned).__name__}\b"):
        adapter.collect(project, model_data)


def test_a_verification_that_returned_nothing_emits_a_leaf_without_a_result():
    project, model_data = probe_project(returns=None)

    emitted = json.loads(adapter.collect(project, model_data).finish())

    assert target(emitted, "S::?verify_x")["leaves"] == [{"id": "S::?verify_x"}]


def test_a_project_without_verifications_is_rejected():
    project = vq.Project("Empty")
    scope = vq.Scope("S")
    project.add_scope(scope)

    @scope.root_model()
    class M(BaseModel):
        x: float

    with pytest.raises(adapter.AdapterError, match="no verification"):
        adapter.collect(project, {"S": M(x=1.0)})


@pytest.mark.parametrize("name", ["power", "class_graph"])
def test_two_runs_over_the_same_project_are_byte_identical(name, tmp_path):
    def run(hash_seed: str) -> bytes:
        output = tmp_path / f"seed-{hash_seed}.json"
        environment = {key: value for key, value in os.environ.items() if key != "CAPSULA_RUN_ID"}
        subprocess.run(
            [
                sys.executable,
                "-m",
                "inka_evidence.veriq",
                str(FIXTURE / f"{name}.py"),
                "--input",
                str(FIXTURE / f"{name}.in.toml"),
                "--output",
                str(output),
                "--provenance",
                "source_revision=4f2a1c9",
            ],
            check=True,
            cwd=tmp_path,
            # Separate interpreters with different hash seeds: nothing may
            # depend on set or dict iteration order either.
            env={**environment, "PYTHONHASHSEED": hash_seed},
        )
        return output.read_bytes()

    first = run("1")
    second = run("2")

    assert first == second


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


def test_the_cli_requires_an_input_when_none_is_configured(tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)  # no pyproject.toml with [tool.veriq] in reach

    with pytest.raises(SystemExit, match="input TOML is required"):
        adapter.main([str(FIXTURE / "power.py"), "--output", "-"])


def test_the_cli_falls_back_to_the_veriq_configuration(tmp_path, monkeypatch):
    (tmp_path / "pyproject.toml").write_text(
        "[tool.veriq]\n"
        f'project = {{ script = "{(FIXTURE / "power.py").as_posix()}" }}\n'
        f'input = "{(FIXTURE / "power.in.toml").as_posix()}"\n'
    )
    monkeypatch.chdir(tmp_path)
    output = tmp_path / "out.json"

    adapter.main(["--output", str(output), "--target", "Power::?verify_reserve"])

    written = json.loads(output.read_text())
    jsonschema.validate(written, SCHEMA)
    assert [t["path"] for t in written["targets"]] == ["Power::?verify_reserve"]


def test_the_cli_reports_an_unloadable_project(tmp_path):
    with pytest.raises(SystemExit, match="cannot load the project"):
        adapter.main([str(tmp_path / "missing.py"), "--input", str(FIXTURE / "power.in.toml")])


def test_the_cli_reports_a_non_bool_result_and_writes_nothing(tmp_path, write_script):
    script = write_script(
        "probe_non_bool",
        PROBE_SCRIPT
        + """

@scope.verification()
def verify_x(x: Annotated[float, vq.Ref("$.x")]) -> bool:
    return 6.0
""",
    )
    (tmp_path / "probe.in.toml").write_text(PROBE_INPUT)
    output = tmp_path / "out.json"

    with pytest.raises(SystemExit, match=r"^error: .*S::\?verify_x .*\bfloat\b"):
        adapter.main([str(script), "--input", str(tmp_path / "probe.in.toml"), "--output", str(output)])

    assert not output.exists()


def test_the_cli_rejects_a_project_without_verifications_and_writes_nothing(tmp_path, write_script, capsys):
    script = write_script("probe_no_verification", PROBE_SCRIPT)
    (tmp_path / "probe.in.toml").write_text(PROBE_INPUT)
    output = tmp_path / "out.json"

    with pytest.raises(SystemExit, match="^error: .*no verification"):
        adapter.main([str(script), "--input", str(tmp_path / "probe.in.toml"), "--output", str(output)])
    with pytest.raises(SystemExit, match="^error: .*no verification"):
        adapter.main([str(script), "--input", str(tmp_path / "probe.in.toml")])

    assert not output.exists()
    assert capsys.readouterr().out == ""
