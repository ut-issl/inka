# inka — Formatting

The canonical layout of `.inka` files, produced by `inka fmt` and by the
language server's formatting request. The [specification](spec.md) defines what
a file means; this page defines only how it is laid out. Japanese version:
[日本語版](https://www.space.t.u-tokyo.ac.jp/inka/ja/design/formatting/).

## Example

````text
// File comment.

// Comment on the block.
structure pointing_ok #mission #aocs {
    ```
    In steady-state observation mode, the pointing control error is at most {$pointing_accuracy}
    ```
    // Comment on the children property.
    children:
        // Comment on this child.
        attitude_knowledge_ok,
        control_law_ok,
        mass_properties_valid
}

fiat mass_properties_valid {
    ```
    The mass properties used by the control design match the flight model
    ```
    // Awaiting the mass-properties test.
    until: M3
    promote: bind
}

structure single {
    ```
    ...
    ```
    children: only_child
}
````

## Guarantees

- **Meaning is preserved.** Formatting never changes any view hash, and the
  formatted file parses to the same syntax tree apart from source positions and
  the indentation of fence content.
- **Idempotent.** Formatting a formatted file changes nothing.
- **Errors are not formatted.** A file with a parse error is left untouched and
  reported. Comments never cause one.

## Layout

- **File.** UTF-8, LF line endings, no trailing whitespace, exactly one final
  newline. The file comment comes first, followed by one blank line. Blocks
  keep their source order and are separated by exactly one blank line.
- **Indentation.** Four spaces per level; never tabs. Block headers and their
  comments sit at level 0, everything inside a block at level 1, and the
  entries of a multi-line `children:` list at level 2.
- **Block header.** `<mode> <id> #tag ... {` with single spaces; tags keep their
  source order.
- **Block body.** The description, then `children:` / `test:` / `veriq:`, then
  `until:`, then `promote:`. One property per line, written `key: value`. No
  blank lines inside a block.
- **Description.** The fences sit at level 1. The opening fence keeps its
  length and info string, and the closing fence takes the opening fence's
  length. The content loses its common leading-space indent and every
  non-blank line is re-indented to level 1; blank lines are empty. When this
  would change the view hash — a multi-line value interpolated into the fence
  can make it do so — or the content names an undefined value, so the view
  hash cannot be computed, the content is left byte-for-byte as written.
- **Children.** A single child without a comment stays on the property line:
  `children: only_child`. Otherwise `children:` stands alone and each entry
  follows on its own line at level 2, separated by a `,` at the end of the line,
  with no comma after the last entry. Entries keep their source order.
- **Comments.** Every comment is written on its own lines directly above its
  anchor, at the anchor's level: block comments above the block header, property
  comments above the property, child-entry comments above the entry, and the
  file comment at the top of the file. Each text line becomes `// <line>`, or
  `//` when the line is empty. Trailing and dangling comments therefore move
  above their anchor; their text and attachment do not change.
- **Line width.** Lines are never wrapped.

## Commands

- `inka fmt [PATH]` rewrites every `*.inka` file of the project at `PATH`
  (default: the current directory) in place.
- `inka fmt --check [PATH]` writes nothing; it lists every file that would
  change and exits non-zero if there is one.
- Both read the project's manifest and values, which decide whether a fence
  may be re-indented; when they cannot be loaded, nothing is formatted. A file
  with a parse error is reported as `inka check` reports it and left
  untouched, the other files are still formatted or listed, and the command
  exits non-zero.
- The language server answers `textDocument/formatting` with the same output
  for the open document.
