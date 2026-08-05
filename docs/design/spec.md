# inka — Decided Specification

Everything on this page is settled. Hash and record layouts become frozen the
moment their golden vectors land (append-only from then on; incompatible
changes ship as a parallel v2). Japanese version: [spec_ja.md](spec_ja.md).

## DSL syntax

```
structure <id> #tag1 #tag2 {
  ```
  description (markdown allowed; {$var} interpolation; fence ``` or longer)
  ```
  children: <id>, <id>, ...
}

bind <id> {
  ```...```
  source: veriq:<module>.<name>   // or ci:<test-target>
}

fiat <id> {
  ```...```
  until: <phase>
  promote: bind
  reason: "<short rationale>"
}
```

- Blocks: `objective` / `structure` / `bind` / `fiat` + id + optional tags +
  `{}`. Indentation is never structurally significant.
- Exactly one fenced description per block. Fence info strings are accepted and
  reserved. Inside fences: natural language only; outside: keywords only.
- Mode-required properties, enforced at parse time: `structure`/`objective` →
  `children:` required; `bind` → `source:` required; `fiat` → neither allowed.
- Annotations form a group anchored on `until:` — `reason:` and `promote:` may
  only appear together with `until:`. `until: <phase>` and `reason: "<short
  text>"` may appear on any block. `promote:` names the successor mode and is
  optional (an `until:` without `promote:` is plain expiry): on fiat the
  target is `structure` or `bind`; on the objective it is `structure` only.
  structure and bind take no `promote:` — announcing a mode change there is
  not supported. `reason` takes a double-quoted, single-line string with no
  escape sequences — the only quoted token in the language; descriptions never
  use quotes.
- `//` line comments; dropped at the lexer, never hashed.
- No escape for `{$` — only the exact pattern `{$identifier}` is interpolation;
  everything else is literal.
- Property order is free; duplicate properties are an error.
- Tags are free vocabulary; `check` says nothing about them (typo detection
  belongs to policy selectors: a selector matching no existing tag warns).
- Namespaces: `::` separator, assigned by directory under `argument/`
  (`argument/power/x.inka` → `power::<id>`); root-level files are unnamespaced;
  same-namespace references may omit the prefix; `::` cannot be written inside
  an ID; policy selectors use the same notation (`power::*`); veriq source
  references keep Python's dotted `module.function` names.
- `source:` gets a format check only at `check` time; existence and pass/fail
  are eval's job.

## Project layout

- `inka.toml`: `[project] name`; `[phases] order` (total order) + `current`
  (validated against order); `[values] files`. Unknown keys are rejected.
- The argument is read by convention: every `*.inka` under `argument/`.
- Values files are flat TOML string maps. No recursive expansion (one pass);
  key collisions across files are an error; an undefined `{$var}` is an error.

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
- Excluded from hashes: IDs, tags, comments, `source:` reference strings.
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
  established (the root's wording is agreed inside inka like any other
  contract).
- blame lists only nodes whose failure cause is their own (missing / stale /
  declined / expired / evidence mismatch), not nodes failing solely through
  their children.

## CI evidence provider (test-target granularity)

- Provider contract — any evidence provider supplies: a content-addressed
  interface hash; a human-readable statement of what passing establishes;
  commit-bound results delivered as repository content (eval calls no external
  APIs); an environment hash.
- `ci-export.json` v1: `commit`; `environment.lockfile_hash`; `targets[]` with
  `id` (a test target such as `tests/hash_props.rs`), `interface_hash`
  (file-content hash, CRLF→LF normalization only), `result` (`pass` | `fail`).
- eval matching for `source: ci:<id>`: the export has an entry for `<id>`; the
  current worktree file's interface hash equals the export's; the bind
  approval's view_hash recomputes against the current description + interface
  hash; the result is pass. An export commit mismatch is a warning (content
  equality is already covered by the interface hash).

## Repository conventions

- Comments in code, fixtures, and config are English. `.md` files are English
  with a `_ja.md` (Japanese) alongside. Fixture contracts and values are
  English.
- Documents describe the current design only — no history, no "formerly X".
- Golden-vector directories are append-only, guarded in CI.