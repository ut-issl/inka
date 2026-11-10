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

The crates under `crate/inka-evidence-rust/tests/fixtures` are standalone
test inputs with versions of their own, not packages of this repository.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import tomllib
from collections.abc import Callable
from pathlib import Path
from typing import Any, NamedTuple

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
VERSION_LINE = re.compile(r'^version = "([^"]*)"$')
SOURCE_LINE = re.compile(r"^source = ")
FENCE = re.compile(r"^(```|~~~)")

Table = dict[str, Any]


class ReleaseError(Exception):
    """A problem that stops a version check, a bump, or note extraction."""


class Found(NamedTuple):
    """One version field: the file, which field in it, and its value."""

    path: str
    field: str
    version: str


def _read(root: Path, relative: str) -> str:
    try:
        return (root / relative).read_text(encoding="utf-8")
    except OSError as error:
        raise ReleaseError(f"cannot read {relative}: {error.strerror or error}") from error


def _parse_toml(text: str, relative: str) -> Table:
    try:
        return tomllib.loads(text)
    except tomllib.TOMLDecodeError as error:
        raise ReleaseError(f"{relative} is not valid TOML: {error}") from error


def _toml(root: Path, relative: str) -> Table:
    return _parse_toml(_read(root, relative), relative)


def _json(root: Path, relative: str) -> Table:
    try:
        return json.loads(_read(root, relative))
    except json.JSONDecodeError as error:
        raise ReleaseError(f"{relative} is not valid JSON: {error}") from error


def _field(data: Any, relative: str, *keys: str) -> Any:
    """`data[keys[0]][keys[1]]...`, or a ReleaseError naming the field."""
    for key in keys:
        if not isinstance(data, dict) or key not in data:
            dotted = ".".join(f'"{k}"' if k == "" else k for k in keys)
            raise ReleaseError(f"{relative} has no {dotted}")
        data = data[key]
    return data


def _check_semver(version: str) -> None:
    if not SEMVER.fullmatch(version):
        raise ReleaseError(f"{version!r} is not a plain MAJOR.MINOR.PATCH version")


def workspace_version(root: Path) -> str:
    """The version every package must carry: Cargo's workspace version."""
    return _field(_toml(root, CARGO_MANIFEST), CARGO_MANIFEST, "workspace", "package", "version")


def _workspace_members(root: Path) -> list[str]:
    """Package names of the workspace members, as Cargo.lock records them."""
    patterns = _toml(root, CARGO_MANIFEST).get("workspace", {}).get("members", [])
    names = []
    for pattern in patterns:
        for manifest in sorted(root.glob(f"{pattern}/Cargo.toml")):
            relative = manifest.relative_to(root).as_posix()
            names.append(_field(_toml(root, relative), relative, "package", "name"))
    return names


def _lock_packages(lock: Table, relative: str) -> list[Table]:
    packages = lock.get("package", [])
    for package in packages:
        _field(package, relative, "name")
        _field(package, relative, "version")
    return packages


def _cargo_lock_versions(root: Path) -> list[Found]:
    # Workspace members are the only packages without a `source`.
    packages = _lock_packages(_toml(root, CARGO_LOCK), CARGO_LOCK)
    local = {p["name"]: p["version"] for p in packages if "source" not in p}
    found = []
    for name in _workspace_members(root):
        if name not in local:
            raise ReleaseError(f"workspace member {name} is missing from {CARGO_LOCK}; run `cargo update --workspace`")
        found.append(Found(CARGO_LOCK, name, local[name]))
    return found


def _project_name(root: Path, directory: str) -> str:
    relative = _relative(directory, "pyproject.toml")
    return _field(_toml(root, relative), relative, "project", "name")


def _uv_lock_version(root: Path, directory: str) -> Found:
    relative = _relative(directory, "uv.lock")
    name = _project_name(root, directory)
    for package in _lock_packages(_toml(root, relative), relative):
        if package["name"] == name:
            return Found(relative, name, package["version"])
    raise ReleaseError(f"project {name} is missing from {relative}; run `uv lock`")


