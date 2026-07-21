# dogfood: write inka's own design argument in the inka DSL

- **Labels:** documentation
- **Milestone:** M0
- **Depends on:** nothing — **this is the starting point of v0**
- **Blocks:** 01 (hash v1) — the findings here fix the contract data model that hashing freezes

## Motivation

Write inka's own design argument (the content of inka-concept.md) in the inka DSL and
use it as both a test fixture and the canonical example. This does not require a
parser. Being the first user is the fastest way to surface problems with the DSL's
writing experience — and fixing the syntax costs the least at this stage, before the
lexer/parser exist.

This issue also has a second, more important job: **it validates the contract data
model that hash v1 (issue 01) will freeze.** The hash is deliberately insulated from
DSL *syntax* (IDs excluded, child order ignored), but it does depend on *what
information a contract consists of* — descriptions, deadline annotations, the
children's descriptions. Writing a real argument by hand is the cheapest way to
check, before freezing anything, that:

- the contract components are sufficient (nothing else wants to be part of the
  approval interface),
- the deadline-annotation vocabulary (`until:` / `promote:`) is expressive enough and
  has an obvious canonical form,
- whether a node ever needs to reference the same child twice (multiset vs set
  semantics in the hash input).

## Tasks

- [ ] Write the argument using the flat block syntax: one block = one proposition's
  contract + child references (`supported-by`), inline leaves (`evidence:` /
  `assumption:`), tags (`#...`), and deadline annotations (`until:` / `promote:`).
- [ ] Place it under `tests/fixtures/`.
- [ ] Record any syntax pain points discovered while writing (as notes in the PR or
  follow-up issues) — these feed directly into the lexer/parser design.
- [ ] Explicitly answer the three contract-model questions above (sufficiency of
  components, annotation canonical form, multiset semantics) in the PR description —
  issue 01 starts from these answers.

## Acceptance criteria

- [ ] A complete dogfood argument exists in `tests/fixtures/`.
- [ ] Syntax pain points (if any) are filed as issues.
- [ ] The three contract-model questions are answered, unblocking issue 01.

## References

- inka-concept.md §12 (syntax examples), §3.1 (binding targets per approval kind)
- inka-implementation-roadmap.md §0 (dogfooding from day one), M0