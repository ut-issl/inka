import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[3]


class PublishTests(unittest.TestCase):
    def run_validation(self, bundle, runner="Linux"):
        with tempfile.TemporaryDirectory(prefix="inka action ") as directory:
            path = Path(directory)
            original = path / "evidence [one].json"
            original.write_text(json.dumps(bundle), encoding="utf-8")
            output = path / "outputs"
            output.touch()
            env = {**os.environ, "RUNNER_OS": runner, "RUNNER_TEMP": directory,
                   "GITHUB_OUTPUT": str(output), "INKA_BUNDLE": str(original),
                   "INKA_BIN": os.environ.get("INKA_BIN", str(ROOT / "target/debug/inka"))}
            result = subprocess.run(["bash", str(ROOT / "actions/publish-evidence/validate.sh")],
                                    env=env, capture_output=True, text=True)
            lines = output.read_text().splitlines()
            if lines:
                staged = Path(lines[0].removeprefix("directory=")) / "evidence.json"
                self.assertEqual(json.loads(staged.read_text()), bundle)
                original.write_text("changed after validation")
                self.assertEqual(json.loads(staged.read_text()), bundle)
            return result, lines

    def test_valid_pass_and_nonpass_snapshots_are_publishable(self):
        vectors = json.loads((ROOT / "tests/golden/evidence/v2/contracts.json").read_text())
        for vector in vectors:
            if vector["valid"]:
                with self.subTest(vector["name"]):
                    result, lines = self.run_validation(vector["bundle"])
                    self.assertEqual(result.returncode, 0, result.stderr)
                    self.assertEqual(len(lines), 1)

    def test_invalid_input_and_other_runners_do_not_emit_upload_path(self):
        for bundle, runner in [({"v": 99}, "Linux"), ({}, "Windows")]:
            result, lines = self.run_validation(bundle, runner)
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(lines, [])
