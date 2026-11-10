#!/usr/bin/env python3
"""Keep every inka package on one version, and extract release notes.

Every package in the repository (the Cargo workspace, both Python projects,
and the three npm packages) carries the same version, recorded once in its
manifest and again in its lockfile. This script is the one place that knows
where those fields live:

    python3 scripts/release.py version        # print the workspace version
    python3 scripts/release.py check          # fail if any field disagrees
    python3 scripts/release.py bump 0.0.2     # rewrite every field
    python3 scripts/release.py notes 0.0.1 [--changelog CHANGELOG.ja.md]

`Cargo.toml`'s `[workspace.package]` version is the reference the others
must match. Versions are plain `MAJOR.MINOR.PATCH`: pre-release and build
suffixes are spelled differently by Cargo, Python, and npm, so none is
accepted. Only the standard library is used (Python 3.11 or later).
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import tomllib
from collections.abc import Callable
from pathlib import Path
from typing import NamedTuple

CARGO_MANIFEST = "Cargo.toml"
CARGO_LOCK = "Cargo.lock"
PYTHON_PROJECTS = (".", "crate/inka-evidence-py")
NPM_PACKAGES = ("web", "editors/vscode", "crate/inka-evidence-wasm")


def _relative(directory: str, name: str) -> str:
    return name if directory == "." else f"{directory}/{name}"


VERSIONED_FILES = (
    CARGO_MANIFEST,
    CARGO_LOCK,
    *(_relative(d, "pyproject.toml") for d in PYTHON_PROJECTS),
    *(_relative(d, "uv.lock") for d in PYTHON_PROJECTS),
    *(_relative(d, "package.json") for d in NPM_PACKAGES),
    *(_relative(d, "package-lock.json") for d in NPM_PACKAGES),
)

SEMVER = re.compile(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)")
TABLE_HEADER = re.compile(r"^\[")
NAME_LINE = re.compile(r'^name = "([^"]*)"$')
VERSION_LINE = re.compile(r'^version = "[^"]*"$')
SOURCE_LINE = re.compile(r"^source = ")


class ReleaseError(Exception):
    """A problem that stops a version check, a bump, or note extraction."""


class Found(NamedTuple):
    """One version field: the file, which field in it, and its value."""

    path: str
    field: str
    version: str


def _read(root: Path, relative: str) -> str:
    path = root / relative
    try:
        return path.read_text(encoding="utf-8")
    except OSError as error:
        raise ReleaseError(f"cannot read {relative}: {error.strerror}") from error


def _toml(root: Path, relative: str) -> dict:
    try:
        return tomllib.loads(_read(root, relative))
    except tomllib.TOMLDecodeError as error:
        raise ReleaseError(f"{relative} is not valid TOML: {error}") from error


def _json(root: Path, relative: str) -> dict:
    try:
        return json.loads(_read(root, relative))
    except json.JSONDecodeError as error:
        raise ReleaseError(f"{relative} is not valid JSON: {error}") from error


def _check_semver(version: str) -> None:
    if not SEMVER.fullmatch(version):
        raise ReleaseError(f"{version!r} is not a plain MAJOR.MINOR.PATCH version")


def workspace_version(root: Path) -> str:
    """The version every package must carry: Cargo's workspace version."""
    try:
        return _toml(root, CARGO_MANIFEST)["workspace"]["package"]["version"]
    except KeyError as error:
        raise ReleaseError(f"{CARGO_MANIFEST} has no [workspace.package] version") from error


def _workspace_members(root: Path) -> list[str]:
    """Package names of the workspace members, as Cargo.lock records them."""
    patterns = _toml(root, CARGO_MANIFEST).get("workspace", {}).get("members", [])
    names = []
    for pattern in patterns:
        for manifest in sorted(root.glob(f"{pattern}/Cargo.toml")):
            relative = manifest.relative_to(root).as_posix()
            names.append(_toml(root, relative)["package"]["name"])
    return names


def _lock_packages(lock: dict) -> list[dict]:
    return lock.get("package", [])


