# bind: generalize evidence providers and add a GitHub CI (test) provider

- **Labels:** enhancement
- **Milestone:** M8 area — but a strong candidate to land **before** the veriq
  adapter, as the first real bind provider (it is strictly simpler: no assumption
  aggregation)
- **Depends on:** M3 (`inka eval` with bind stubbed), draft 06 (`inka check`)

## Motivation

`source:` currently admits only veriq references. Test code run in CI can serve the
same role: a human bind approval says "this test is appropriate evidence for this
proposition", and CI supplies the truth side (does it pass on this commit).

The veriq connection was deliberately isolated behind a one-file JSON adapter;
this issue generalizes that seam into a **provider contract** and adds `ci:` as the
second provider. Payoff for dogfooding: the propositions currently written as
`fiat until:M2 promote:bind` in the dogfood argument (e.g. `local_view_hash_sound`)
can bind to **inka's own proptests running in its own CI**.

## Provider contract

Any evidence provider must supply four things for a bind to be meaningful:

1. **Content-addressed identity** — a hash of the evidence's *interface*
   (for a test: its normalized function AST; a name is not an identity).
   Bind approvals bind to this hash; editing the test invalidates the approval.
2. **Statement** — a human-readable claim of what passing establishes
   (for pytest: the docstring). Presented at approval time.
3. **Commit-bound results as repository content** — eval never calls external
   APIs; the provider writes an export file (result + commit SHA) into the repo,
   exactly like the veriq export JSON.
4. **Environment** — lockfile hash + CI workflow-definition hash as an
   environment node (declared assumptions where the provider supports them).

## Scope

- `source:` provider syntax: `veriq:<module>.<name>` / `ci:<test-id>`
  (test IDs keep pytest's native form, e.g. `tests/test_hash.py::test_order_invariance`)
- **Opt-in marker** on the test side (e.g. `@inka_evidence(statement="...")`):
  only annotated tests are bindable. The marker carries the statement and makes
  the evidence boundary explicit (mirror of veriq's `@calc` discipline).
- pytest plugin generating `ci-export.json`:

  ```json
  {
    "commit": "abc123...",
    "environment": { "lockfile_hash": "...", "workflow_hash": "..." },
    "tests": [
      {
        "id": "tests/test_hash.py::test_order_invariance",
        "interface_hash": "sha256:...",
        "statement": "hash is invariant under child permutation",
        "result": "pass"
      }
    ]
  }
  ```

- GitHub Actions job that runs the suite and commits (or uploads) the export.
- eval side: match bind approval hash == current interface hash; export commit ==
  evaluated commit; result == pass. Stale/mismatched export → the bind is simply
  not established (with a clear blame message).
- concept §2.1 update: "source is restricted to veriq" → "source references a
  registered evidence provider" + the provider contract above.
- Integrity upgrade path (later, M7 machinery): CI-signed attestations via
  GitHub OIDC / Sigstore (`actions/attest`), giving `ci:github-actions` the same
  standing as other non-human attestors (`llm:<model>` precedent).

## Out of scope

- **Effective-content (closure) change detection** — a test's helpers can change
  while its interface stays identical. Deliberately split into its own issue
  (draft 08): it applies equally to veriq and ci, and must NOT enter the bind
  binding target (invalidation-explosion risk).

## Acceptance criteria

- [ ] Provider contract documented (rustdoc) and `source:` grammar generalized.
- [ ] pytest marker + export generation working in inka's own CI.
- [ ] One dogfood proposition (e.g. `local_view_hash_sound`) bound to a real
      proptest and established end-to-end.
- [ ] Editing the bound test's body invalidates the bind approval (test).
- [ ] concept.md §2.1 / §6 updated to the provider generalization.

## References

- inka-concept.md §2.1 (source restriction), §6 (truth/validity separation),
  §9 (attestations; `llm:` precedent for non-human attestors)
- inka-implementation-roadmap.md M8