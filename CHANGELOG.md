# Changelog

Release notes for inka ([日本語](CHANGELOG.ja.md)).
Each `## v<version>` section becomes the body of its GitHub Release, and `CHANGELOG.ja.md` has the same section in Japanese.
GitHub Releases keep line breaks, so write one sentence per line.
See [RELEASING.md](RELEASING.md) for how a release is cut.

## v0.0.4

This release builds the graph viewer into `inka`, so `inka serve` works without downloading anything else.

### Command line (`inka`)

- `inka serve` serves the graph viewer compiled into the binary, so `--assets` is no longer needed.
- `--assets <DIR>` still serves a viewer bundle from disk, for working on the viewer itself.
- `cargo install inka-cli` embeds the viewer as well.

### Distribution

- The web viewer bundle `inka-web-<version>.tar.gz` is no longer attached to the GitHub Release.
- The npm package is staged on npm by the release workflow and made public when a maintainer approves it.

## v0.0.3

This release changes nothing in inka itself and fixes how the release workflow publishes the packages.

### Distribution

- The npm package `@inka-project/inka-evidence` is published to npm, which v0.0.2 failed to do.
- Publishing to Open VSX fails at once when the `OVSX_PAT` secret is missing, instead of waiting for a token.

## v0.0.2

This release adds `inka init` and is the first to publish the packages to their registries.

### Command line (`inka`)

- `inka init [PATH]` scaffolds a new project with `inka.toml`, an empty `values.toml`, and a starter argument.
- The new project passes `inka check` and `inka fmt --check` as it stands.
- The project name defaults to the directory's name, and `--name` overrides it.
- `--phases` sets `phases.order`, and its first entry becomes `phases.current` (`M0` by default).
- `inka init` only adds files and never overwrites an existing project.

### Distribution

- GitHub Releases now attach prebuilt binaries for Linux (musl) and macOS on x86_64 and aarch64, and Windows on x86_64.
- They also attach the VS Code extension per platform, the Python wheels and sdist, and the npm package.
- The web viewer bundle for `inka serve --assets` is attached as well.
- Every asset has a build provenance attestation and is listed in `SHA256SUMS`.
- The crates are published to crates.io, `inka-evidence` to PyPI, and `@inka-project/inka-evidence` to npm.
- The VS Code extension is published as `inka-project.inka-vscode` to the VS Code Marketplace and Open VSX.
- The VS Code extension bundles `inka-lsp`, and uses it before one on `PATH`.

### Breaking changes

- The npm package is renamed from `@ut-issl/inka-evidence` to `@inka-project/inka-evidence`.

## v0.0.1

The first release of inka, a text DSL for writing design arguments as structured, reviewable claims.
Every package in the repository is released together under this version.

### Command line (`inka`)

- `inka check` loads a project and reports every structural problem in its argument.
- `inka eval` evaluates the argument: whether it is currently valid, and if not, why not.
- `inka review` lists the approval targets, walks them interactively, or records a single decision.
- `inka fmt` rewrites every argument file in the canonical layout; `--check` only lists the files it would change.
- `inka export` writes the argument as cytoscape `elements` JSON, and `inka serve` serves the read-only graph viewer.

### Editor support

- `inka-lsp` provides diagnostics, hover, go to definition, find references, rename, and semantic tokens.
- It also provides inlay hints, approval code lenses, and document formatting.
- A VS Code extension in `editors/vscode` registers the language and starts the server.

### Test evidence

- Test evidence v1 is a JSON schema for the results of test runs that an argument can cite.
- `inka-evidence-rust` emits it from cargo-nextest runs.
- The `inka-evidence` Python package emits it, with a pytest plugin.
- The `@ut-issl/inka-evidence` npm package emits it, with a Vitest adapter.

The packages are not yet published to crates.io, PyPI, npm, or the VS Code Marketplace.
Build them from this repository.
