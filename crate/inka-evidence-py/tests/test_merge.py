"""Behavioral tests for the merge binding and its CLI module.

The merge semantics are proven in the shared Rust crate; these tests pin
the Python surface: argument translation, error mapping, the CLI's file
handling, and the naming (the ``merge`` function must stay importable
although a ``merge_cli`` submodule exists).
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

from inka_evidence import EvidenceBuilder, Execution, InvalidEvidenceError, merge


def evidence(case: str, outcome: str | None = "pass") -> str:
    builder = EvidenceBuilder()
    builder.provenance("source_revision", "abc123")
    builder.add_test("suite::test", "the body", [Execution(case, outcome)])
    return builder.finish()


def scrubbed_env() -> dict[str, str]:
    env = dict(os.environ)
    env.pop("CAPSULA_RUN_ID", None)
    return env


def test_merges_labeled_inputs_and_fills_the_run_id() -> None:
    merged = merge(
        [("linux", evidence("default")), ("macos", evidence("default", None))],
        [("runner", "matrix-ci")],
        "run-9",
    )

    data = json.loads(merged)
    assert data["provenance"] == {
        "source_revision": "abc123",
        "runner": "matrix-ci",
        "capsula_run_id": "run-9",
    }
    assert data["tests"][0]["executions"] == [
        {"case": "linux/default", "outcome": "pass"},
        {"case": "macos/default"},
    ]


def test_merge_errors_surface_as_invalid_evidence() -> None:
    with pytest.raises(InvalidEvidenceError, match="more than one input"):
        merge([(None, evidence("default")), (None, evidence("default"))])


def test_the_cli_module_merges_files(tmp_path: Path) -> None:
    linux = tmp_path / "linux.json"
    linux.write_text(evidence("default"), encoding="utf-8")
    macos = tmp_path / "macos.json"
    macos.write_text(evidence("default"), encoding="utf-8")
    out = tmp_path / "merged.json"

    result = subprocess.run(
        [
            sys.executable,
            "-m",
            "inka_evidence.merge_cli",
            "--output",
            str(out),
            f"linux={linux}",
            f"macos={macos}",
        ],
        capture_output=True,
        text=True,
        check=False,
        env=scrubbed_env(),
    )

    assert result.returncode == 0, result.stderr
    data = json.loads(out.read_text(encoding="utf-8"))
    cases = [execution["case"] for execution in data["tests"][0]["executions"]]
    assert cases == ["linux/default", "macos/default"]
    assert "capsula_run_id" not in data["provenance"]


def test_the_cli_module_reports_merge_errors(tmp_path: Path) -> None:
    one = tmp_path / "one.json"
    one.write_text(evidence("default"), encoding="utf-8")
    two = tmp_path / "two.json"
    two.write_text(evidence("default"), encoding="utf-8")

    result = subprocess.run(
        [sys.executable, "-m", "inka_evidence.merge_cli", str(one), str(two)],
        capture_output=True,
        text=True,
        check=False,
        env=scrubbed_env(),
    )

    assert result.returncode != 0
    assert "more than one input" in result.stderr


def test_the_cli_module_reports_unreadable_files(tmp_path: Path) -> None:
    result = subprocess.run(
        [sys.executable, "-m", "inka_evidence.merge_cli", "missing.json"],
        capture_output=True,
        text=True,
        check=False,
        cwd=tmp_path,
        env=scrubbed_env(),
    )

    assert result.returncode != 0
    assert "cannot read missing.json" in result.stderr
