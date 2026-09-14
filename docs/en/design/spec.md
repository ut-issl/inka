# inka — Decided Specification

Test evidence uses `test:`. The [integration design](test-evidence-integration.md) defines automatic
collection from independently executed tests.

Everything on this page is settled. Hash and record layouts become frozen the
moment their golden vectors land (append-only from then on; incompatible
changes ship as a parallel v2). Japanese version: [日本語版](https://www.space.t.u-tokyo.ac.jp/inka/ja/design/spec/).

## DSL syntax

````text
structure <id> #tag1 #tag2 {
  ```
  description (markdown allowed; {$var} interpolation; fence ``` or longer)
  ```
  children: <id>, <id>, ...
}

bind <id> {
  ```...```
  test: "<test-target>"   // or veriq: "<module>.<name>"
}

fiat <id> {
  ```...```
  until: <phase>
  promote: bind
  reason: "<short rationale>"
}
````

- Blocks: `objective` / `structure` / `bind` / `fiat` + id + optional tags +
  `{}`. Indentation is never structurally significant.
- Identifiers (block ids, references, phase names, tag names) match
  `[A-Za-z_][A-Za-z0-9_-]*`; namespace-qualified references join identifiers
  with `::`.
- Exactly one fenced description per block. Fence info strings are accepted and
  reserved. Inside fences: natural language only; outside: keywords only.
- Mode-required properties, enforced at parse time: `structure`/`objective` →
  `children:` required; `bind` → exactly one of `test:` / `veriq:` required;
  `fiat` → neither allowed.
- Annotations form a group anchored on `until:` — `reason:` and `promote:` may
  only appear together with `until:`. `until: <phase>` and `reason: "<short
  text>"` may appear on any block. `promote:` names the successor mode and is
  optional (an `until:` without `promote:` is plain expiry): on fiat the
  target is `structure` or `bind`; on the objective it is `structure` only.
  structure and bind take no `promote:` — announcing a mode change there is
  not supported. The `reason`, `test:`, and `veriq:` properties take a
  double-quoted, single-line string value with no escape sequences — the only
  quoted values in the language. A `test:` / `veriq:` value is the opaque
  evidence target, which `check` does not interpret. Descriptions never use
  quotes.
- `//` line comments; dropped at the lexer, never hashed.
- No escape for `{$` — only the exact pattern `{$identifier}` is interpolation;
  everything else is literal.
- Property order is free; duplicate properties are an error.
- Tags are free vocabulary; `check` says nothing about them (typo detection
  belongs to policy selectors: a selector matching no existing tag warns).
- Namespaces: `::` separator, assigned by directory under `argument/`
  (`argument/power/x.inka` → `power::<id>`); root-level files are unnamespaced;
  same-namespace references may omit the prefix; `::` cannot be written inside
  an ID; policy selectors use the same notation (`power::*`); `veriq:`
  references keep Python's dotted `module.function` names.
- `test:` / `veriq:` references get a format check only at `check` time;
  existence and pass/fail are eval's job.

## Project layout

- `inka.toml`: `[project] name`; `[phases] order` (total order) + `current`
  (validated against order); `[values] files`. Unknown keys are rejected.
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
  (e.g. `["promote:bind", "reason:awaiting the mass-properties test",
  "until:M3"]`; the `reason` value is the string content without quotes).
- Framing: every field is u64-LE length-prefixed; lists carry a u64-LE element
  count. Domain tags: `inka:objective:v1` / `inka:structure:v1` /
  `inka:fiat:v1` / `inka:bind:v1`.
  - objective = tag ‖ desc ‖ annotations ‖ children descs (sorted bytewise)
  - structure = tag ‖ desc ‖ annotations ‖ children descs (sorted bytewise)
  - fiat = tag ‖ desc ‖ annotations
  - bind = tag ‖ desc ‖ annotations ‖ interface_hash (raw 32 bytes) ‖
    assumption list (empty until assumption aggregation lands; the slot exists
    so filling it later is not a layout change)
- Excluded from hashes: IDs, tags, comments, `test:` / `veriq:` reference strings.
  Included: deadline annotations. Interpolation happens before normalization
  and hashing — hashes never see placeholders.

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
  evidence match; objective = satisfied objective approval ∧ all children
  established (each root's wording is agreed inside inka like any other
  contract).
- blame lists only nodes whose failure cause is their own (missing / stale /
  declined / expired / evidence mismatch), not nodes failing solely through
  their children.

## Test evidence provider

The [semantic contract](ci-evidence.md) and [integration contract](test-evidence-integration.md) define
source-owned independent test execution, automatic publication and offline evaluation of imported content.
Evaluation matches delivered content. Environment metadata is optional.

## Repository conventions

- Comments in code, fixtures, and config are English. Docs are English-canonical
  (`docs/en/`) with a Japanese translation alongside (`docs/ja/`). Fixture contracts and values are
  English.
- Documents describe the current design only — no history, no "formerly X".
- Golden-vector directories are append-only, guarded in CI.
