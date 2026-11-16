# Releasing inka

This document describes how to cut an inka release ([日本語](RELEASING.ja.md)).

## Versioning

- Every package in the repository carries one version: the Cargo workspace (all crates), the Python projects
  (`pyproject.toml` and `crate/inka-evidence-py`), and the npm packages (`web`, `editors/vscode`, and
  `crate/inka-evidence-wasm`). `Cargo.toml`'s `[workspace.package]` version is the reference, and the
  `[workspace.dependencies]` entries that point at a workspace member by `path` carry it as their version requirement,
  because `cargo publish` replaces the path with that requirement.
- Versions follow [Semantic Versioning](https://semver.org) as plain `MAJOR.MINOR.PATCH`, without pre-release or build
  suffixes. While the major version is `0`, a release that breaks compatibility bumps the minor version, and any other
  release bumps the patch version.
- The format versions (the `.inka` language, test evidence v1, and the approval hash v1) are versioned on their own and
  change only when the format does; a release that changes one says so in its notes.
- CI (`Check package versions agree` in `repository-check.yaml`) fails whenever a manifest or a lockfile disagrees with
  the workspace version.

## What a release publishes

A release is the tag `v<version>` and a [GitHub Release](https://github.com/ut-issl/inka/releases) on it, with these
assets attached (each with a build provenance attestation, and all listed in `SHA256SUMS`). The binaries are
`inka` (with the graph viewer built in, so `inka serve` needs nothing else), `inka-lsp`, `inka-evidence-rust` and
`inka-evidence-merge`.

| Asset | Contents |
| --- | --- |
| `inka-v<version>-<target>.tar.gz` (`.zip` on Windows) | the four binaries for one platform |
| `inka-vscode-v<version>-<platform>.vsix` | the VS Code extension with `inka-lsp` bundled, one per platform |
| `inka_evidence-<version>-*.whl`, `inka_evidence-<version>.tar.gz` | the Python package `inka-evidence` and its sdist |
| `inka-project-inka-evidence-<version>.tgz` | the npm package `@inka-project/inka-evidence` |

The platforms are x86_64 and aarch64 Linux (static musl binaries), x86_64 and aarch64 macOS, and x86_64 Windows.

The same assets are then published to the registries, each from its own GitHub environment:

| Registry | Packages | Environment | Credential |
| --- | --- | --- | --- |
| [crates.io](https://crates.io) | `inka-core`, `inka-evidence`, `inka-cli`, `inka-lsp`, `inka-evidence-rust` | `crates-io` | Trusted Publishing (OIDC) |
| [PyPI](https://pypi.org) | `inka-evidence` | `pypi` | Trusted Publishing (OIDC) |
| [npm](https://www.npmjs.com) | `@inka-project/inka-evidence` | `npm` | Trusted Publishing (OIDC), staged |
| [VS Code Marketplace](https://marketplace.visualstudio.com) | `inka-project.inka-vscode` | `vscode-marketplace` | Trusted Publishing (OIDC) |
| [Open VSX](https://open-vsx.org) | `inka-project.inka-vscode` | `open-vsx` | `OVSX_PAT` secret |

The binding crates `inka-evidence-py` and `inka-evidence-wasm` are not published to crates.io (`publish = false`);
they reach users as the Python and npm packages.

The published names (`inka-project` as the npm scope, the Marketplace publisher and the Open VSX namespace) are
deliberately independent of the repository's GitHub organization: the trusted publishers are bound to the repository,
so moving the repository means updating them, never renaming a package.

## One-time setup

Each registry is set up once by whoever owns the account there. Until a registry is set up, its publish job fails
while the rest of the release goes through; see [Fixing a release](#fixing-a-release).

1. **GitHub environments.** In Settings → Environments, create `crates-io`, `pypi`, `npm`, `vscode-marketplace` and
   `open-vsx`. Optionally require reviewers on them, so a release pauses before each registry. Trusted publishers below
   are bound to these environment names and to the workflow file `release.yaml`.
2. **crates.io.** The first version of each crate must be published by hand with an API token of the owner
   (`cargo publish -p <crate>` in dependency order: `inka-evidence`, `inka-core`, `inka-evidence-rust`, `inka-lsp`,
   `inka-cli`), because Trusted Publishing can only be configured on a crate that exists. Then, on each crate's
   Settings page, add a Trusted Publisher for repository `ut-issl/inka`, workflow `release.yaml`, environment
   `crates-io`. Consider adding a GitHub team of the organization as an owner, so ownership does not rest on one person.
3. **PyPI.** Add a *pending* Trusted Publisher for the project `inka-evidence` (owner `ut-issl`, repository `inka`,
   workflow `release.yaml`, environment `pypi`). The first release creates the project.
4. **npm.** Create the `inka-project` organization if it does not exist. The first version of
   `@inka-project/inka-evidence` must be published by hand (`npm publish` of the tarball from the GitHub Release, with
   an account that may create packages in the scope), because a trusted publisher can only be configured on an
   existing package. Then, in the package's Settings, add a Trusted Publisher for `ut-issl/inka`, workflow
   `release.yaml`, environment `npm`, with `npm stage publish` as its only allowed action: the workflow stages each
   version and a maintainer makes it public (step 4 of the procedure), so no automation can publish on its own.
   Set *Publishing access* to require two-factor authentication and disallow tokens.
5. **VS Code Marketplace.** Create the publisher `inka-project` in the
   [Marketplace publisher management](https://marketplace.visualstudio.com/manage) (a personal Microsoft account
   suffices; no Azure DevOps organization or token is needed). On the publisher's *Trusted Publishing* tab, create a
   policy with repository owner `ut-issl`, repository `inka`, workflow `release.yaml`, environment
   `vscode-marketplace`. Add other maintainers under *Members*, so the publisher does not rest on one account.
6. **Open VSX.** Sign in to [open-vsx.org](https://open-vsx.org) with GitHub and create the namespace `inka-project`, claiming
   its ownership through the [publisher agreement](https://github.com/EclipseFdn/open-vsx.org/wiki/Namespace-Access).
   Then create an access token and store it as the secret `OVSX_PAT` of the `open-vsx` environment.

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
   section becomes the body of the GitHub Release, and the release workflow refuses to release unless both files have
   the section.
3. Open the pull request and merge it into `main` after CI passes.

### 2. Optional: dry run

In the Actions tab, run the `Release` workflow with `mode` left at its default `dry-run`. A dry run can start from any
branch. It runs the checks of a release, builds every asset exactly as a release would, and lists them with their
sizes in the run summary, but creates and publishes nothing. An existing tag or a missing CHANGELOG section is reported
as a warning instead of failing, so the build can be rehearsed on a branch whose version is already released. Each
asset can be downloaded from the run as a workflow artifact.

### 3. Run the release

Run the `Release` workflow on `main` with `mode` set to `release`, once CI has passed on the commit at the tip of
`main`. The workflow does not check CI results itself.

The workflow releases the version in `Cargo.toml` as the tag `v<version>`. Do not push the tag yourself: the workflow
creates it on the dispatched commit together with the GitHub Release. The run fails before creating anything when

- it was not started from `main`,
- some manifest or lockfile carries another version,
- the tag already exists,
- `CHANGELOG.md` or `CHANGELOG.ja.md` has no section for the version, or
- any asset fails to build.

Once the GitHub Release exists, the registry jobs run in parallel. One registry refusing does not stop the others.

### 4. Verify the release

Check that the [GitHub Release](https://github.com/ut-issl/inka/releases) has the title `v<version>`, the notes from
`CHANGELOG.md`, every asset of the table above, and that it is marked as the latest release. Then check that the
version appears on each registry.

On npm the version is staged, not yet public. A maintainer with 2FA on their npm account approves it, from the
*Staged packages* tab of the package on npmjs.com or from a machine where they are logged in:

```console
npm stage list @inka-project/inka-evidence
npm stage view <stage-id>       # or `npm stage download <stage-id>` to inspect the tarball
npm stage approve <stage-id>    # `npm stage reject <stage-id>` discards it instead
```

Until it is approved, `npm install @inka-project/inka-evidence@<version>` does not resolve.

## Fixing a release

A published tag is never moved. If a release turns out to be broken, fix the problem on `main` and cut a new patch
release. Edit the notes of an existing GitHub Release on GitHub directly when only the text is wrong, and update the
matching `CHANGELOG` sections in a pull request.

If a registry job failed (a trusted publisher not yet configured, an expired Open VSX token, a registry outage), fix
the cause and use *Re-run failed jobs* on the same workflow run: the GitHub Release and the other registries are left
as they are, and the failed job publishes the assets that run built. Publishing to a registry cannot be undone, so
never re-run the whole workflow for a version that is already out.
