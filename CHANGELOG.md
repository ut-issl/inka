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

### Calc evidence

- Calc evidence v1 is a JSON schema for the results of a calculation graph's verifications, with the sources they rest on.
- A bind cites a verification with `calc: "<path>"`, and `[evidence] calcs` in `inka.toml` lists the files to load.
- The nodes a calculation assumes become dependencies of the bind and part of what its approval covers.
- `inka review` shows a calc bind's sources, results and assumptions.
- It also shows how the assumptions changed since the last approval of the bind.
- The `inka-evidence` Python package emits it from a veriq project, with the `inka-evidence-veriq` command.

`inka-evidence-veriq` needs a veriq newer than any released one, so the `veriq` extra installs veriq from a pinned git commit.

The packages are not yet published to crates.io, PyPI, npm, or the VS Code Marketplace.
Build them from this repository.
