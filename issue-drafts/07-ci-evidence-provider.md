# bind: implement the CI (test-target) evidence provider

- **Labels:** enhancement
- **Milestone:** M8 area — a strong candidate to land **before** the veriq
  adapter, as the first real bind provider (strictly simpler: no assumption
  aggregation).
- **Depends on:** `inka eval` with bind stubbed (07-eval), `inka check` (03/06).
- **Implementation guide:** `docs/poc-implementation/08-ci-provider.md`.

## Motivation

The DSL already distinguishes two evidence properties — `veriq:` (a Python calc
reference) and `ci:` (a test target). Their syntax, the provider contract, the
`ci-export.json` v1 format, and eval-side matching are settled in `spec.md` →
*CI evidence provider*. What is missing is the **implementation** of the `ci:`
provider.

`ci:` lets a test stand as machine evidence: a human bind approval says "this
test is appropriate evidence for this proposition", and CI supplies the truth
side (does it pass on this commit). Dogfood payoff: propositions currently
written as `fiat until:M2 promote:bind` (e.g. `local_view_hash_sound`) can bind
to **inka's own proptests running in its own CI**.

## What the spec already fixes (do not re-decide)

- **Granularity:** test-target (a whole test file), referenced as
  `ci: "tests/hash_props.rs"`.
- **Provider contract:** a content-addressed interface hash; a human-readable
  statement of what passing establishes; commit-bound results delivered as
  repository content (eval calls no external APIs); an environment hash.
- **`ci-export.json` v1:** `commit`; `environment.lockfile_hash`; `targets[]` of
  `{ id, interface_hash, result }`, where `interface_hash` is the test file's
  content hash (CRLF→LF normalization only) and `result` is `pass` | `fail`.
- **eval matching:** the export has an entry for `<id>`; the current worktree
  file's interface hash equals the export's; the bind approval's `view_hash`
  recomputes against the current description + interface hash; `result` is pass.
  A commit mismatch is a warning (content equality is already covered by the
  interface hash).

## Scope (implementation)

- **`inka ci-export` subcommand** — computes each target's `interface_hash`
  (file content, CRLF→LF) and writes `ci-export.json` for the current commit.
  This is the provider's "results as repository content".
- **GitHub Actions job** — runs the test suite and commits (or uploads) the
  export on each relevant commit.
- **eval-side `ci:` matching** — implement the matching above; a stale or
  mismatched export leaves the bind simply not established, with a clear blame
  message.
- **Dogfood wiring** — flip one dogfood proposition (e.g.
  `local_view_hash_sound`) from `fiat` to `bind { ci: "..." }` against a real
  proptest and drive it established end-to-end.

## Example export (`ci-export.json` v1)

```json
{
  "commit": "abc123...",
  "environment": { "lockfile_hash": "sha256:..." },
  "targets": [
    {
      "id": "tests/hash_props.rs",
      "interface_hash": "sha256:...",
      "result": "pass"
    }
  ]
}
```

The human-readable statement is carried by the bind node's description; the
export keeps only identity + result (no per-target statement field).

## Out of scope

- **Effective-content (closure) change detection** — a test's helpers can change
  while the target file's own content hash still matches. Deliberately split
  into its own issue (draft 08): it applies equally to `veriq:` and `ci:`, and
  must NOT enter the bind binding target (invalidation-explosion risk).

## Integrity upgrade path (later)

- CI-signed attestations via GitHub OIDC / Sigstore (`actions/attest`), giving
  `ci:github-actions` the same standing as other non-human attestors
  (`llm:<model>` precedent). Out of scope for the first cut.

## Acceptance criteria

- [ ] `inka ci-export` produces a spec-shaped `ci-export.json` for real targets.
- [ ] A GitHub Actions job generates the export in inka's own CI.
- [ ] eval-side `ci:` matching implemented, with blame for stale or mismatched
      exports.
- [ ] One dogfood proposition (e.g. `local_view_hash_sound`) bound to a real
      proptest and established end-to-end.
- [ ] Editing the bound test file's content invalidates the bind approval.

## References

- `docs/en/design/spec.md` → *CI evidence provider (test-target granularity)*
- `docs/en/design/poc-plan.md` (CI evidence provider bullet)
- `docs/poc-implementation/08-ci-provider.md` (implementation guide)