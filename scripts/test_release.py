"""Tests for scripts/release.py.

Run with `python3 -m unittest discover -s scripts`. Every test builds a
small synthetic repository in a temporary directory, so nothing here reads
or writes the real manifests.
"""

from __future__ import annotations

import contextlib
import io
import json
import tempfile
import textwrap
import unittest
from pathlib import Path

import release

CARGO_TOML = """\
[workspace]
members = ["crate/*"]

[workspace.package]
version = "0.1.0"
edition = "2024"

[workspace.dependencies]
serde = { version = "1" }
"""

CARGO_LOCK = """\
version = 4

[[package]]
name = "inka-cli"
version = "0.1.0"
dependencies = [
 "inka-core",
]

[[package]]
name = "inka-core"
version = "0.1.0"

[[package]]
name = "serde"
version = "0.1.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
"""

DOCS_PYPROJECT = """\
[project]
name = "inka"
version = "0.1.0"

[dependency-groups]
dev = ["zensical>=0.0.52"]
"""

DOCS_UV_LOCK = """\
version = 1

[[package]]
name = "inka"
version = "0.1.0"
source = { virtual = "." }

[[package]]
name = "zensical"
version = "0.1.0"
source = { registry = "https://pypi.org/simple" }
"""

PY_PYPROJECT = """\
[project]
name = "inka-evidence"
version = "0.1.0"

[tool.other]
version = "9.9.9"
"""

PY_UV_LOCK = """\
version = 1

[[package]]
name = "inka-evidence"
version = "0.1.0"
source = { editable = "." }
"""

NPM_PACKAGES = ("web", "editors/vscode", "crate/inka-evidence-wasm")


def package_json(name: str, version: str) -> str:
    return json.dumps({"name": name, "version": version, "private": True}, indent=2) + "\n"


def package_lock(name: str, version: str) -> str:
    lock = {
        "name": name,
        "version": version,
        "lockfileVersion": 3,
        "packages": {
            "": {"name": name, "version": version},
            "node_modules/dep": {"version": "0.1.0", "license": "MIT"},
        },
    }
    return json.dumps(lock, indent=2) + "\n"


def write(root: Path, relative: str, text: str) -> None:
    path = root / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")


def make_repository(root: Path) -> None:
    write(root, "Cargo.toml", CARGO_TOML)
    write(root, "Cargo.lock", CARGO_LOCK)
    write(root, "crate/inka-cli/Cargo.toml", '[package]\nname = "inka-cli"\nversion.workspace = true\n')
    write(root, "crate/inka-core/Cargo.toml", '[package]\nname = "inka-core"\nversion.workspace = true\n')
    write(root, "pyproject.toml", DOCS_PYPROJECT)
    write(root, "uv.lock", DOCS_UV_LOCK)
    write(root, "crate/inka-evidence-py/pyproject.toml", PY_PYPROJECT)
    write(root, "crate/inka-evidence-py/uv.lock", PY_UV_LOCK)
    for directory in NPM_PACKAGES:
        name = directory.rsplit("/", 1)[-1]
        write(root, f"{directory}/package.json", package_json(name, "0.1.0"))
        write(root, f"{directory}/package-lock.json", package_lock(name, "0.1.0"))


class RepositoryTest(unittest.TestCase):
    def setUp(self) -> None:
        temp = tempfile.TemporaryDirectory()
        self.addCleanup(temp.cleanup)
        self.root = Path(temp.name)
        make_repository(self.root)


class CheckTest(RepositoryTest):
    def test_agreeing_versions_have_no_mismatch(self) -> None:
        self.assertEqual(release.mismatches(self.root), [])

    def test_every_version_field_is_found(self) -> None:
        # 6 manifests, 2 Cargo.lock members, 2 uv.lock projects, and 2 fields
        # in each of the 3 package-lock.json files.
        self.assertEqual(len(release.versions(self.root)), 6 + 2 + 2 + 6)

    def test_a_disagreeing_manifest_is_reported_by_location(self) -> None:
        write(self.root, "web/package.json", package_json("web", "0.2.0"))

        self.assertEqual(
            release.mismatches(self.root),
            [release.Found("web/package.json", "version", "0.2.0")],
        )

    def test_a_disagreeing_lockfile_entry_is_reported(self) -> None:
        old = 'name = "inka-core"\nversion = "0.1.0"'
        write(self.root, "Cargo.lock", CARGO_LOCK.replace(old, old.replace("0.1.0", "0.0.9")))

        self.assertEqual(
            release.mismatches(self.root),
            [release.Found("Cargo.lock", "inka-core", "0.0.9")],
        )

    def test_a_workspace_member_missing_from_the_lockfile_is_an_error(self) -> None:
        write(self.root, "crate/inka-lsp/Cargo.toml", '[package]\nname = "inka-lsp"\nversion.workspace = true\n')

        with self.assertRaisesRegex(release.ReleaseError, "inka-lsp.*Cargo.lock"):
            release.versions(self.root)