def versions(root: Path) -> list[Found]:
    """Every version field the release process keeps in step."""
    found = [Found(CARGO_MANIFEST, "workspace.package.version", workspace_version(root))]
    found += _cargo_lock_versions(root)
    for directory in PYTHON_PROJECTS:
        relative = _relative(directory, "pyproject.toml")
        found.append(Found(relative, "project.version", _field(_toml(root, relative), relative, "project", "version")))
        found.append(_uv_lock_version(root, directory))
    for directory in NPM_PACKAGES:
        manifest = _relative(directory, "package.json")
        found.append(Found(manifest, "version", _field(_json(root, manifest), manifest, "version")))
        lock_path = _relative(directory, "package-lock.json")
        lock = _json(root, lock_path)
        found.append(Found(lock_path, "version", _field(lock, lock_path, "version")))
        root_version = _field(lock, lock_path, "packages", "", "version")
        found.append(Found(lock_path, 'packages[""].version', root_version))
    return found


def mismatches(root: Path, found: list[Found] | None = None) -> list[Found]:
    """The version fields that differ from the workspace version."""
    expected = workspace_version(root)
    return [entry for entry in (versions(root) if found is None else found) if entry.version != expected]


def _changed_leaves(old: Any, new: Any, path: tuple[Any, ...] = ()) -> list[tuple[tuple[Any, ...], Any]]:
    """Every value that differs between two parsed documents, by path."""
    if isinstance(old, dict) and isinstance(new, dict) and old.keys() == new.keys():
        return [leaf for key in old for leaf in _changed_leaves(old[key], new[key], (*path, key))]
    if isinstance(old, list) and isinstance(new, list) and len(old) == len(new):
        return [leaf for index, pair in enumerate(zip(old, new)) for leaf in _changed_leaves(*pair, (*path, index))]
    return [] if old == new else [(path, new)]


class _TomlTarget(NamedTuple):
    """Which `version = "..."` lines of a TOML file a bump rewrites: those
    of the `header` tables that `select(name, has_source)` accepts."""

    header: str
    select: Callable[[str | None, bool], bool]
    expected: int


def _rewrite_toml(text: str, relative: str, target: _TomlTarget, version: str) -> str:
    """Rewrite the chosen version lines, leaving every other byte as it was.

    The lines are found textually, so the result is parsed again and must
    differ from the original in exactly those version fields: a header or a
    version line inside a multi-line string fails instead of being edited.
    """
    sections: list[list[str]] = [[]]
    for line in text.splitlines(keepends=True):
        if TABLE_HEADER.match(line):
            sections.append([])
        sections[-1].append(line)

    previous: list[str] = []
    for section in sections:
        if not section or section[0].rstrip("\n") != target.header:
            continue
        bare = [line.rstrip("\n") for line in section]
        names = [m.group(1) for m in map(NAME_LINE.match, bare) if m]
        has_source = any(SOURCE_LINE.match(line) for line in bare)
        if not target.select(names[0] if names else None, has_source):
            continue
        for index, line in enumerate(bare):
            if match := VERSION_LINE.match(line):
                previous.append(match.group(1))
                section[index] = f'version = "{version}"\n'
                break
    if len(previous) != target.expected:
        raise ReleaseError(
            f"expected {target.expected} version field(s) under {target.header} in {relative}, found {len(previous)}"
        )

    rewritten = "".join(line for section in sections for line in section)
    changed = _changed_leaves(_parse_toml(text, relative), _parse_toml(rewritten, relative))
    expected_changes = sum(old != version for old in previous)
    if len(changed) != expected_changes or any(path[-1] != "version" or value != version for path, value in changed):
        raise ReleaseError(f"{relative} is laid out in a way bump cannot edit safely; update its version by hand")
    return rewritten


def _dump_json(data: Table) -> str:
    return json.dumps(data, indent=2, ensure_ascii=False) + "\n"


