# Releasing inka

This document describes how to cut an inka release ([日本語](RELEASING.ja.md)).

## Versioning

- Every package in the repository carries one version: the Cargo workspace (all crates), the Python projects
  (`pyproject.toml` and `crate/inka-evidence-py`), and the npm packages (`web`, `editors/vscode`, and
  `crate/inka-evidence-wasm`). `Cargo.toml`'s `[workspace.package]` version is the reference.
- Versions follow [Semantic Versioning](https://semver.org) as plain `MAJOR.MINOR.PATCH`, without pre-release or build
  suffixes. While the major version is `0`, a release that breaks compatibility bumps the minor version, and any other
  release bumps the patch version.
- The format versions (the `.inka` language, test evidence v1, and the approval hash v1) are versioned on their own and
  change only when the format does; a release that changes one says so in its notes.
- CI (`Check package versions agree` in `repository-check.yaml`) fails whenever a manifest or a lockfile disagrees with
  the workspace version.

A release publishes a [GitHub Release](https://github.com/ut-issl/inka/releases) and its tag `v<version>`. Nothing is
published to crates.io, PyPI, npm, or the VS Code Marketplace yet.

## Release procedure

### 1. Prepare the release pull request

On a new branch from `main`:

1. Bump the version of every package:

   ```console
   python3 scripts/release.py bump 0.1.0
   ```

   This rewrites the version in every manifest and lockfile and nothing else. Run
   `python3 scripts/release.py check` to confirm that they agree.
2. Write the release notes. Add a section whose heading is exactly `## v<version>` (for example `## v0.1.0`) above the
   previous release in both `CHANGELOG.md` and `CHANGELOG.ja.md`. Use `###` for subsections such as
   `### Breaking changes`, and write one sentence per line, because a GitHub Release keeps line breaks. The English
   section becomes the body of the GitHub Release, and the release workflow refuses to run unless both files have the
   section.
3. Open the pull request and merge it into `main` after CI passes.

### 2. Optional: dry run

In the Actions tab, run the `Release` workflow with `mode` left at its default `dry-run`. A dry run can start from any
branch. It runs every check of a release (so it also fails when the tag already exists) and shows the release notes in
the run summary, but creates nothing.

### 3. Run the release

Run the `Release` workflow on `main` with `mode` set to `release`, once CI has passed on the commit at the tip of
`main`. The workflow does not check CI results itself.

The workflow releases the version in `Cargo.toml` as the tag `v<version>`. Do not push the tag yourself: the workflow
creates it on the dispatched commit together with the GitHub Release. The run fails before creating anything when

- it was not started from `main`,
- some manifest or lockfile carries another version,
- the tag already exists, or
- `CHANGELOG.md` or `CHANGELOG.ja.md` has no section for the version.

### 4. Verify the release

Check that the [GitHub Release](https://github.com/ut-issl/inka/releases) has the title `v<version>` and the notes from
`CHANGELOG.md`, and that it is marked as the latest release.

## Fixing a release

A published tag is never moved. If a release turns out to be broken, fix the problem on `main` and cut a new patch
release. Edit the notes of an existing GitHub Release on GitHub directly when only the text is wrong, and update the
matching `CHANGELOG` sections in a pull request.
