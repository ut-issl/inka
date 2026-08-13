# inka — PoC Implementation Plan

The smallest implementation with which one person can experience the inka
system end to end.
Japanese version: [日本語版](https://www.space.t.u-tokyo.ac.jp/inka/ja/design/poc-plan/).

## Goal

The following loop runs against the dogfood project
(`tests/fixtures/dogfood`):

1. Write the argument → `inka check` passes
2. Approve it yourself with `inka review` → `inka eval` is clean
3. Edit a contract (description or a value) → exactly the affected approvals go
   stale and appear in blame
4. Re-approve → clean again
5. **One real CI bind becomes established**: `local_view_hash_sound` is bound
   to an actual proptest file; editing the test invalidates the bind approval,
   while implementation-only changes leave it intact and let CI re-decide truth

Step 5 is the proof of concept for bind (machine truth / human validity).

## Scope

- **Loading**: lexer + recursive-descent parser for the decided syntax;
  `inka.toml`; the `argument/` convention (flat layout only — namespace
  resolution is out); `{$var}` interpolation before anything else consumes
  descriptions.
- **`inka check`**: graph construction (interning, adjacency, toposort,
  reachability) and the structural diagnostics; non-zero exit on error.
- **Hashing**: normalization + the four view-hash kinds (objective / structure /
  bind / fiat). The bind hash uses
  the evidence interface hash with an empty assumption slot. A minimal
  golden-vector set freezes the layout before real approvals are recorded.
- **Approval records**: the v1 schema with `sig` left empty; jsonl read/write;
  the canonical-signing-bytes function (no signing yet).
- **`inka review` (CLI, no TUI)**: queue of missing/stale targets; per-target
  approve / decline (comment required) under `--as <approver>`, interactively
  or via `--approve` / `--decline` on a single target; append-only record
  writing. The queue and eval share one validity function.
- **Policy**: built-in default only — every target needs `any(1)`. No policy
  language. Decline veto applies.
- **`inka eval`**: established fixpoint (each objective requires its own
  approval); stale detection; decline veto; hard node-side `until:` expiry
  against the declared phase order (post-deadline approvals do not count
  either); blame output with per-cause reasons; non-zero exit when not clean.
- **CI evidence provider (test-target granularity)**: `ci: "<target>"`;
  `ci-export.json` generated in CI (an `inka ci-export` subcommand is the
  preferred generator); eval-side matching.

## Explicitly out of scope

Signatures and anchoring; veriq integration; the policy language; TUI; LSP;
what-if evaluation flags; visualization; import/bundles; LLM lint; namespace
resolution; approver-side `until:` enforcement; diff display at approval time;
closure hashing.

## Testing the PoC itself

- Fixtures: dogfood (the loop's target) and aocs (parser/check load test;
  its binds stay unestablished without an export, which is correct).
- Broken-fixture suite firing each diagnostic once, snapshot-fixed (insta).
- Hash: golden vectors + property tests (idempotence; indent/trailing-space/
  CRLF invariance; newline and single-character sensitivity; child-order
  invariance for structure).
- Scenario tests: approve → edit → stale; value change → only referencing
  contracts stale; test-file change → bind invalidated; decline → unsatisfied.

## Module layout (single crate)

```text
src/
  syntax/    // lexer, parser, AST, span
  model/     // graph, ID interning
  hash/      // normalization and view_hash (frozen)
  record/    // approval records (frozen)
  eval/      // established fixpoint, blame, built-in policy
  provider/  // ci-export.json reading and matching
  cli/       // check / hash / approve / eval / ci-export
tests/
  golden/    // hash and record vectors (append-only)
  fixtures/  // dogfood, aocs, broken variants
```
