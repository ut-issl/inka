"""Source-side pytest adapter; inka itself never provisions or runs pytest."""

import ast
import hashlib
import json
import os
from pathlib import Path
import subprocess

import pytest


def extract(source, selector):
    """Select an explicit function/method and its decorators using AST spans."""
    nodes = ast.parse(source).body
    parts = selector.split("::")
    for index, part in enumerate(parts):
        matches = [node for node in nodes if getattr(node, "name", None) == part]
        if len(matches) != 1:
            raise ValueError(f"ambiguous or missing Python selector: {selector}")
        node = matches[0]
        if index < len(parts) - 1:
            if not isinstance(node, ast.ClassDef):
                raise ValueError("only explicit class methods and module functions are supported")
            nodes = node.body
        elif not isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
            raise ValueError("the selected contract must be a function")
    start = min([node.lineno] + [decorator.lineno for decorator in node.decorator_list])
    lines = source.encode("utf-8").splitlines(keepends=True)
    first = sum(map(len, lines[:start - 1])) + node.col_offset
    last = sum(map(len, lines[:node.end_lineno - 1])) + node.end_col_offset
    return source.encode("utf-8")[first:last].decode("utf-8")


def write_json(path, value):
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def pytest_addoption(parser):
    group = parser.getgroup("inka evidence")
    group.addoption("--inka-plan", help="Source-side JSON selection plan")
    group.addoption("--inka-output", default=".inka-pytest", help="New output directory for this execution")
    group.addoption("--inka-environment", default="default", help="Required-case environment identity")
    group.addoption("--inka-bin", default=os.environ.get("INKA_BIN", "inka"), help="Inka CLI executable")


def pytest_configure(config):
    if config.getoption("inka_plan"):
        if hasattr(config, "workerinput") or getattr(config.option, "numprocesses", 0):
            raise pytest.UsageError("inka's initial pytest adapter requires a single process (no xdist)")
        config.pluginmanager.register(EvidencePlugin(config), "inka-evidence-producer")


class EvidencePlugin:
    def __init__(self, config):
        self.config = config
        self.plan_path = Path(config.getoption("inka_plan")).resolve()
        self.plan = json.loads(self.plan_path.read_text(encoding="utf-8"))
        if set(self.plan) != {"v", "source", "execution", "targets"} or self.plan["v"] != 1:
            raise pytest.UsageError("invalid pytest evidence selection plan")
        self.out = Path(config.getoption("inka_output")).resolve()
        if self.out.exists():
            raise pytest.UsageError("inka output directory already exists; use a fresh execution directory")
        self.out.mkdir(parents=True)
        self.environment = config.getoption("inka_environment")
        if not self.environment.strip():
            raise pytest.UsageError("inka environment must not be blank")
        self.sources = {}
        self.stages = {}
        self.selected = {}
        self.receipt = None
        self.duplicate_report = False

    def pytest_collectstart(self, collector):
        path = Path(collector.path).resolve()
        if path.suffix == ".py" and path.is_file() and path not in self.sources:
            # Capture before importing the module, including assertion rewriting.
            self.sources[path] = path.read_bytes()

    def pytest_collection_finish(self, session):
        if session.testsfailed:
            return
        targets = []
        guards = []
        used = set()
        try:
            for index, spec in enumerate(self.plan["targets"]):
                if set(spec) != {"id", "file", "selector"}:
                    raise ValueError("target requires exactly id, file and selector")
                path = (self.plan_path.parent / spec["file"]).resolve()
                original = self.sources.get(path)
                if original is None:
                    raise ValueError(f"selected source was not collected: {path}")
                content = extract(original.decode("utf-8"), spec["selector"])
                items = []
                for item in session.items:
                    if Path(item.path).resolve() != path:
                        continue
                    parts = item.nodeid.split("::")[1:]
                    parts[-1] = getattr(item, "originalname", None) or item.name
                    if "::".join(parts) == spec["selector"]:
                        code = getattr(getattr(item, "obj", None), "__code__", None)
                        if code is None or Path(code.co_filename).resolve() != path:
                            raise ValueError("externally wrapped/generated Python tests are unsupported")
                        items.append(item)
                if not items:
                    raise ValueError(f"selected function has no collected cases: {spec['selector']}")
                required = [f"{self.environment}/{item.nodeid}" for item in items]
                if len(required) != len(set(required)):
                    raise ValueError("duplicate collected pytest case identity")
                contract = self.out / f"contract-{index}.txt"
                contract.write_text(content, encoding="utf-8")
                targets.append({"id": spec["id"], "file": str(contract), "kind": "function",
                                "profile": "pytest-source:v1", "required": required})
                self.selected[spec["id"]] = [item.nodeid for item in items]
                if path not in used:
                    guards.append({"path": str(path), "sha256": "sha256:" + hashlib.sha256(original).hexdigest()})
                    used.add(path)
            generated = {"v": 1, "source": self.plan["source"], "execution": self.plan["execution"],
                         "targets": targets, "guards": guards}
            write_json(self.out / "plan.json", generated)
            subprocess.run([self.config.getoption("inka_bin"), "ci-export", "prepare", "--plan",
                            str(self.out / "plan.json"), "--out", str(self.out / "receipt.json")], check=True)
            self.receipt = json.loads((self.out / "receipt.json").read_text(encoding="utf-8"))
        except (ValueError, OSError, subprocess.CalledProcessError) as error:
            raise pytest.UsageError(f"cannot prepare inka evidence: {error}") from error

    def pytest_runtest_logreport(self, report):
        stages = self.stages.setdefault(report.nodeid, {})
        if report.when in stages:
            self.duplicate_report = True
        stages[report.when] = "skip" if getattr(report, "wasxfail", None) else report.outcome

    def pytest_sessionfinish(self, session, exitstatus):
        if self.receipt is None:
            return
        targets = []
        for target in self.receipt["bundle"]["targets"]:
            results = []
            for nodeid in self.selected[target["id"]]:
                stages = self.stages.get(nodeid, {})
                values = list(stages.values())
                if self.duplicate_report or exitstatus in (2, 3, 4):
                    outcome = "cancelled"
                elif "failed" in values:
                    outcome = "fail"
                elif "skipped" in values or "skip" in values:
                    outcome = "skip"
                elif all(stages.get(stage) == "passed" for stage in ("setup", "call", "teardown")):
                    outcome = "pass"
                else:
                    continue
                results.append({"case": f"{self.environment}/{nodeid}", "result": outcome})
            targets.append({"id": target["id"], "interface_hash": target["interface_hash"], "results": results})
        write_json(self.out / "results.json", {"v": 1, "source": self.plan["source"],
                                              "execution": self.plan["execution"], "targets": targets})