class BumpTest(RepositoryTest):
    def test_bump_rewrites_every_version_field(self) -> None:
        release.bump(self.root, "0.0.1")

        found = release.versions(self.root)
        self.assertEqual({entry.version for entry in found}, {"0.0.1"})
        self.assertEqual(release.workspace_version(self.root), "0.0.1")

    def test_bump_leaves_every_other_line_alone(self) -> None:
        before = {path: (self.root / path).read_text(encoding="utf-8") for path in release.VERSIONED_FILES}

        release.bump(self.root, "0.0.1")

        for path, old in before.items():
            new = (self.root / path).read_text(encoding="utf-8")
            changed = [(a, b) for a, b in zip(old.splitlines(), new.splitlines(), strict=True) if a != b]
            self.assertTrue(changed, f"{path} was not bumped")
            for old_line, new_line in changed:
                self.assertEqual(old_line.replace("0.1.0", "0.0.1"), new_line, path)

    def test_bump_keeps_dependency_and_other_table_versions(self) -> None:
        release.bump(self.root, "0.0.1")

        cargo_lock = (self.root / "Cargo.lock").read_text(encoding="utf-8")
        self.assertIn('name = "serde"\nversion = "0.1.0"', cargo_lock)
        self.assertIn('serde = { version = "1" }', (self.root / "Cargo.toml").read_text(encoding="utf-8"))
        python = (self.root / "crate/inka-evidence-py/pyproject.toml").read_text(encoding="utf-8")
        self.assertIn('[tool.other]\nversion = "9.9.9"', python)
        npm_lock = json.loads((self.root / "web/package-lock.json").read_text(encoding="utf-8"))
        self.assertEqual(npm_lock["packages"]["node_modules/dep"]["version"], "0.1.0")

    def test_bump_rejects_a_version_that_is_not_plain_semver(self) -> None:
        for version in ["1.0", "v1.0.0", "01.0.0", "1.0.0-rc.1", "1.0.0+build", ""]:
            with self.subTest(version=version), self.assertRaises(release.ReleaseError):
                release.bump(self.root, version)

        self.assertEqual(release.workspace_version(self.root), "0.1.0")


class NotesTest(unittest.TestCase):
    CHANGELOG = textwrap.dedent(
        """\
        # Changelog

        Preamble.

        ## v0.0.2

        Second.

        ### Fixed

        - A fix.

        ## v0.0.1

        First.
        """
    )

    def test_notes_are_the_section_body_up_to_the_next_release(self) -> None:
        self.assertEqual(release.notes(self.CHANGELOG, "0.0.2"), "Second.\n\n### Fixed\n\n- A fix.\n")

    def test_the_last_section_runs_to_the_end_of_the_file(self) -> None:
        self.assertEqual(release.notes(self.CHANGELOG, "0.0.1"), "First.\n")

    def test_a_missing_section_is_an_error(self) -> None:
        with self.assertRaisesRegex(release.ReleaseError, "## v0.0.3"):
            release.notes(self.CHANGELOG, "0.0.3")

    def test_a_heading_only_matches_exactly(self) -> None:
        with self.assertRaises(release.ReleaseError):
            release.notes("## v0.0.10\n\nTen.\n", "0.0.1")

    def test_an_empty_section_is_an_error(self) -> None:
        with self.assertRaisesRegex(release.ReleaseError, "empty"):
            release.notes("## v0.0.2\n\n## v0.0.1\n\nFirst.\n", "0.0.2")


