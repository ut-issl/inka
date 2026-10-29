# dogfood: inka's own design argument

inka's own design claims written in the inka DSL (ut-issl/inka#6).
This is the canonical example project and the primary fixture for the PoC
([docs/en/design/poc-plan.md](../../docs/en/design/poc-plan.md)): the goal loop —
write → check → approve → eval → edit → stale → re-approve — runs against this
project.

Japanese version: [README_ja.md](README_ja.md)

## Layout

| File | Content |
| --- | --- |
| `inka.toml` | Project definition: phase order/current, values files |
| `values.toml` | Values referenced by `{$var}` in descriptions |
| `argument/` | The argument itself; every `*.inka` under it is read by convention |
| `argument/objective.inka` | Root: decomposition into four branches |
| `argument/editing.inka` | Branch 1: edit cost is proportional to interface change (§3, §11) |
| `argument/approval.inka` | Branch 2: approvals do not degenerate into ritual (§4, §5, §7) |
| `argument/audit.inka` | Branch 3: records are auditable (§9); includes a `{$sig_scheme}` interpolation example |
| `argument/truth-validity.inka` | Branch 4: truth/validity separation (§6) |
| `argument/assumptions.inka` | Environment assumptions (plain fiat propositions) |

## DSL features exercised

Multi-file split with cross-file references; DAG sharing (`local_view_hash_sound`
has three parents); deadline annotations (`until:` / `promote:`); a markdown
description (`labels_cover_reality`); value interpolation (`approver_unforgeable`).

Every machine-verifiable claim is currently a `fiat` block carrying
`until: <milestone>` / `promote: bind` — the fiat→bind lifecycle of concept §2.1
made visible. The first real bind lands with the CI provider
(target: `local_view_hash_sound`).

## Where the authoritative design lives

This README does not restate the design. See [docs/en/design/](../../docs/en/design/):

- [concept.md](../../docs/en/design/concept.md) — what inka is
- [spec.md](../../docs/en/design/spec.md) — the decided specification
- [poc-plan.md](../../docs/en/design/poc-plan.md) /
  [full-plan.md](../../docs/en/design/full-plan.md) — implementation plans
- [open-questions.md](../../docs/en/design/open-questions.md) — what is still
  undecided

## Test evidence

`evidence/test-evidence.json` is real adapter output: the hash v1
golden-vector replay test run through `capsula run` (#75). To regenerate
from the repository root (build the adapter once with
`cargo build -p inka-evidence-rust`):

```console
capsula run -- target/debug/inka-evidence-rust --output evidence.json \
    --provenance source_revision=$(git rev-parse --short HEAD) \
    -- every_recorded_vector_still_hashes
cp "$(capsula run-dir <RUN_NAME>)/post-0-capture-file/evidence.json" \
    tests/fixtures/dogfood/evidence/test-evidence.json
```

Editing the referenced test changes its contract body, so the approval in
`approvals/shimomura.jsonl` goes stale and `bind:hash_vectors_replay`
needs fresh approval against the regenerated evidence.
