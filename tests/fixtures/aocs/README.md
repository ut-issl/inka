# aocs: attitude-control example argument

A second example argument, modelling a satellite attitude-control subsystem (AOCS).
It deliberately exercises the DSL features the dogfood argument (inka's meta
argument) leaves unloaded: **bind with `ci:`/`veriq:`, tags, numeric value
interpolation, and DAG sharing of a bind node**.

Japanese version: [README_ja.md](README_ja.md)

## Layout

| File | Content |
| --- | --- |
| `inka.toml` | Project definition; phases are satellite-development BBM / EM / FM (current = EM) |
| `values.toml` | Eight requirement/condition values (pointing accuracy, tip-off rate, margins, …) |
| `argument/objective.inka` | Root: control performance and vehicle safety |
| `argument/pointing.inka` | Branch 1: pointing performance (#mission); three binds |
| `argument/safety.inka` | Branches 2–3: detumbling and safe mode (#safety); includes a markdown bullet-list contract |
| `argument/momentum.inka` | Branch 4: momentum management; shares `actuator_authority_ok` with pointing |
| `argument/assumptions.inka` | Disturbance-model and mass-property assumptions (fiat), shared across branches |

Features exercised: bind ×5 (veriq references), `#safety` / `#mission` tags,
`{$var}` ×8 (numbers with units), **DAG sharing of a bind node**
(`actuator_authority_ok` has two parents), fiat with `until: FM` /
`promote: bind` (the EM-phase debt made explicit), and a markdown bullet-list
contract (`safe_mode_ok`).

## Findings (current status)

Settled since this example was written — now part of the decided specification
([docs/en/design/spec.md](../../docs/en/design/spec.md)):

- **ID namespaces**: `::` separator, assigned by directory structure under
  `argument/`; policy selectors use the same notation (`power::*`). `veriq:`
  references keep Python's dotted `module.function` names.
- **Tags** are free vocabulary; `check` says nothing about them (typo detection
  arrives with policy selectors in M3).
- **Unresolvable `ci:`/`veriq:` references** get a format check only at `check` time;
  existence is eval's job.

Still open / standing observations:

1. **bind description vs. evidence statement duplication.** The description
   (human-side contract) restates what the verification claims. Not redundant by
   design — a bind approval presents both — but worth tracking as writing
   experience.
2. **Values as plain strings suffice** (`"0.05 deg (3σ)"`); unit typing is not
   needed so far.
3. **Multisets remain unnecessary**: no case wanted the same child twice
   ("set + duplicate-is-error" stands).

## What this example demonstrated

- Mode-declaring blocks (structure / bind / fiat) get *more* readable once binds
  actually appear: where machine verification ends and acceptance begins is
  visible at a glance.
- The row of `until: FM` reads directly as the EM-phase debt list — the value of
  `--forecast FM` is tangible.
- With numbers in values.toml, inka's core story — a requirement change
  propagating to interfaces — is demonstrable in this example alone: changing
  `pointing_accuracy` invalidates exactly two contracts plus their parent.