class BumpGuardTest(RepositoryTest):
    """The cases bump's guards exist for: each must fail loudly or pick the
    right field, and a failure must leave every file as it was."""

    def snapshot(self) -> dict[str, str]:
        return {path: (self.root / path).read_text(encoding="utf-8") for path in release.VERSIONED_FILES}

    def test_a_registry_crate_named_like_a_member_is_left_alone(self) -> None:
        registry = '\n[[package]]\nname = "inka-core"\nversion = "0.1.0"\nsource = "registry+https://example.invalid"\n'
        write(self.root, "Cargo.lock", CARGO_LOCK + registry)

        release.bump(self.root, "0.0.1")

        lock = (self.root / "Cargo.lock").read_text(encoding="utf-8")
        self.assertTrue(lock.endswith(registry))
        self.assertEqual(lock.count('version = "0.0.1"'), 2)

    def test_a_block_whose_name_is_not_its_first_line_is_still_found(self) -> None:
        reordered = CARGO_LOCK.replace('name = "inka-core"\nversion = "0.1.0"', 'version = "0.1.0"\nname = "inka-core"')
        write(self.root, "Cargo.lock", reordered)

        release.bump(self.root, "0.0.1")

        self.assertEqual(release.mismatches(self.root), [])

    def test_a_header_inside_a_multiline_string_is_refused(self) -> None:
        tricky = CARGO_TOML.replace(
            '[workspace.package]\nversion = "0.1.0"',
            '[workspace.package]\ndescription = """\n[workspace.package]\nversion = "9.9.9"\n"""\nversion = "0.1.0"',
        )
        write(self.root, "Cargo.toml", tricky)
        before = self.snapshot()

        with self.assertRaisesRegex(release.ReleaseError, "Cargo.toml"):
            release.bump(self.root, "0.0.1")

        self.assertEqual(self.snapshot(), before)

    def test_a_failure_in_a_late_file_leaves_the_early_ones_untouched(self) -> None:
        compact = json.dumps(json.loads(package_lock("web", "0.1.0"))) + "\n"
        write(self.root, "web/package-lock.json", compact)
        before = self.snapshot()

        with self.assertRaisesRegex(release.ReleaseError, "web/package-lock.json"):
            release.bump(self.root, "0.0.1")

        self.assertEqual(self.snapshot(), before)

    def test_a_missing_field_names_the_file_and_the_field(self) -> None:
        write(self.root, "pyproject.toml", '[project]\nname = "inka"\n')

        with self.assertRaisesRegex(release.ReleaseError, r"pyproject.toml has no project\.version"):
            release.versions(self.root)


class MainTest(RepositoryTest):
    def run_main(self, *argv: str) -> tuple[int, str, str]:
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            code = release.main(["--root", str(self.root), *argv])
        return code, out.getvalue(), err.getvalue()

    def test_check_succeeds_when_versions_agree(self) -> None:
        self.assertEqual(self.run_main("check"), (0, "All 16 version fields are 0.1.0.\n", ""))

    def test_check_names_each_disagreeing_field(self) -> None:
        write(self.root, "web/package.json", package_json("web", "0.2.0"))

        code, _, err = self.run_main("check")

        self.assertEqual(code, 1)
        self.assertIn("web/package.json: version is 0.2.0, expected 0.1.0", err)

    def test_an_error_is_reported_without_a_traceback(self) -> None:
        code, out, err = self.run_main("bump", "1.0")

        self.assertEqual((code, out), (1, ""))
        self.assertTrue(err.startswith("error: '1.0' is not a plain MAJOR.MINOR.PATCH version"), err)


class NotesEdgeTest(unittest.TestCase):
    def test_crlf_and_a_trailing_space_after_the_heading_are_accepted(self) -> None:
        self.assertEqual(release.notes("## v0.0.1  \r\n\r\nFirst.\r\n", "0.0.1"), "First.\n")

    def test_a_heading_inside_a_code_fence_does_not_end_the_section(self) -> None:
        changelog = "## v0.0.1\n\nText.\n\n```md\n## Not a heading\n```\n\nMore.\n\n## v0.0.0\n\nOld.\n"

        self.assertEqual(release.notes(changelog, "0.0.1"), "Text.\n\n```md\n## Not a heading\n```\n\nMore.\n")

    def test_a_release_heading_inside_a_code_fence_is_not_a_section(self) -> None:
        changelog = "~~~\n## v0.0.1\n~~~\n\n## v0.0.1\n\nReal.\n"

        self.assertEqual(release.notes(changelog, "0.0.1"), "Real.\n")

    def test_a_duplicated_section_is_an_error(self) -> None:
        with self.assertRaisesRegex(release.ReleaseError, "more than one"):
            release.notes("## v0.0.1\n\nA.\n\n## v0.0.1\n\nB.\n", "0.0.1")


if __name__ == "__main__":
    unittest.main()
