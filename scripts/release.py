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
    python3 scripts/release.py prepare patch [--dry-run]   # start the release branch

`prepare` starts a release. It takes the part of the version to increment
(`major`, `minor` or `patch`; the parts after it become 0) or an explicit
version above the current one. From the tip of `origin/main` it creates the
branch `release/v<version>`, bumps every field, and drafts the
`## v<version>` section of both changelogs from the pull requests merged
since the previous release. It commits and pushes nothing: it prints the
commands that do, and a description for the pull request
`chore(release): v<version>` that lists every one of those pull requests.
It needs only `git`, and reads the pull requests from the commits their
merges left on `main`. The drafted sections carry a marker that `check` and
`notes` refuse, so rewrite them before committing: a pull request that still
holds a draft fails its checks.

`Cargo.toml`'s `[workspace.package]` version is the reference the others
must match. The `[workspace.dependencies]` entries that point at a workspace
member by `path` carry the same version as their requirement, because
`cargo publish` replaces the path with that requirement. Versions are plain `MAJOR.MINOR.PATCH`: pre-release and build
suffixes are spelled differently by Cargo, Python, and npm, so none is
accepted. Only the standard library is used (Python 3.11 or later).

The crates under `crate/inka-evidence-rust/tests/fixtures` are standalone
test inputs with versions of their own, not packages of this repository.
"""

from __future__ import annotations

import argparse
import json
import re
import shlex
import subprocess
import sys
import textwrap
import tomllib
from collections.abc import Callable
from pathlib import Path
from typing import Any, NamedTuple

CARGO_MANIFEST = "Cargo.toml"
CARGO_LOCK = "Cargo.lock"
PYTHON_PROJECTS = (".", "crate/inka-evidence-py")
NPM_PACKAGES = ("web", "editors/vscode", "crate/inka-evidence-wasm")
CHANGELOGS = ("CHANGELOG.md", "CHANGELOG.ja.md")
BASE_BRANCH = "main"
# Opens the comment that marks a changelog section as an unedited draft.
DRAFT_MARKER = "<!-- release-draft"
# MD013 in .markdownlint-cli2.yaml.
MARKDOWN_WIDTH = 120
VERSION_PARTS = ("major", "minor", "patch")
# The subject GitHub gives the commit of a merged pull request.
MERGED_SUBJECT = re.compile(r"(.+) \(#(\d+)\)")


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
DEPENDENCY_VERSION = re.compile(r'^(?P<head>[A-Za-z0-9_-]+ = \{.*\bversion = ")(?P<version>[^"]*)(?P<tail>".*\})$')
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


def _path_dependencies(manifest: Table) -> dict[str, Any]:
    """The `[workspace.dependencies]` entries that name a workspace member by
    `path`: the ones whose version requirement must track the workspace."""
    dependencies = manifest.get("workspace", {}).get("dependencies", {})
    return {name: spec for name, spec in dependencies.items() if isinstance(spec, dict) and "path" in spec}


def _path_dependency_versions(root: Path) -> list[Found]:
    manifest = _toml(root, CARGO_MANIFEST)
    found = []
    for name, spec in _path_dependencies(manifest).items():
        field = f"workspace.dependencies.{name}.version"
        if "version" not in spec:
            raise ReleaseError(
                f"{CARGO_MANIFEST}: {field} is missing; cargo publish needs the requirement next to the path"
            )
        found.append(Found(CARGO_MANIFEST, field, spec["version"]))
    return found


def versions(root: Path) -> list[Found]:
    """Every version field the release process keeps in step."""
    found = [Found(CARGO_MANIFEST, "workspace.package.version", workspace_version(root))]
    found += _path_dependency_versions(root)
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


def _rewrite_path_dependencies(text: str, relative: str, version: str) -> str:
    """Rewrite the version requirement of every path dependency under
    `[workspace.dependencies]`, each written as one inline table on a line
    of its own; any other layout is refused rather than guessed at."""
    names = set(_path_dependencies(_parse_toml(text, relative)))
    if not names:
        return text
    sections: list[list[str]] = [[]]
    for line in text.splitlines(keepends=True):
        if TABLE_HEADER.match(line):
            sections.append([])
        sections[-1].append(line)

    rewritten: set[str] = set()
    for section in sections:
        if not section or section[0].rstrip("\n") != "[workspace.dependencies]":
            continue
        for index, line in enumerate(section):
            match = DEPENDENCY_VERSION.match(line.rstrip("\n"))
            if match and match.group("head").split(" ", 1)[0] in names:
                section[index] = f'{match.group("head")}{version}{match.group("tail")}\n'
                rewritten.add(match.group("head").split(" ", 1)[0])
    missing = sorted(names - rewritten)
    if missing:
        raise ReleaseError(
            f"{relative}: the path dependency {', '.join(missing)} is not one inline table line with a version "
            "under [workspace.dependencies]; update its version by hand"
        )

    result = "".join(line for section in sections for line in section)
    changed = _changed_leaves(_parse_toml(text, relative), _parse_toml(result, relative))
    expected = {("workspace", "dependencies", name, "version") for name in names}
    if any(path not in expected or value != version for path, value in changed):
        raise ReleaseError(f"{relative} is laid out in a way bump cannot edit safely; update its version by hand")
    return result


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
    rewritten[CARGO_MANIFEST] = _rewrite_path_dependencies(rewritten[CARGO_MANIFEST], CARGO_MANIFEST, version)
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
    if DRAFT_MARKER in body:
        raise ReleaseError(f"the '{heading}' section of the changelog is still the generated draft")
    return body + "\n"


class PullRequest(NamedTuple):
    """A merged pull request, as far as a release describes it."""

    number: int
    title: str
    automated: bool


def _version_key(version: str) -> tuple[int, ...]:
    _check_semver(version)
    return tuple(int(part) for part in version.split("."))


def _wrap_item(text: str) -> list[str]:
    """A list item within the line length markdownlint enforces."""
    return textwrap.wrap(text, width=MARKDOWN_WIDTH, initial_indent="- ", subsequent_indent="  ")


def next_version(previous: str, target: str) -> str:
    """The version a release moves to from `previous`: `target` names the
    part to increment, or is itself a version above `previous`."""
    major, minor, patch = _version_key(previous)
    if target in VERSION_PARTS:
        parts = {"major": (major + 1, 0, 0), "minor": (major, minor + 1, 0), "patch": (major, minor, patch + 1)}
        return ".".join(map(str, parts[target]))
    if not SEMVER.fullmatch(target):
        raise ReleaseError(f"{target!r} is neither {', '.join(VERSION_PARTS)} nor a plain MAJOR.MINOR.PATCH version")
    if _version_key(target) <= _version_key(previous):
        raise ReleaseError(f"{target} is not above the current version {previous}")
    return target


def draft_section(version: str, pull_requests: list[PullRequest], language: str = "English") -> str:
    """A `## v<version>` section to start the release notes from: one item
    per pull request a person wrote, under the marker that keeps the draft
    from being merged or released as it stands."""
    written = [pr for pr in pull_requests if not pr.automated]
    lines = [
        f"## v{version}",
        "",
        DRAFT_MARKER,
        "Generated by `scripts/release.py prepare` from the merged pull requests.",
        f"Rewrite this section as release notes in {language}, then delete this comment.",
        "-->",
        "",
        *(line for pr in written for line in _wrap_item(f"{pr.title} (#{pr.number})")),
    ]
    if not written:
        lines.append("- Dependency updates only.")
    return "\n".join(lines) + "\n"


def insert_section(changelog: str, version: str, section: str) -> str:
    """The changelog with `section` above its newest release."""
    heading = f"## v{version}"
    lines = changelog.splitlines(keepends=True)
    headings = _headings([line.rstrip("\r\n") for line in lines])
    if any(lines[i].rstrip() == heading for i in headings):
        raise ReleaseError(f"the changelog already has a '{heading}' section")
    if not headings:
        return changelog.rstrip("\n") + "\n\n" + section
    at = headings[0]
    return "".join(lines[:at]) + section + "\n" + "".join(lines[at:])


def pull_request_body(version: str, previous: str, pull_requests: list[PullRequest]) -> str:
    """The description of the release pull request. A bare `#123` list item
    is rendered by GitHub with the title and state of that pull request."""
    written = [pr for pr in pull_requests if not pr.automated]
    automated = [pr for pr in pull_requests if pr.automated]
    changelogs = " and ".join(f"`{name}`" for name in CHANGELOGS)
    lines = [
        "## Description",
        "",
        f"Prepare the v{version} release: bump every package from {previous} to {version} "
        f"(`scripts/release.py bump`) and add the `## v{version}` section to {changelogs}.",
        "",
        f"After this is merged, run the `Release` workflow on `{BASE_BRANCH}` with `mode: release` "
        "(see RELEASING.md).",
        "",
        f"## Pull requests included since v{previous}",
        "",
        "### Changes by maintainers",
        "",
        *([f"- #{pr.number}" for pr in written] or ["None."]),
    ]
    if automated:
        noun = "pull request" if len(automated) == 1 else "pull requests"
        lines += [
            "",
            "### Automated updates",
            "",
            "<details>",
            f"<summary>{len(automated)} {noun}</summary>",
            "",
            *(f"- #{pr.number}" for pr in automated),
            "",
            "</details>",
        ]
    return "\n".join(lines) + "\n"


def _command(root: Path, *argv: str) -> str:
    try:
        done = subprocess.run(argv, cwd=root, capture_output=True, text=True, check=False)
    except OSError as error:
        raise ReleaseError(f"cannot run {argv[0]}: {error.strerror or error}") from error
    if done.returncode != 0:
        raise ReleaseError(f"`{' '.join(argv)}` failed: {done.stderr.strip() or done.stdout.strip()}")
    return done.stdout


def parse_merged(log: str) -> list[PullRequest]:
    """The pull requests behind the commits of a `git log` listing, in the
    order they were opened. Each line is the author and the subject of a
    commit, separated by a tab. GitHub ends the subject of a merged pull
    request with its number, and names an app that wrote one `<app>[bot]`.
    A commit without a number was not merged from a pull request."""
    merged = []
    for line in log.splitlines():
        author, _, subject = line.partition("\t")
        if match := MERGED_SUBJECT.fullmatch(subject.strip()):
            merged.append(PullRequest(int(match.group(2)), match.group(1).strip(), author.endswith("[bot]")))
    return sorted(merged)


def _merged_since(root: Path, tag: str, tip: str) -> list[PullRequest]:
    return parse_merged(_command(root, "git", "log", "--first-parent", "--format=%an%x09%s", f"{tag}..{tip}"))


def prepare(root: Path, target: str, dry_run: bool) -> None:
    """Start the branch of the release `target` names: a part of the version
    to increment, or the version itself. Nothing is committed or pushed."""
    tip = f"origin/{BASE_BRANCH}"
    _command(root, "git", "fetch", "--tags", "origin", BASE_BRANCH)
    previous = _field(
        _parse_toml(_command(root, "git", "show", f"{tip}:{CARGO_MANIFEST}"), CARGO_MANIFEST),
        CARGO_MANIFEST, "workspace", "package", "version",
    )  # fmt: skip
    version = next_version(previous, target)
    branch = f"release/v{version}"
    title = f"chore(release): v{version}"
    if not _command(root, "git", "tag", "--list", f"v{previous}").strip():
        raise ReleaseError(f"{tip} carries {previous}, but the tag v{previous} does not exist; release it first")
    pull_requests = _merged_since(root, f"v{previous}", tip)
    if not pull_requests:
        raise ReleaseError(f"no pull request was merged into {BASE_BRANCH} since v{previous}; nothing to release")
    body = pull_request_body(version, previous, pull_requests)
    if dry_run:
        print(f"Would start {branch} from {tip}, for the pull request '{title}' with this description:\n")
        print(body, end="")
        return

    if _command(root, "git", "status", "--porcelain").strip():
        raise ReleaseError("the working tree has uncommitted changes; commit or stash them first")
    _command(root, "git", "switch", "--create", branch, tip)
    bump(root, version)
    for name in CHANGELOGS:
        language = "Japanese" if name.endswith(".ja.md") else "English"
        section = draft_section(version, pull_requests, language)
        (root / name).write_text(insert_section(_read(root, name), version, section), encoding="utf-8")
    print(f"Started {branch} with every package at {version}. Nothing is committed or pushed.\n")
    print(f"1. Rewrite the v{version} sections of {' and '.join(CHANGELOGS)} and delete their draft comments.")
    print("2. Commit and push:\n")
    print(f"       git commit --all --message {shlex.quote(title)}")
    print(f"       git push --set-upstream origin {branch}\n")
    print(f"3. Open the pull request '{title}' into {BASE_BRANCH} with this description:\n")
    print(body, end="")


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
    prepare_command = commands.add_parser("prepare", help="start the branch that prepares the next release")
    prepare_command.add_argument(
        "version", metavar="major|minor|patch|VERSION", help="the part of the version to increment, or the new version"
    )
    prepare_command.add_argument(
        "--dry-run", action="store_true", help="print the pull request description and change nothing"
    )
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
    drafts = [name for name in CHANGELOGS if (root / name).is_file() and DRAFT_MARKER in _read(root, name)]
    for name in drafts:
        print(f"{name}: a section is still the generated draft; rewrite it and delete its marker", file=sys.stderr)
    if drafts:
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
    elif arguments.command == "prepare":
        prepare(root, arguments.version, arguments.dry_run)
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