def _cargo_lock_versions(root: Path) -> list[Found]:
    # Workspace members are the only packages without a `source`.
    local = {p["name"]: p["version"] for p in _lock_packages(_toml(root, CARGO_LOCK)) if "source" not in p}
    found = []
    for name in _workspace_members(root):
        if name not in local:
            raise ReleaseError(f"workspace member {name} is missing from {CARGO_LOCK}; run `cargo update --workspace`")
        found.append(Found(CARGO_LOCK, name, local[name]))
    return found


def _project_name(root: Path, directory: str) -> str:
    return _toml(root, _relative(directory, "pyproject.toml"))["project"]["name"]


def _uv_lock_version(root: Path, directory: str) -> Found:
    relative = _relative(directory, "uv.lock")
    name = _project_name(root, directory)
    for package in _lock_packages(_toml(root, relative)):
        if package["name"] == name:
            return Found(relative, name, package["version"])
    raise ReleaseError(f"project {name} is missing from {relative}; run `uv lock`")


def versions(root: Path) -> list[Found]:
    """Every version field the release process keeps in step."""
    found = [Found(CARGO_MANIFEST, "workspace.package.version", workspace_version(root))]
    found += _cargo_lock_versions(root)
    for directory in PYTHON_PROJECTS:
        relative = _relative(directory, "pyproject.toml")
        found.append(Found(relative, "project.version", _toml(root, relative)["project"]["version"]))
        found.append(_uv_lock_version(root, directory))
    for directory in NPM_PACKAGES:
        manifest = _relative(directory, "package.json")
        found.append(Found(manifest, "version", _json(root, manifest)["version"]))
        lock_path = _relative(directory, "package-lock.json")
        lock = _json(root, lock_path)
        found.append(Found(lock_path, "version", lock["version"]))
        found.append(Found(lock_path, 'packages[""].version', lock["packages"][""]["version"]))
    return found


def mismatches(root: Path) -> list[Found]:
    """The version fields that differ from the workspace version."""
    expected = workspace_version(root)
    return [entry for entry in versions(root) if entry.version != expected]


def _rewrite_toml(
    text: str,
    relative: str,
    header: str,
    select: Callable[[str | None, bool], bool],
    expected: int,
    version: str,
) -> str:
    """Rewrite the `version = "..."` line of every `header` table chosen by
    `select(name, has_source)`, leaving every other byte as it was."""
    lines = text.splitlines(keepends=True)
    sections: list[list[str]] = [[]]
    for line in lines:
        if TABLE_HEADER.match(line):
            sections.append([])
        sections[-1].append(line)

    rewritten = 0
    for section in sections:
        if not section or section[0].rstrip("\n") != header:
            continue
        bare = [line.rstrip("\n") for line in section]
        names = [m.group(1) for m in map(NAME_LINE.match, bare) if m]
        has_source = any(SOURCE_LINE.match(line) for line in bare)
        if not select(names[0] if names else None, has_source):
            continue
        for index, line in enumerate(bare):
            if VERSION_LINE.match(line):
                section[index] = f'version = "{version}"\n'
                rewritten += 1
                break
    if rewritten != expected:
        raise ReleaseError(f"expected {expected} version field(s) under {header} in {relative}, found {rewritten}")
    return "".join(line for section in sections for line in section)


def _rewrite_json(text: str, relative: str, paths: list[tuple[str, ...]], version: str) -> str:
    data = json.loads(text)
    if _dump_json(data) != text:
        # Rewriting would also reformat the file; refuse rather than churn it.
        raise ReleaseError(f"{relative} is not formatted as npm writes it; run npm install first")
    for path in paths:
        target = data
        for key in path[:-1]:
            target = target[key]
        target[path[-1]] = version
    return _dump_json(data)


def _dump_json(data: dict) -> str:
    return json.dumps(data, indent=2, ensure_ascii=False) + "\n"


