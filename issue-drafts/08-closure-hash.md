# eval: execution-closure hash — detect effective-content changes behind a stable interface

- **Labels:** enhancement
- **Milestone:** after draft 07 (extends the provider export format; applies to
  both `veriq:` and `ci:` providers)
- **Depends on:** 07 (provider contract / export schema), M3 policy engine for the
  predicate part

## Motivation

Bind approvals fix the evidence's *interface* (statement + normalized body).
This is intentional: implementation truth is re-established by re-execution, and
binding approvals to implementation internals would bring back the invalidation
explosion inka exists to avoid.

But one failure mode passes through both the hash and re-execution: **a helper
called by the test/verification is gutted while the interface stays identical** —
the weakened evidence keeps passing, the approval stays valid, and nothing
surfaces. Static call-graph analysis cannot close this soundly in Python
(dynamic dispatch, monkeypatching). Runtime tracing can.

## Mechanism

- During the provider's run (pytest plugin / veriq export), record the set of
  code objects actually executed (coverage-style tracing, per evidence item),
  normalize and hash them, and emit a **`closure_hash`** per test/verification
  in the export file. Prior art: pytest-testmon, Bazel incremental test
  selection — this moves "what does this evidence depend on" from static
  guessing to recorded fact.
- **`closure_hash` does NOT enter the bind binding target.** Approvals stay
  bound to the interface; a harmless helper refactor must not invalidate them.
- eval diagnostic: if an evidence's `closure_hash` changed since the last
  recorded run while its interface hash did not, emit a warning
  ("effective content of this evidence changed behind an unchanged interface").
  The gutted-helper case can no longer pass silently.
- Policy predicate **`closure-fresh`**: teams opt into strictness per selector,
  e.g. `rule bind(#safety): closure-fresh` — closure changes require
  re-approval for safety-tagged binds only. Fixation depth becomes a reviewable
  policy declaration instead of a global constant.

## Known limits (document honestly)

- Coverage of one run reflects the paths that ran: nondeterministic or
  data-dependent branches may differ between runs. Hash at code-object
  granularity (not line granularity) to keep this stable.
- Native extensions are opaque — covered by the environment/lockfile hash.
- Data files read by the evidence need explicit inclusion (declared inputs or
  file-hash capture) — start without, record as a limitation.

## Acceptance criteria

- [ ] `closure_hash` present in both provider exports.
- [ ] eval warning fires on closure change with stable interface (test with a
      gutted-helper fixture).
- [ ] `closure-fresh` predicate implemented and snapshot-tested.
- [ ] Rationale (why closure is not part of the binding target) in rustdoc.

## References

- draft 07 (provider contract)
- inka-concept.md §6 (interface fixation), §7 (policy predicates)