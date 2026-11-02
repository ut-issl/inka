# inka for VS Code

Minimal language support for `.inka` files: language registration, a
deliberately simple TextMate grammar, and a client for the `inka-lsp`
language server (diagnostics follow every edit, saved or not).

The grammar stays flat on purpose — rich highlighting inside fenced
descriptions is the server's job via semantic tokens, which supersede these
scopes once the server provides them.

## How to run

1. `cargo build -p inka-lsp` at the repository root, or install the binary
   somewhere on `PATH`.
2. `npm ci && npm run compile` in this directory.
3. Open this directory in VS Code and press `F5` (Run Extension), then open a
   folder containing an inka project in the development host.

When `inka-lsp` is not on `PATH`, point the `inka.serverPath` setting at the
built binary with an **absolute** path, for example
`/home/you/inka/target/debug/inka-lsp` — settings do not expand
`${workspaceFolder}`. The setting is machine-scoped, so a workspace cannot
quietly point it at a script of its own. The binary is churned by
`cargo build`, so consider adding `**/target/**` to `files.watcherExclude`
if your editor does not already exclude it.
