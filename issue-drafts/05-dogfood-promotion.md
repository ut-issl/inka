# dogfood: promote the dogfood argument from tests/fixtures/ to a live project location

- **Labels:** enhancement
- **Milestone:** M3 (this is the last step of "start dogfooding")
- **Depends on:** #6 (the dogfood argument exists), and the M3 `inka eval` work
  (approval log + policy + eval must exist for the argument to be "live")

## Motivation

The dogfood argument starts its life in `tests/fixtures/` (#6), where it is purely
*input data* for parser/hash/eval tests. That location is correct for M0–M2.

From M3 onward its nature changes: dogfooding means the argument acquires
`approvals/*.jsonl`, `phase.inka`, and `policy.inka`, gets edited routinely, and gets
approved through real `inka` commands. At that point it is no longer a test fixture —
it is **this repository's own live inka project**, and a live project that people
edit and approve does not belong under `tests/`.

Filing this now so the promotion is a planned step of M3, not something we notice
after paths and CI have ossified around `tests/fixtures/`.

## When to promote (trigger)

Promote at the start of M3 dogfooding — concretely, at the moment we write the first
*real* approval record against the argument (as opposed to hand-crafted jsonl test
fixtures). Before that moment there is nothing "live" about it; after that moment
every day of delay grows the confusion between fixture data and live data.

## Tasks

- [ ] Decide the destination: repository root (argument + `approvals/` + `phase.inka`
  + `policy.inka` at top level, i.e. exactly how a real user's repo would look) vs. a
  dedicated `dogfood/` directory. Default proposal: **repo root**, because the dogfood
  repo should double as the canonical example of an inka project layout.
- [ ] Move the argument files; keep `tests/fixtures/` for what actually is fixture
  data: deliberately broken variants (diagnostics tests) and small synthetic
  arguments (eval scenario tests).
- [ ] Decide how tests consume the live argument afterwards: reference it in place
  (dogfood doubles as an integration test — `inka check` must always pass on it) or
  snapshot a frozen copy into fixtures. Default proposal: **reference in place** for
  `check`/`hash` smoke tests; synthetic fixtures for everything that needs a stable
  expected output.
- [ ] Update CI paths and any docs that point at the old location.

## Acceptance criteria

- [ ] The live argument (+ approvals, phase, policy) lives outside `tests/`.
- [ ] `tests/fixtures/` contains only broken variants and synthetic arguments.
- [ ] CI runs `inka check` (and `eval`, once available) against the live argument.

## References

- inka-implementation-roadmap.md §0 (dogfooding from day one), M3 (first dogfooding
  point), §5 (update the dogfood argument at the end of every milestone)
- inka-concept.md §13 (workflow: approval is post-merge on main — the live argument
  is where this workflow actually runs)
