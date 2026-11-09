# inka for VS Code

Minimal language support for `.inka` files: language registration, a
deliberately simple TextMate grammar, and a client for the `inka-lsp`
language server. The server provides:

- diagnostics that follow every edit, saved or not;
- hover, go to definition, find references, and rename (which also rewrites
  approval record targets, so renaming invalidates nothing);
- semantic tokens — ids read as type names (teal in the default dark theme),
  and the CommonMark inside fenced descriptions is highlighted the way the
  current theme styles Markdown;
- inlay hints showing each child's first contract line after its
  `children:` entry, with the whole contract as a tooltip. VS Code caps the
  combined hint length per line and drops hints past the cap, so this
  extension sets `editor.inlayHints.maximumLength` to `0` (no cap) for inka
  files only — override it under `"[inka]"` in your settings if you prefer;
- read-only code lenses with every block's approval state (established, or
  the reason it is not), refreshed when approval records change on disk;
- document formatting (Format Document) with exactly the output of
  `inka fmt`; a document that does not parse, or whose project's manifest or
  values cannot be loaded, is left as it is. Formatting on save is not turned
  on for you — enable `editor.formatOnSave` under `"[inka]"` if you want it.

The grammar stays flat on purpose — rich highlighting inside fenced
descriptions is the server's job via semantic tokens, which supersede these
scopes once the server provides them. Beyond keywords and ids it only colors
the phase after `until:` (as a function name: pale yellow in the default dark
theme) and the mode after `promote:` (like the mode keyword opening a block).

## How to run

1. `cargo build -p inka-lsp` at the repository root, or install the binary
   somewhere on `PATH`.
2. `npm ci` in this directory.
3. Open **this directory** (not the repository root) as the folder in
   VS Code — e.g. `code editors/vscode` from a terminal.
4. Press `F5` (or Run → Start Debugging). The bundled launch configuration
   compiles the extension and opens a second VS Code window titled
   **[Extension Development Host]** with the extension loaded. Only that
   second window has inka support; the first one is just the debugger.
5. In the Extension Development Host window, File → Open Folder… and pick a
   folder containing an inka project (any ancestor works — the server finds
   the nearest `inka.toml` per file), then open a `.inka` file. Break a
   `children:` reference to see a diagnostic appear before saving.

To stop, close the Extension Development Host window or press the red stop
button in the first window.

When `inka-lsp` is not on `PATH`, point the `inka.serverPath` setting at the
built binary with an **absolute** path, for example
`/home/you/inka/target/debug/inka-lsp` — settings do not expand
`${workspaceFolder}`. The setting is machine-scoped, so a workspace cannot
quietly point it at a script of its own. The binary is churned by
`cargo build`, so consider adding `**/target/**` to `files.watcherExclude`
if your editor does not already exclude it.
