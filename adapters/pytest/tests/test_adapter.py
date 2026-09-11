"""Run real pytest processes with the plugin, then consume their evidence."""

import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


PLUGIN = Path(__file__).resolve().parents[1]
INKA = os.environ.get("INKA_BIN", str(PLUGIN.parents[1] / "target/debug/inka"))


class AdapterTests(unittest.TestCase):
    def run_project(self, source, targets, extra=()):
        temporary = tempfile.TemporaryDirectory(prefix="inka-pytest-")
        self.addCleanup(temporary.cleanup)
        root = Path(temporary.name)
        (root / "test_sample.py").write_text(source, encoding="utf-8")
        plan = {"v": 1, "source": {"id": "github:example/python", "revision": "test"},
                "execution": {"id": "synthetic-test", "attempt": 1},
                "targets": [{"id": name, "file": "test_sample.py", "selector": name} for name in targets]}
        (root / "selection.json").write_text(json.dumps(plan), encoding="utf-8")
        env = dict(os.environ, PYTHONPATH=str(PLUGIN), PYTEST_DISABLE_PLUGIN_AUTOLOAD="1")
        run = subprocess.run([sys.executable, "-m", "pytest", "-p", "inka_pytest", "--inka-plan",
                              "selection.json", "--inka-output", "out", "--inka-environment", "linux",
                              "--inka-bin", INKA, *extra], cwd=root, env=env, capture_output=True, text=True)
        return root, run

    def finish(self, root):
        result = subprocess.run([INKA, "ci-export", "finish", "--receipt", str(root / "out/receipt.json"),
                                 "--results", str(root / "out/results.json"), "--out", str(root / "bundle.json")],
                                capture_output=True, text=True)
        return result, json.loads((root / "bundle.json").read_text()) if (root / "bundle.json").exists() else None

    def test_parameterized_function_and_method_pass(self):
        source = "import pytest\n@pytest.mark.parametrize('x', [1, 2])\ndef test_values(x):\n    assert x > 0\nclass TestGroup:\n    def test_method(self):\n        assert True\n"
        root, run = self.run_project(source, ["test_values", "TestGroup::test_method"])
        self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
        result, bundle = self.finish(root)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(bundle["targets"][0]["required"]), 2)
        self.assertIn("@pytest.mark.parametrize", bundle["targets"][0]["content"])
        self.assertNotIn("class TestGroup", bundle["targets"][0]["content"])
        self.assertEqual(bundle["targets"][1]["kind"], "function")

    def test_skip_teardown_failure_and_unexecuted_cases_do_not_pass(self):
        source = "import pytest\n@pytest.fixture\ndef broken():\n    yield\n    assert False\ndef test_teardown(broken):\n    pass\n@pytest.mark.skip\ndef test_skip():\n    pass\ndef test_fail():\n    assert False\ndef test_later():\n    pass\n"
        root, run = self.run_project(source, ["test_teardown", "test_skip", "test_fail", "test_later"], ["-x"])
        self.assertNotEqual(run.returncode, 0)
        result, bundle = self.finish(root)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(bundle["targets"][0]["results"][0]["result"], "fail")
        self.assertEqual(bundle["targets"][3]["results"], [])
        root, run = self.run_project(source, ["test_skip"], ["-k", "test_skip"])
        result, bundle = self.finish(root)
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(bundle["targets"][0]["results"][0]["result"], "skip")

    def test_source_mutation_prevents_finishing(self):
        source = "from pathlib import Path\ndef test_mutates():\n    Path(__file__).write_text('# replaced')\n"
        root, run = self.run_project(source, ["test_mutates"])
        self.assertEqual(run.returncode, 0, run.stdout + run.stderr)
        result, bundle = self.finish(root)
        self.assertNotEqual(result.returncode, 0)
        self.assertIsNone(bundle)
        self.assertIn("changed after preparation", result.stderr)

    def test_import_time_source_mutation_is_not_relabelled_as_executed_content(self):
        source = "from pathlib import Path\nPath(__file__).write_text('# changed during import')\ndef test_one():\n    pass\n"
        root, run = self.run_project(source, ["test_one"])
        self.assertNotEqual(run.returncode, 0)
        self.assertFalse((root / "out/receipt.json").exists())

    def test_unresolved_function_is_an_explicit_error(self):
        root, run = self.run_project("def test_one():\n    pass\n", ["missing"])
        self.assertNotEqual(run.returncode, 0)
        self.assertFalse((root / "out/receipt.json").exists())


if __name__ == "__main__":
    unittest.main()
