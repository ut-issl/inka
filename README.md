# inka

Structured argumentation with accountable approvals. Text-based, LSP-powered, built for iterative development.

## QuickStart

An inka project is a directory holding `inka.toml` and an `argument/` directory of `*.inka` files.
This repository ships two of them, so everything below runs as written from the repository root:
`tests/fixtures/aocs` (a satellite attitude-control argument) and `tests/fixtures/dogfood` (inka's
argument for itself).

You need a Rust toolchain for the commands, and Node.js for the graph viewer in step 3.

### 1. Check an argument

`check` loads a project and reports every structural problem in it — undefined references, duplicate
ids, dependency cycles, a missing objective, nodes no objective reaches. A clean project says
nothing and exits 0.

```console
cargo run -p inka-cli -- check tests/fixtures/aocs
```

Findings are printed as annotated source snippets. Errors exit 1; warnings alone still exit 0.

To get an `inka` binary on your PATH instead of typing `cargo run -p inka-cli --` every time:

```console
cargo install --locked --path crate/inka-cli
```

The rest of this section uses that binary.

### 2. Export the argument

`export` writes the argument to standard output as cytoscape.js `elements` JSON — inka's machine
interface to the graph. The viewer is one consumer of it; anything that reads JSON is another, so
the built-in UI is never the only way to read an argument.

```console
inka export tests/fixtures/aocs > aocs.elements.json
```

Each node carries its id, discharge mode, namespace, interpolated description, tags, evidence
target and deadline; each edge points from a parent to one child. The output is deterministic, so
it diffs cleanly between runs. An argument that fails `check` exports nothing.

### 3. Build the graph viewer

The viewer is a static front-end bundle under [`web/`](web/README.md). Build it once:

```console
cd web
npm install
npm run build
cd ..
```

This writes `web/dist`, which is where `serve` looks by default.

### 4. Serve the interactive graph

`serve` hosts the viewer and feeds it the argument. It is strictly read-only: nothing it serves can
change the argument, approvals, or any other state.

```console
inka serve tests/fixtures/aocs
```

Open <http://127.0.0.1:8080>. The whole argument is laid out top-down with the objectives on top;
click a node to focus it with its direct parents and children, click a namespace badge or a list
header to focus that namespace, hover a node for its full description, and press `Esc` to return to
the whole view.

Useful flags: `--port` (default 8080), `--host` (loopback by default, so an argument under review is
not published to the network), and `--assets <DIR>` if the bundle is somewhere other than
`web/dist`. Without a bundle the server still answers `GET /elements.json`, which is the same JSON
step 2 writes.

### Next

- [`docs/`](docs/en/index.md) — the concept, the specification, and the design of each output
- [`web/README.md`](web/README.md) — the viewer's contract with `inka export`, and how to develop it
- `inka <command> --help` — every flag of a command

## Pre-commit Setup

This repository uses [prek](https://prek.j178.dev), a faster drop-in replacement for [pre-commit](https://pre-commit.com).

```console
prek install --hook-type pre-commit --hook-type pre-push
```

If you prefer `pre-commit`, substitute `pre-commit` for `prek` in the command above.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <https://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <https://opensource.org/licenses/MIT>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in the work by you,
as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
