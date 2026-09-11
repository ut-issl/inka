# dogfood: inka's own design argument

Implementation status: this prototype does not yet implement the revised
[workflow-only integration contract](../../../docs/en/design/test-evidence-integration.md).
The replacement must reuse an existing independent test job. The passing
prototype workflow does not validate the revised onboarding requirements.

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

The first CI bind, `normalization_idempotent`, states only the normalization
property checked for the selected generated inputs. It references a function
contract extracted from `crate/inka-core/tests/hash_props.rs`. The broader
`local_view_hash_sound` claim remains fiat: a passing property test does not
prove a general one-to-one correspondence between all contracts and hashes.

The [dogfood workflow](../../../.github/workflows/dogfood-evidence.yaml) prepares
file and function contracts, runs the eight proptests in the source job, then
publishes and imports the v2 bundle through official Artifact Actions.
`evidence/ci.json` is a local import, excluded from Git. Without an import the
bind reports missing evidence; with a passing import it still requires approval.
The workflow never writes human approvals. The CLI integration test uses
explicitly synthetic records in a temporary copy to verify pass → approve →
established → changed contract → stale → reapprove, plus nonpass/input errors.

The remaining machine-verifiable claims retain `fiat` with milestone annotations.

## Where the authoritative design lives

This README does not restate the design. See [docs/en/design/](../../docs/en/design/):

- [concept.md](../../docs/en/design/concept.md) — what inka is
- [spec.md](../../docs/en/design/spec.md) — the decided specification
- [poc-plan.md](../../docs/en/design/poc-plan.md) /
  [full-plan.md](../../docs/en/design/full-plan.md) — implementation plans
- [open-questions.md](../../docs/en/design/open-questions.md) — what is still
  undecided
