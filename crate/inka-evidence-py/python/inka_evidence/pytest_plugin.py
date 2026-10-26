"""Emit inka test evidence v1 from a pytest run.

Inert unless ``--inka-evidence=PATH`` is passed. The required execution
set is declared from the finally selected items, never inferred from
results: a run that stops early (``-x``, an interrupt) leaves declared
executions with a missing outcome.

Identity: the test-id is the nodeid without its parametrization part
(``tests/test_x.py::TestGroup::test_m``); each parametrization is one
required execution whose case is the parameter id, or ``default`` when
the test takes no parameters. Changing the parameter set changes the
approval interface.

Bodies are the test function's own definition — decorators included —
via :func:`inspect.getsource`; fixture and helper bodies stay excluded,
as the contract assigns dependency capture elsewhere. Items that are not
function-based tests (doctests, ...) and pytest-xdist distribution are
explicit errors; with pytest-rerunfailures the final attempt wins.
"""

from __future__ import annotations

import inspect
import sys
from pathlib import Path

import pytest

from inka_evidence import EvidenceBuilder, Execution

DEFAULT_CASE = "default"

_OUTCOME_PASS = "pass"
_OUTCOME_FAIL = "fail"
_OUTCOME_SKIP = "skip"


def pytest_addoption(parser: pytest.Parser) -> None:
    group = parser.getgroup("inka-evidence")
    group.addoption(
        "--inka-evidence",
        metavar="PATH",
        default=None,
        help="write inka test evidence v1 JSON for this run to PATH",
    )
    group.addoption(
        "--inka-evidence-provenance",
        metavar="KEY=VALUE",
        action="append",
        default=[],
        help="extra provenance entry for the evidence file (repeatable)",
    )


def pytest_configure(config: pytest.Config) -> None:
    output = config.getoption("--inka-evidence")
    raw_provenance: list[str] = config.getoption("--inka-evidence-provenance")
    if output is None:
        if raw_provenance:
            raise pytest.UsageError(
                "inka-evidence: --inka-evidence-provenance requires --inka-evidence"
            )
        return
    if _xdist_distribution_active(config):
        raise pytest.UsageError(
            "inka-evidence: pytest-xdist distribution is not supported; "
            "run without -n/--dist or drop --inka-evidence"
        )
    provenance = [_split_provenance(entry) for entry in raw_provenance]
    config.pluginmanager.register(
        _EvidenceCollector(Path(output), provenance), "inka-evidence-collector"
    )


def _split_provenance(entry: str) -> tuple[str, str]:
    key, separator, value = entry.partition("=")
    if not separator:
        raise pytest.UsageError(
            f"inka-evidence: provenance entry {entry!r} is not key=value"
        )
    return key, value


def _xdist_distribution_active(config: pytest.Config) -> bool:
    if not config.pluginmanager.hasplugin("xdist"):
        return False
    processes = config.getoption("numprocesses", default=None)
    distribution = config.getoption("dist", default="no")
    return bool(processes) or distribution != "no"


def _identify(item: pytest.Item) -> tuple[str, str]:
    """The (test-id, case) of one selected item."""
    if not isinstance(item, pytest.Function):
        raise pytest.UsageError(
            f"inka-evidence: {item.nodeid} is not a function-based test "
            f"({type(item).__name__}); doctests and other item kinds are "
            "not supported"
        )
    callspec = getattr(item, "callspec", None)
    if callspec is None:
        return item.nodeid, DEFAULT_CASE
    suffix = f"[{callspec.id}]"
    if not item.nodeid.endswith(suffix):
        raise pytest.UsageError(
            f"inka-evidence: cannot split the parameter id {callspec.id!r} "
            f"out of {item.nodeid}"
        )
    return item.nodeid[: -len(suffix)], callspec.id


def _body(item: pytest.Function) -> str:
    """The test function's own definition, decorators included, verbatim."""
    try:
        return inspect.getsource(item.function)
    except (OSError, TypeError) as error:
        raise pytest.UsageError(
            f"inka-evidence: cannot extract the source of {item.nodeid}: {error}"
        ) from error


class _DeclaredTest:
    """One test-id with its body and its declared executions in order."""

    __slots__ = ("body", "cases")

    def __init__(self, body: str) -> None:
        self.body = body
        self.cases: list[tuple[str, str]] = []  # (case, nodeid)


class _EvidenceCollector:
    """Session state: the declared set, the outcomes, and the emission."""

    def __init__(self, output: Path, provenance: list[tuple[str, str]]) -> None:
        self._output = output
        self._provenance = provenance
        self._tests: dict[str, _DeclaredTest] = {}
        self._outcomes: dict[str, str] = {}
        self._broken: str | None = None

    def pytest_collection_finish(self, session: pytest.Session) -> None:
        for item in session.items:
            test_id, case = _identify(item)
            declared = self._tests.get(test_id)
            if declared is None:
                declared = _DeclaredTest(_body(item))
                self._tests[test_id] = declared
            declared.cases.append((case, item.nodeid))

    def pytest_collectreport(self, report: pytest.CollectReport) -> None:
        if report.failed:
            self._broken = "collection errors"

    def pytest_internalerror(self, excrepr: object) -> None:
        self._broken = "an internal pytest error"

    def pytest_runtest_logreport(self, report: pytest.TestReport) -> None:
        if report.outcome == "rerun":  # pytest-rerunfailures: not final
            return
        current = self._outcomes.get(report.nodeid)
        if report.failed:
            # A failure in any phase — setup, call or teardown — is final.
            self._outcomes[report.nodeid] = _OUTCOME_FAIL
        elif report.skipped:
            # Skips and xfails; never downgrades an earlier failure.
            if current is None:
                self._outcomes[report.nodeid] = _OUTCOME_SKIP
        elif report.when == "call" and current is None:
            # A passing setup or teardown alone is not a result.
            self._outcomes[report.nodeid] = _OUTCOME_PASS

    def pytest_sessionfinish(
        self, session: pytest.Session, exitstatus: int | pytest.ExitCode
    ) -> None:
        if self._broken is not None:
            self._announce(
                session.config,
                f"{self._broken}: the declared set is unreliable, "
                "no evidence was emitted",
            )
            return
        if not self._tests:
            self._announce(
                session.config, "the run selected no tests, no evidence was emitted"
            )
            return
        builder = EvidenceBuilder()
        builder.capsula_env()
        for key, value in self._provenance:
            builder.provenance(key, value)
        for test_id, declared in self._tests.items():
            executions = [
                Execution(case, self._outcomes.get(nodeid))
                for case, nodeid in declared.cases
            ]
            builder.add_test(test_id, declared.body, executions)
        self._output.write_text(builder.finish(), encoding="utf-8")
        self._announce(session.config, f"wrote {self._output}")

    @staticmethod
    def _announce(config: pytest.Config, message: str) -> None:
        text = f"inka-evidence: {message}"
        reporter = config.pluginmanager.get_plugin("terminalreporter")
        if reporter is None:
            print(text, file=sys.stderr)  # noqa: T201 - no reporter to write to
        else:
            reporter.write_line(text)
