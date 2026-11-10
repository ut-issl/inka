# inka — Decided Specification

Test evidence uses `test:`; calc evidence (calculation graphs such as veriq's) uses `calc:`. The
[test evidence contracts](test-evidence.md) and the [calc evidence contracts](calc-evidence.md) define the
responsibility boundaries and the evidence JSON semantics.

Everything on this page is settled. Hash and record layouts become frozen the
moment their golden vectors land (append-only from then on; incompatible
changes ship as a parallel v2). Japanese version: [日本語版](https://www.space.t.u-tokyo.ac.jp/inka/ja/design/spec/).

## DSL syntax

````text
// Comment on the block.
structure <id> #tag1 #tag2 {
    ```
    description (markdown allowed; {$var} interpolation; fence ``` or longer)
    ```
    children:
        <id>,
        // Comment on this child.
        <id>
}

bind <id> {
    ```...```
    test: "<test-id>"
}

bind <id> {
    ```...```
    calc: "<Scope>::?<verification>"
}

fiat <id> {
    ```...```
    // Short rationale for the deadline.
    until: <phase>
    promote: bind
}
````

The examples use the layout the [formatter](formatting.md) produces; the parser
itself accepts any layout.

- Blocks: `objective` / `structure` / `bind` / `fiat` + id + optional tags +
  `{}`. Indentation is never structurally significant.
- Identifiers (block ids, references, phase names, tag names) match
  `[A-Za-z_][A-Za-z0-9_-]*`; namespace-qualified references join identifiers
  with `::`.
- Exactly one fenced description per block. Fence info strings are accepted and
  reserved. Inside fences: natural language only; outside: keywords only.
- Mode-required properties, enforced at parse time: `structure`/`objective` →
  `children:` required; `bind` → exactly one of `test:` / `calc:` required;
  `fiat` → neither allowed.
- Annotations form a group anchored on `until:` — `promote:` may only appear
  together with `until:`. `until: <phase>` may appear on any block; `check`
  rejects an `until:` phase that `phases.order` does not list. `promote:` names
  the successor mode and is optional (an `until:` without `promote:` is plain
  expiry): on fiat the target is `structure` or `bind`; on the objective it is
  `structure` only. structure and bind take no `promote:` — announcing a mode
  change there is not supported. The rationale for a deadline is written as a
  [comment](#comments) on `until:`. The `test:` and `calc:` properties take a
  double-quoted, single-line string value with no escape sequences — the only
  quoted values in the language. A `test:` / `calc:` value is the opaque
  evidence target, which `check` does not interpret: a test-id, or the target
  path in the producer's calculation graph (for veriq, `Scope::?name`).
  Descriptions never use quotes.
- `//` line comments are part of the syntax tree; see [Comments](#comments).
- No escape for `{$` — only the exact pattern `{$identifier}` is interpolation;
  everything else is literal.
- Property order is free; duplicate properties are an error.
- Tags are free vocabulary; `check` says nothing about them (typo detection
  belongs to policy selectors: a selector matching no existing tag warns).
- Namespaces: `::` separator, assigned by directory under `argument/`
  (`argument/power/x.inka` → `power::<id>`); root-level files are unnamespaced;
  same-namespace references may omit the prefix; `::` cannot be written inside
  an ID; policy selectors use the same notation (`power::*`); `calc:`
  references are producer paths, not inka IDs, and are never namespaced by
  inka.
- `test:` / `calc:` references get a format check only at `check` time;
  existence and pass/fail are eval's job. Calc evidence also carries
  structure: the argument nodes it assumes. That part is read at load time,
  so `check` reports an assumption ID that names no node, and a dependency
  cycle through an assumption edge, as errors (see
  [calc evidence](calc-evidence.md#load-time-structure)).

## Comments

A comment is a note on the element it is attached to. Comments are part of the
syntax tree, are presented with their element at review, and — except file
comments — are contract content that enters the view hash. A comment is never
a syntax error: every `//` outside a fence attaches to exactly one place.

- **Anchors.** A comment attaches to a block, to one property of a block
  (`children`, `test`, `calc`, `until`, `promote`), to one entry of a
  `children:` list, or to the file. The description fence is not an anchor.
- **Text.** A comment runs from `//` to the end of the line. Its text drops the
  `//`, then one following space if present, then trailing whitespace, and is
  NFC-normalized. `//` inside a fence is description text, not a comment.
- **Attachment.** Rules apply in order:
  1. *File comment.* The run of comment lines at the very top of the file
     attaches to the file when a blank line follows it.
  2. *Trailing.* A comment that follows a token on the same line attaches to
     the innermost anchor that token belongs to: a block header or its `{` →
     the block; `children:` → the property; a child entry and its `,` → that
     entry; `until: M3` → the property.
  3. *Leading.* A comment on its own line attaches to the anchor that the
     next token after it begins, blank lines notwithstanding. When that token
     begins no anchor, a comment inside a property — between its name and the
     end of its value — attaches to the property.
  4. *Dangling.* A comment with no anchor to attach to under rules 2–3 — before
     a description fence, inside a block header, before a closing `}` —
     attaches to the enclosing block. At the top level it attaches to the next
     block, or to the file after the last block.
- **Merging.** Several comments on one anchor join into a single text, one
  source line per text line, in source order. Blank lines between them are not
  kept, and leading and trailing empty lines are dropped. An anchor whose
  comment lines are all empty carries no comment.
- **No interpolation.** `{$var}` inside a comment is literal text.
- **Presentation.** `inka review` and the exported view show each comment with
  its anchor. File comments are not shown: they belong to no node.

## Project layout

- `inka.toml`: `[project] name`; `[phases] order` (total order) + `current`
  (validated against order); `[values] files`; `[evidence] tests` / `calcs`
  (test and calc evidence JSON files, paths relative to the manifest).
  Unknown keys are rejected.
- The argument is read by convention: every `*.inka` under `argument/`.
- Values files are flat TOML maps; keys match the identifier grammar, so every
  key is referenceable as `{$key}`. A string value is taken verbatim; any
  other TOML value — numbers, booleans, arrays, tables (e.g. a vector
  requirement) — is rendered with its TOML spelling. No recursive expansion
  (one pass); key collisions across files are an error; an undefined `{$var}`
  is an error.

## Hashing (v1)

- Normalization of a rendered description, in order: NFC; CRLF/CR→LF; strip
  per-line trailing whitespace; drop leading/trailing blank lines; strip common
  space indent; join with `\n`. Idempotent. Internal newlines and spacing are
  contract content (near-verbatim; no markdown-AST normalization).
- Annotation canonical form: `key:value` strings sorted by key
  (e.g. `["promote:bind", "until:M3"]`).
- Comment canonical form: one `anchor:text` string per anchor of the node that
  carries a comment, sorted bytewise, where the anchor is `block` or the
  property name (`children` / `test` / `calc` / `until` / `promote`) and the
  text is the merged comment text (e.g. `["block:shared with momentum_ok",
  "until:awaiting the mass-properties test"]`). Child-entry comments are not
  in this list; they travel with the child entry.
- Child entry: the child's normalized description followed by the comment on
  that entry (empty when there is none), each as a field.
- Framing: every field is u64-LE length-prefixed; lists carry a u64-LE element
  count. Domain tags: `inka:objective:v1` / `inka:structure:v1` /
  `inka:fiat:v1` / `inka:bind:v1`.
  - objective = tag ‖ desc ‖ annotations ‖ comments ‖ child entries (sorted
    by description, then by comment, bytewise)
  - structure = tag ‖ desc ‖ annotations ‖ comments ‖ child entries (sorted
    by description, then by comment, bytewise)
  - fiat = tag ‖ desc ‖ annotations ‖ comments
  - bind = tag ‖ desc ‖ annotations ‖ comments ‖ interface_hash (raw 32 bytes)
    ‖ assumption list (the normalized descriptions of the nodes the evidence
    assumes, sorted bytewise; empty for a test bind)
- Interface hashes have their own domain tags: `inka:test-evidence:v1`
  ([test evidence](test-evidence.md#content-hashing-and-reapproval)) and
  `inka:calc-evidence:v1`
  ([calc evidence](calc-evidence.md#content-hashing-and-reapproval)).
- Excluded from hashes: IDs, tags, file comments, `test:` / `calc:` reference
  strings. Included: deadline annotations and every other comment.
  Interpolation happens before normalization and hashing — hashes never see
  placeholders.

## Approval records (v1)

- Fields: `v` (=1), `target` (`"<kind>:<id>"`, kind ∈ objective | structure |
  bind | fiat), `view_hash` (`"sha256:<hex>"`),
  `label` (approve | decline), `comment` (required for decline), `approver`,
  `key_id`, `timestamp` (RFC3339 UTC; audit only — eval never reads it),
  `phase` (current phase at approval time), `until` (approver-side reservation),
  `policy_hash`, `sig` (reserved; empty until signatures land).
- Canonical signing bytes: sorted-key compact JSON of the record minus `sig`;
  absent optional fields serialize as `null`.
- jsonl, append-only, one file per approver (`approvals/<name>.jsonl`);
  a malformed line fails loudly with file and line; unknown fields are rejected.
- `approvals/snapshots/<hex>.json`: what a calc bind's approval was shown,
  content-addressed by the approved view hash; a presentation aid `review`
  writes and diffs against, never read by `eval` (see
  [calc evidence](calc-evidence.md#approval-snapshots-and-diffs)).

## Evaluation semantics

- A record counts iff: its target and view_hash match the current values; no
  decline exists on the same hash (a decline is a veto regardless of quorum);
  and the node's `until:` has not expired (compared via the declared phase
  order). **Expiry is hard**: once the current phase is past `until:`, no
  approval of that node counts — including approvals granted after the
  deadline. An `until:` is a jointly-approved decline placed toward the future;
  lifting it requires editing the annotation, which changes the contract and
  demands fresh approval.
- established: fiat = satisfied fiat approval; structure = satisfied structure
  approval ∧ all children established; bind = satisfied bind approval ∧
  evidence match ∧ all assumed nodes established (a test bind assumes
  nothing); objective = satisfied objective approval ∧ all children
  established (each root's wording is agreed inside inka like any other
  contract).
- blame lists only nodes whose failure cause is their own (missing / stale /
  declined / expired / evidence mismatch / unestablished assumption), not
  nodes failing solely through their children.

## Test evidence provider

The [test evidence contracts](test-evidence.md) define the responsibility boundary: the source
repository executes tests and emits evidence JSON, capsula stores and delivers it, and inka evaluates
the evidence JSON at a configured path offline. Worktree file matching and mandatory environment
hashes are not adopted. The evidence JSON carries the reviewable contract body and the corresponding
execution results, and follows `schemas/test-evidence-v1.schema.json`.

## Calc evidence provider

The [calc evidence contracts](calc-evidence.md) define the responsibility boundary for evidence drawn
from a calculation graph (veriq first): the source repository evaluates the graph and an adapter emits
evidence JSON, capsula stores and delivers it, and inka evaluates it offline. Per target the JSON carries
the reviewable sources of the whole definition-level upstream subgraph, the argument nodes it assumes, the
required leaf results and the producer's validity, and follows `schemas/calc-evidence-v1.schema.json`.
Rust never parses Python — the JSON is the entire seam.

## Repository conventions

- Comments in code, fixtures, and config are English. Docs are English-canonical
  (`docs/en/`) with a Japanese translation alongside (`docs/ja/`). Fixture contracts and values are
  English.
- Documents describe the current design only — no history, no "formerly X".
- Golden-vector directories are append-only, guarded in CI.