def _rewrite_json(text: str, relative: str, paths: list[tuple[str, ...]], version: str) -> str:
    try:
        data = json.loads(text)
    except json.JSONDecodeError as error:
        raise ReleaseError(f"{relative} is not valid JSON: {error}") from error
    if _dump_json(data) != text:
        raise ReleaseError(
            f"{relative} is not laid out as npm writes it (two-space indent, one entry per line), "
            "and bump does not reformat files; fix its layout or update its version by hand"
        )
    for path in paths:
        _field(data, relative, *path)
        target = data
        for key in path[:-1]:
            target = target[key]
        target[path[-1]] = version
    return _dump_json(data)


def bump(root: Path, version: str) -> None:
    """Set every version field to `version`.

    Every file is rewritten in memory first, so a failure leaves all of
    them untouched.
    """
    _check_semver(version)
    members = set(_workspace_members(root))
    rewritten: dict[str, str] = {}

    def toml(relative: str, target: _TomlTarget) -> None:
        rewritten[relative] = _rewrite_toml(_read(root, relative), relative, target, version)

    def npm(relative: str, paths: list[tuple[str, ...]]) -> None:
        rewritten[relative] = _rewrite_json(_read(root, relative), relative, paths, version)

    toml(CARGO_MANIFEST, _TomlTarget("[workspace.package]", lambda _n, _s: True, 1))
    toml(CARGO_LOCK, _TomlTarget("[[package]]", lambda n, has_source: n in members and not has_source, len(members)))
    for directory in PYTHON_PROJECTS:
        toml(_relative(directory, "pyproject.toml"), _TomlTarget("[project]", lambda _n, _s: True, 1))
        name = _project_name(root, directory)
        toml(_relative(directory, "uv.lock"), _TomlTarget("[[package]]", lambda n, _s, name=name: n == name, 1))
    for directory in NPM_PACKAGES:
        npm(_relative(directory, "package.json"), [("version",)])
        npm(_relative(directory, "package-lock.json"), [("version",), ("packages", "", "version")])

    for relative, text in rewritten.items():
        (root / relative).write_text(text, encoding="utf-8")


def _headings(lines: list[str]) -> list[int]:
    """Indices of the `## ` headings outside fenced code blocks."""
    headings = []
    fence: str | None = None
    for index, line in enumerate(lines):
        if match := FENCE.match(line):
            if fence is None:
                fence = match.group(1)
            elif line.startswith(fence):
                fence = None
        elif fence is None and line.startswith("## "):
            headings.append(index)
    return headings


def notes(changelog: str, version: str) -> str:
    """The body of the `## v<version>` section of a changelog: everything up
    to the next `## ` heading, without surrounding blank lines."""
    heading = f"## v{version}"
    lines = changelog.splitlines()
    headings = _headings(lines)
    starts = [i for i in headings if lines[i].rstrip() == heading]
    if not starts:
        raise ReleaseError(f"the changelog has no '{heading}' section")
    if len(starts) > 1:
        raise ReleaseError(f"the changelog has more than one '{heading}' section")
    start = starts[0] + 1
    end = next((i for i in headings if i >= start), len(lines))
    body = "\n".join(lines[start:end]).strip("\n")
    if not body.strip():
        raise ReleaseError(f"the '{heading}' section of the changelog is empty")
    return body + "\n"


def _parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description="Keep every inka package on one version, and extract release notes.")
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


def _check(root: Path) -> int:
    expected = workspace_version(root)
    _check_semver(expected)
    found = versions(root)
    wrong = mismatches(root, found)
    for entry in wrong:
        print(f"{entry.path}: {entry.field} is {entry.version}, expected {expected}", file=sys.stderr)
    if wrong:
        print("Run `python3 scripts/release.py bump <version>` to bring them in step.", file=sys.stderr)
        return 1
    print(f"All {len(found)} version fields are {expected}.")
    return 0


def _run(arguments: argparse.Namespace) -> int:
    root: Path = arguments.root
    if arguments.command == "version":
        print(workspace_version(root))
    elif arguments.command == "check":
        return _check(root)
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
