# Changelog

Release notes for inka ([日本語](CHANGELOG.ja.md)).
Each `## v<version>` section becomes the body of its GitHub Release, and `CHANGELOG.ja.md` has the same section in Japanese.
GitHub Releases keep line breaks, so write one sentence per line.
See [RELEASING.md](RELEASING.md) for how a release is cut.

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