def bump(root: Path, version: str) -> None:
    """Set every version field to `version`.

    Every file is rewritten in memory first, so a failure leaves all of
    them untouched.
    """
    _check_semver(version)
    members = set(_workspace_members(root))
    rewritten: dict[str, str] = {}

    rewritten[CARGO_MANIFEST] = _rewrite_toml(
        _read(root, CARGO_MANIFEST), CARGO_MANIFEST, "[workspace.package]", lambda _n, _s: True, 1, version
    )
    rewritten[CARGO_LOCK] = _rewrite_toml(
        _read(root, CARGO_LOCK),
        CARGO_LOCK,
        "[[package]]",
        lambda name, has_source: name in members and not has_source,
        len(members),
        version,
    )
    for directory in PYTHON_PROJECTS:
        manifest = _relative(directory, "pyproject.toml")
        rewritten[manifest] = _rewrite_toml(
            _read(root, manifest), manifest, "[project]", lambda _n, _s: True, 1, version
        )
        lock = _relative(directory, "uv.lock")
        name = _project_name(root, directory)
        rewritten[lock] = _rewrite_toml(
            _read(root, lock), lock, "[[package]]", lambda n, _s, name=name: n == name, 1, version
        )
    for directory in NPM_PACKAGES:
        manifest = _relative(directory, "package.json")
        rewritten[manifest] = _rewrite_json(_read(root, manifest), manifest, [("version",)], version)
        lock = _relative(directory, "package-lock.json")
        rewritten[lock] = _rewrite_json(
            _read(root, lock), lock, [("version",), ("packages", "", "version")], version
        )

    for relative, text in rewritten.items():
        (root / relative).write_text(text, encoding="utf-8")


def notes(changelog: str, version: str) -> str:
    """The body of the `## v<version>` section of a changelog: everything up
    to the next `## ` heading, without surrounding blank lines."""
    heading = f"## v{version}"
    lines = changelog.splitlines()
    try:
        start = next(i for i, line in enumerate(lines) if line.rstrip() == heading) + 1
    except StopIteration:
        raise ReleaseError(f"the changelog has no '{heading}' section") from None
    end = next((i for i in range(start, len(lines)) if lines[i].startswith("## ")), len(lines))
    body = "\n".join(lines[start:end]).strip("\n")
    if not body.strip():
        raise ReleaseError(f"the '{heading}' section of the changelog is empty")
    return body + "\n"


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n", 1)[0])
    parser.add_argument(
        "--root",
        type=Path,
        default=Path(__file__).resolve().parent.parent,
        help="repository root (default: the parent of scripts/)",
    )
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("version", help="print the workspace version")
    commands.add_parser("check", help="fail unless every version field matches the workspace version")
    bump_command = commands.add_parser("bump", help="set every version field to VERSION")
    bump_command.add_argument("version")
    notes_command = commands.add_parser("notes", help="print the changelog section of VERSION")
    notes_command.add_argument("version")
    notes_command.add_argument("--changelog", default="CHANGELOG.md", help="relative to the root")
    return parser


def _run(arguments: argparse.Namespace) -> int:
    root: Path = arguments.root
    if arguments.command == "version":
        print(workspace_version(root))
    elif arguments.command == "check":
        expected = workspace_version(root)
        _check_semver(expected)
        wrong = mismatches(root)
        for entry in wrong:
            print(f"{entry.path}: {entry.field} is {entry.version}, expected {expected}", file=sys.stderr)
        if wrong:
            print("Run `python3 scripts/release.py bump <version>` to bring them in step.", file=sys.stderr)
            return 1
        print(f"All {len(versions(root))} version fields are {expected}.")
    elif arguments.command == "bump":
        bump(root, arguments.version)
        print(f"Set every version field to {arguments.version}.")
    elif arguments.command == "notes":
        print(notes(_read(root, arguments.changelog), arguments.version), end="")
    return 0


def main(argv: list[str] | None = None) -> int:
    arguments = _parser().parse_args(argv)
    try:
        return _run(arguments)
    except ReleaseError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
