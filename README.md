# inka

Structured argumentation with accountable approvals. Text-based, LSP-powered, built for iterative development.

## Install

Every [GitHub Release](https://github.com/ut-issl/inka/releases) ships the same version of every package, for x86_64 and
aarch64 Linux, x86_64 and aarch64 macOS, and x86_64 Windows.

- **Command line.** Download `inka-v<version>-<target>.tar.gz` (`.zip` on Windows) from the release and put `inka`,
  `inka-lsp`, `inka-evidence-rust` and `inka-evidence-merge` on your `PATH`. With a Rust toolchain,
  `cargo binstall inka-cli` fetches the same archive, and `cargo install --locked inka-cli` (or `inka-lsp`,
  `inka-evidence-rust`) builds from [crates.io](https://crates.io/crates/inka-cli).
- **VS Code.** Install `inka-project.inka-vscode` from the
  [Marketplace](https://marketplace.visualstudio.com/items?itemName=inka-project.inka-vscode) or
  [Open VSX](https://open-vsx.org/extension/inka-project/inka-vscode); it bundles `inka-lsp`. The release also carries the
  `.vsix` for *Install from VSIX…*.
- **Web UI.** Built into `inka`: `inka serve` needs nothing else. `--assets <DIR>` serves a bundle you built
  yourself instead.
- **Test evidence adapters.** `pip install inka-evidence` (pytest, [PyPI](https://pypi.org/project/inka-evidence/)) and
  `npm install --save-dev @inka-project/inka-evidence` (Vitest, [npm](https://www.npmjs.com/package/@inka-project/inka-evidence));
  the Rust adapter `inka-evidence-rust` is in the command-line archive.

`SHA256SUMS` on the release lists every asset, and each asset carries a GitHub build provenance attestation
(`gh attestation verify <file> --repo ut-issl/inka`). [RELEASING.md](RELEASING.md) describes how releases are cut.

## QuickStart

An inka project is a directory holding `inka.toml` and an `argument/` directory of `*.inka` files.
This repository ships two of them, so everything below runs as written from the repository root:
`tests/fixtures/aocs` (a satellite attitude-control argument) and `tests/fixtures/dogfood` (inka's
argument for itself).

You need a Rust toolchain for the commands, and Node.js for the Web UI in step 3.

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
interface to the graph. Anything that reads JSON can consume it, so the built-in Web UI is never the
only way to read an argument.

```console
inka export tests/fixtures/aocs > aocs.elements.json
```

Each node carries its id, discharge mode, namespace, interpolated description, tags, whether it is
an objective root, evidence target, deadline and attached comments; each edge points from a parent
to one child and carries that child entry's comment. The output is deterministic, so
it diffs cleanly between runs. An argument that fails `check` exports nothing.

### 3. Build the Web UI

The Web UI is a static front-end bundle under [`web/`](web/README.md) that `inka` embeds at compile time. Build it
once, then build `inka` again so it picks the bundle up:

```console
cd web
npm install
npm run build
cd ..
cargo build -p inka-cli
```

This writes `crate/inka-cli/web-dist`, which the crate's build script compiles into the binary (so run
`cargo install --locked --path crate/inka-cli` after this step, not before, to get an `inka` with the Web UI). A
release binary already carries it.

### 4. Review and explore in the browser

`serve` hosts the Web UI and feeds it the argument. Started like this it is read-only: nothing it serves
can change the argument, approvals, or any other state. Every page load reads the project again, so an
edit or a new approval shows on reload without restarting the server; a project that fails `check`
is shown as its findings until it is fixed.

```console
inka serve tests/fixtures/aocs
```

Open <http://127.0.0.1:8080>. There are two surfaces, and a sidebar on both that lists the review
queue and every node and searches them by id and description:

- **Review** walks the queue `inka review --list` prints, one target at a time. Each target is shown
  as its local view — its description, its evidence reference, its direct children's descriptions and
  its deadline, each with the comments attached — with descriptions rendered as Markdown.
- **Graph** shows the whole argument as cards carrying each node's mode, id, status and description.
  It opens with the objectives and their direct children; expand a card to go deeper, click one to
  read its local view beside the graph, and press `Esc` to close it.

Every node shows the verdict `inka eval` gives it, and the count `established n/m` is always in sight.

To review in the browser, start it with an approver:

```console
inka serve tests/fixtures/dogfood --as iwata
```

The review surface then offers approve and decline under each target that can take a decision, and a
decision appends the same record `inka review --as iwata` would, to `approvals/iwata.jsonl`. That is
the only thing the server can change. Who decisions are recorded as is fixed when the server starts
(`--key-id` sets the key id, as for `inka review`); a recording server listens on a loopback address
only and accepts a decision only from its own pages; and a decision is recorded only if the target
still has the content that was on screen.

Useful flags: `--port` (default 8080), `--host` (loopback by default, so an argument under review is
not published to the network), and `--assets <DIR>` to serve a bundle from disk instead of the
embedded one. The cytoscape JSON of step 2 comes from `inka export` only; the server does not serve it.

### 5. Start your own project

`init` lays out a new project: an `inka.toml`, an empty `values.toml`, and a starter argument in
`argument/objective.inka` — an objective resting on one fiat, which `check` and `fmt --check` accept
as it stands.

```console
inka init my-argument --phases BBM,EM,FM
```

The directory is created if it is missing. The name defaults to the directory's name (`--name`
overrides it), and the first of `--phases` becomes the current phase (`M0` alone by default).
`init` only ever adds files: it fails without writing anything when any file it would create already
exists.

### 6. Evaluate and approve

`check` asks whether the argument is well-formed; `eval` asks whether it currently holds. It
reports how many nodes are established and, for each one that is not, why not. It exits 0 only when
every node is established, and `--json` writes the same result for other tools.

```console
inka eval my-argument
```

A fresh project establishes nothing, because no one has approved it yet. `review` shows the targets
waiting for a decision and records decisions on them:

```console
inka review --path my-argument --as alice --list
inka review --path my-argument --as alice premise --approve
inka review --path my-argument --as alice goal --decline --comment "state a real objective"
```

Without `--list` or a target, `review` walks the queue interactively. The queue runs from the leaves
up, one subtree at a time, so a block comes after everything below it. A block that cannot be
decided on yet — a bind with no evidence, say — is never offered, so its parent can come up before
it; the parent shows each direct child with the status `inka eval` gives it, and the reasons when the
child is blamed. Every decision is appended to
`approvals/<approver>.jsonl` and nothing is ever rewritten. An approval covers the content it was
given for, so editing a block makes its approval stale, and its parents' too, since they rest on its
contract. Each block with a stale approval goes back in the queue.

### 7. Format and edit

`fmt` rewrites every argument file in the canonical layout, and `fmt --check` lists the files it
would change and fails if there is one, which suits CI.

```console
inka fmt my-argument
```

For editing, the [VS Code extension](editors/vscode/README.md) runs `inka-lsp`. It gives diagnostics
as you type, completion, hover, go to definition, find references, rename, inlay hints, approval code
lenses, and formatting with exactly the output of `inka fmt`.

### Next

- [`docs/`](docs/en/index.md) — the concept, the specification, and the design of each output
- [`web/README.md`](web/README.md) — the Web UI's contract with `inka serve`, and how to develop it
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
