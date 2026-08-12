# inka — Open Questions

Undecided items, with the current tentative stance and the work that will
settle each. Japanese version: [日本語版](https://www.space.t.u-tokyo.ac.jp/inka/ja/design/open-questions/).

## DSL and semantics

- **Multiset vs. set children.** No example has wanted the same child twice.
  Tentative: a set, with duplicates as an error. Revisit only if a real
  argument needs multiplicity.
- **Values typing and units.** Values accept any TOML value, rendered with its
  TOML spelling (`"0.05 deg (3σ)"`, `42`, `[1.5, 2.0, 3.25]`), so numeric
  parameters and vectors need no quoting. Whether units deserve structure is
  open. Guard: values are for shared literal constants — anything computed or
  verified belongs to an evidence provider, so values must not grow into a
  shadow one.

## Policy

- **Policy file format and location.** A dedicated file referenced from
  `inka.toml` is the leading option; format undecided. Settled by the policy
  language work.
- **Tag vocabulary.** Tags are free vocabulary today. Whether teams may declare
  a closed vocabulary (making unknown tags an error rather than relying on
  selector-side warnings) is open.

## Evidence

- **bind description vs. statement duplication.** The description (human-side
  contract) tends to restate what the evidence claims. By design both are
  presented at approval; recorded as a writing-experience observation to watch.
- **veriq contract-hash scope.** How much of a Python function's context enters
  its contract hash. Settled by the veriq integration.
- **Environment-node granularity.** External libraries are currently imagined
  as a single lockfile-hashed environment node; whether finer granularity is
  needed is open.
- **Closure-hash limits.** Runtime-traced closures reflect executed paths only;
  native extensions are opaque (covered by the environment hash); data files
  read by evidence need explicit inclusion. Start without data-file capture and
  record it as a limitation.
- **graphcal as an input.** Whether [graphcal](https://graphcal.org/docs/) — a
  unit-aware, DAG-based reactive calculation language (`.gcl`) — can be chosen
  as an input to inka is an option under consideration. Nothing is decided:
  neither adoption nor the integration shape (an evidence provider alongside
  `veriq:` / `ci:`, a source of shared values, or something else). If inka's
  operating discipline needs capabilities on the graphcal side (e.g. stable
  interface hashes or a machine-readable export), proposing them upstream as
  graphcal PRs is part of the option space.

## Audit and operations

- **Anchoring strength.** The lightweight hash chain makes deletion
  tamper-evident locally; upgrading to a transparency log is a reserved
  extension point, adopted if certification demands it.
- **LLM-lint override UX.** A human override with a logged reason exists as a
  concept; its concrete interface is undecided.
- **Large assumption sets at approval time.** How to present a large aggregated
  assumption set in the approval UI (grouping, diff-only views) — build the
  naive version first and decide from use.
