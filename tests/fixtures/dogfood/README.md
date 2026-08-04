# dogfood: inka's own design argument

inka's design claims (inka-concept.md) written in the inka DSL (ut-issl/inka#6).
This is the canonical example project and the primary fixture for the minimum
implementation (inka-minimum-roadmap.md): the goal loop — write → check → approve →
eval → edit → stale → re-approve — runs against this project.

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
(inka-minimum-roadmap.md §8, target: `local_view_hash_sound`).

## Where the authoritative design lives

This README does not restate the design. See:

- **inka-concept.md** — the current design
