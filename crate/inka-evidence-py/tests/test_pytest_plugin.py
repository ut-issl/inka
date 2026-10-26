"""Behavioral tests for the pytest evidence plugin.

Each test drives a real nested pytest run through the ``pytester``
fixture and inspects the emitted evidence file, pinning the contract
adapters rely on: identity, bodies, outcome mapping and the explicit
failures for unsupported shapes.
"""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any

import jsonschema
import pytest

pytest_plugins = ("pytester",)

SCHEMA = json.loads(
    (
        Path(__file__).resolve().parents[3] / "schemas" / "test-evidence-v1.schema.json"
    ).read_text(encoding="utf-8")
)

# The entry point loads the plugin; naming it keeps the runs working
# even where plugin autoloading is disabled.
PLUGIN_ARGS = ("-p", "inka_evidence")


@pytest.fixture(autouse=True)
def _scrub_capsula_env(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.delenv("CAPSULA_RUN_ID", raising=False)


def read_evidence(path: Path) -> dict[str, Any]:
    data = json.loads(path.read_text(encoding="utf-8"))
    jsonschema.validate(data, SCHEMA)
    return data


def by_test_id(data: dict[str, Any]) -> dict[str, dict[str, Any]]:
    return {test["id"]: test for test in data["tests"]}


def outcome_by_case(test: dict[str, Any]) -> dict[str, str | None]:
    return {
        execution["case"]: execution.get("outcome") for execution in test["executions"]
    }


def test_a_full_run_maps_identities_bodies_and_outcomes(
    pytester: pytest.Pytester,
) -> None:
    pytester.makepyfile(
        test_sample=(
            "import pytest\n"
            "\n"
            "\n"
            '@pytest.mark.parametrize("value", [1, 2], ids=["one", "two"])\n'
            "def test_param(value):\n"
            "    assert value < 2\n"
            "\n"
            "\n"
            "def test_plain():\n"
            "    assert True\n"
            "\n"
            "\n"
            "def test_fails():\n"
            "    assert False\n"
            "\n"
            "\n"
            '@pytest.mark.skip(reason="later")\n'
            "def test_skipped():\n"
            "    assert True\n"
            "\n"
            "\n"
            '@pytest.mark.xfail(reason="known")\n'
            "def test_xfail():\n"
            "    assert False\n"
            "\n"
            "\n"
            '@pytest.mark.xfail(reason="lucky")\n'
            "def test_xpass():\n"
            "    assert True\n"
            "\n"
            "\n"
            '@pytest.mark.xfail(reason="strict", strict=True)\n'
            "def test_strict_xpass():\n"
            "    assert True\n"
            "\n"
            "\n"
            "@pytest.fixture\n"
            "def broken_setup():\n"
            '    raise RuntimeError("boom")\n'
            "\n"
            "\n"
            "def test_setup_error(broken_setup):\n"
            "    assert True\n"
            "\n"
            "\n"
            "@pytest.fixture\n"
            "def broken_teardown():\n"
            "    yield\n"
            '    raise RuntimeError("late boom")\n'
            "\n"
            "\n"
            "def test_teardown_error(broken_teardown):\n"
            "    assert True\n"
            "\n"
            "\n"
            "class TestGroup:\n"
            "    def test_method(self):\n"
            "        assert True\n"
        )
    )
    evidence = pytester.path / "evidence.json"

    result = pytester.runpytest(
        *PLUGIN_ARGS,
        "--inka-evidence",
        str(evidence),
        "--inka-evidence-provenance",
        "source_revision=abc123",
    )

    assert result.ret == pytest.ExitCode.TESTS_FAILED
    result.stdout.fnmatch_lines(["*inka-evidence: wrote*evidence.json*"])
    data = read_evidence(evidence)
    assert data["v"] == 1
    assert data["provenance"] == {"source_revision": "abc123"}

    tests = by_test_id(data)
    assert outcome_by_case(tests["test_sample.py::test_param"]) == {
        "one": "pass",
        "two": "fail",
    }
    singles = {
        "test_sample.py::test_plain": "pass",
        "test_sample.py::test_fails": "fail",
        "test_sample.py::test_skipped": "skip",
        "test_sample.py::test_xfail": "skip",
        "test_sample.py::test_xpass": "pass",
        "test_sample.py::test_strict_xpass": "fail",
        "test_sample.py::test_setup_error": "fail",
        "test_sample.py::test_teardown_error": "fail",
        "test_sample.py::TestGroup::test_method": "pass",
    }
    for test_id, expected in singles.items():
        assert outcome_by_case(tests[test_id]) == {"default": expected}, test_id
    assert len(tests) == len(singles) + 1

    assert tests["test_sample.py::test_param"]["body"] == (
        '@pytest.mark.parametrize("value", [1, 2], ids=["one", "two"])\n'
        "def test_param(value):\n"
        "    assert value < 2\n"
    )
    assert tests["test_sample.py::TestGroup::test_method"]["body"] == (
        "    def test_method(self):\n        assert True\n"
    )


def test_capsula_run_id_is_picked_up_from_the_environment(
    pytester: pytest.Pytester, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv("CAPSULA_RUN_ID", "run-9")
    pytester.makepyfile(test_one="def test_one():\n    assert True\n")
    evidence = pytester.path / "evidence.json"

    pytester.runpytest(*PLUGIN_ARGS, "--inka-evidence", str(evidence)).assert_outcomes(
        passed=1
    )

    data = read_evidence(evidence)
    assert data["provenance"] == {"capsula_run_id": "run-9"}


def test_an_aborted_run_leaves_missing_results(pytester: pytest.Pytester) -> None:
    pytester.makepyfile(
        test_order=(
            "def test_first():\n"
            "    assert False\n"
            "\n"
            "\n"
            "def test_second():\n"
            "    assert True\n"
        )
    )
    evidence = pytester.path / "evidence.json"

    result = pytester.runpytest(*PLUGIN_ARGS, "-x", "--inka-evidence", str(evidence))

    assert result.ret == pytest.ExitCode.TESTS_FAILED
    tests = by_test_id(read_evidence(evidence))
    assert outcome_by_case(tests["test_order.py::test_first"]) == {"default": "fail"}
    # Declared by the selection, never run: the outcome key is absent.
    assert tests["test_order.py::test_second"]["executions"] == [{"case": "default"}]


def test_deselection_narrows_the_declared_set(pytester: pytest.Pytester) -> None:
    pytester.makepyfile(
        test_pick=(
            "def test_kept():\n"
            "    assert True\n"
            "\n"
            "\n"
            "def test_dropped():\n"
            "    assert True\n"
        )
    )
    evidence = pytester.path / "evidence.json"

    pytester.runpytest(
        *PLUGIN_ARGS, "-k", "kept", "--inka-evidence", str(evidence)
    ).assert_outcomes(passed=1)

    tests = by_test_id(read_evidence(evidence))
    assert list(tests) == ["test_pick.py::test_kept"]


def test_non_function_items_are_an_explicit_error(pytester: pytest.Pytester) -> None:
    pytester.makepyfile(
        doc_module=(
            "def documented():\n"
            '    """Doubles.\n'
            "\n"
            "    >>> documented()\n"
            "    2\n"
            '    """\n'
            "    return 2\n"
        )
    )
    evidence = pytester.path / "evidence.json"

    result = pytester.runpytest(
        *PLUGIN_ARGS, "--doctest-modules", "--inka-evidence", str(evidence)
    )

    assert result.ret == pytest.ExitCode.USAGE_ERROR
    result.stderr.fnmatch_lines(["*is not a function-based test*"])
    assert not evidence.exists()


def test_collection_errors_suppress_emission(pytester: pytest.Pytester) -> None:
    pytester.makepyfile(
        test_ok="def test_ok():\n    assert True\n",
        test_broken="import does_not_exist_inka_xyz\n",
    )
    evidence = pytester.path / "evidence.json"

    result = pytester.runpytest(*PLUGIN_ARGS, "--inka-evidence", str(evidence))

    assert result.ret != 0
    result.stdout.fnmatch_lines(["*collection errors*no evidence was emitted*"])
    assert not evidence.exists()


def test_an_empty_selection_emits_nothing(pytester: pytest.Pytester) -> None:
    evidence = pytester.path / "evidence.json"

    result = pytester.runpytest(*PLUGIN_ARGS, "--inka-evidence", str(evidence))

    assert result.ret == pytest.ExitCode.NO_TESTS_COLLECTED
    result.stdout.fnmatch_lines(["*selected no tests, no evidence was emitted*"])
    assert not evidence.exists()


def test_the_plugin_is_inert_without_the_option(pytester: pytest.Pytester) -> None:
    pytester.makepyfile(test_one="def test_one():\n    assert True\n")

    pytester.runpytest(*PLUGIN_ARGS).assert_outcomes(passed=1)

    assert not (pytester.path / "evidence.json").exists()


def test_provenance_without_output_is_a_usage_error(
    pytester: pytest.Pytester,
) -> None:
    result = pytester.runpytest(*PLUGIN_ARGS, "--inka-evidence-provenance", "k=v")

    assert result.ret == pytest.ExitCode.USAGE_ERROR
    result.stderr.fnmatch_lines(["*requires --inka-evidence*"])


def test_a_malformed_provenance_entry_is_a_usage_error(
    pytester: pytest.Pytester,
) -> None:
    result = pytester.runpytest(
        *PLUGIN_ARGS,
        "--inka-evidence",
        "evidence.json",
        "--inka-evidence-provenance",
        "nokey",
    )

    assert result.ret == pytest.ExitCode.USAGE_ERROR
    result.stderr.fnmatch_lines(["*is not key=value*"])
