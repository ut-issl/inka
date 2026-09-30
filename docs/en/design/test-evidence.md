# Test evidence contracts

Japanese translation: [日本語版](../../ja/design/test-evidence.md).

## Responsibility boundary

Define the project repository as the repository holding the inka project, and
the source repository as the repository holding the tests used as inka
evidence. They may be the same repository.

The source repository selects tests and executes them in its own environment.
Using an adapter such as the inka CLI or a GitHub Action, the source repository
emits an evidence JSON file that the inka project can recognize as test
execution results.

Capsula owns storage and delivery of execution results. The source repository
executes tests as a capsula run and stores the evidence JSON as a run artifact.
Capsula's directory vault and server storage (push/pull) also serve as delivery
to the project repository side. Inka does not implement this storage and
delivery machinery and takes no responsibility for it.

The inka project reads evidence assuming the latest evidence JSON is present at
a configured path. Whether that JSON is current is the responsibility of the
delivery machinery from the source repository. Inka does not interpret
capsula's vault layout; it handles only the evidence JSON file path. Evaluation
runs offline against the imported version and needs no source checkout,
compiler, test runner or network.

The JSON records only the tests the source repository provides and their
execution results. Whether they correspond to an inka proposition is judged by
review of the bind node (human approval).

## Evidence JSON for binds

A bind references evidence with `test: "<test-id>"`. The test-id is an opaque
identifier assigned by the source side. Whether it starts with a file name or
contains a repository prefix is internal structure inka does not interpret:
inka only needs the evidence to be uniquely identifiable. Duplicate test-ids in
loaded evidence are an input error.

The evidence JSON follows `schemas/test-evidence-v1.schema.json` at the
repository root. For each test-id it records:

- **id** — the test-id binds reference.
- **body** — the human-reviewable source string of the test (nonempty UTF-8).
  How much to include (attributes, decorators, parameters) is the source-side
  adapter's extraction responsibility; inka never parses or executes
  language-specific syntax.
- **executions** — the declared executions required for this test to pass,
  with their results. The whole list is the required set; an entry whose
  `outcome` (pass / fail / skip / cancelled) is omitted is a declared
  execution whose result is missing. At least one entry is required, and
  duplicate execution IDs are an input error.

The file-level **provenance** object records descriptive metadata such as the
tested source revision or the capsula run ID. It is not part of the approval
contract, and inka ignores unknown keys.

```json
{
  "v": 1,
  "provenance": {
    "source_revision": "4f2a1c9",
    "capsula_run_id": "01K8WSYC91YAE21R7CWHQ4KYN2"
  },
  "tests": [
    {
      "id": "example-repo/tests/hash_props.rs::roundtrip",
      "body": "#[test]\nfn roundtrip() { /* ... */ }",
      "executions": [{ "id": "default", "outcome": "pass" }]
    }
  ]
}
```

The contract body is included in reviewable form, not only as a digest. Inka
presents it to the approval interface. The evidence JSON carries no declared
digest: inka computes the interface hash at load time, and verifying content
in transit is capsula's responsibility.

The required execution set is declared by the evidence JSON itself. It must not
be inferred from successful results alone, shrunk by an aborted run, or
assembled from successes across different runs.

## Content hashing and reapproval

The interface hash covers the test-id, the contract body and the required
execution ID set. Body normalization replaces CRLF with LF only; comments,
Unicode normalization state, indentation and trailing whitespace remain
content. Framing uses the same rules as the view hash in the
[specification](spec.md) (`field` / `list` with u64-LE length prefixes).

```text
preimage = field("inka:test-evidence:v1") ‖ field(test-id)
           ‖ field(normalized body) ‖ list(required execution IDs)
interface_hash = SHA-256(preimage)
```

Required execution IDs are sorted bytewise: reordering `executions` is not an
edit. Outcomes do not enter the hash.

The bind view hash keeps its frozen v1 layout: the proposition description and
annotations, the raw interface digest, and the currently empty assumption list.
Run IDs, outcomes and environment metadata do not enter the approval contract.

Changing the contract body, the test-id or the required execution set requires
fresh approval: silently dropping a failing case from the required set cannot
keep looking like a pass under the existing approval. A new result for the
same interface can be evaluated with the existing approval.
Stale approvals and execution failures are reported as independent causes: do
not call a stale approval an execution failure, or request reapproval just
because a test with the same contract body started failing.

Provenance is descriptive metadata in this unsigned protocol. Recomputing a
digest detects inconsistent content; it does not authenticate who ran the test
or prove that every execution was reported. Signatures are a separate
authentication mechanism.

## Bind evaluation

Assuming the node is not expired and has no live decline, a bind is established
when both a valid approval and passing evidence exist. Existing approval
quorum, veto and expiry rules continue to apply.

| Observed state | Meaning for the bind |
| --- | --- |
| Every required execution passed | Evidence may establish the bind |
| Any required execution failed | Evidence does not establish the bind |
| A required execution skipped or was cancelled | Evidence does not establish the bind |
| A required execution has no result | Evidence does not establish the bind |
| The referenced test-id is absent from the evidence | Missing evidence; not established |
| Conflicting/duplicate records for one execution | Malformed input; abort evaluation |

| Imported evidence and approvals | Evaluation |
| --- | --- |
| Contract body A, matching approval, all required executions pass | Established |
| Matching approval, a required execution fails | Unestablished; evidence failure |
| Passing evidence, no approval | Unestablished; missing approval |
| Body B imported; only A was approved | Unestablished; stale approval |
| Same body A, new passing execution | Approval stays valid; established |
| Same body A, new failing execution | Approval stays valid; unestablished |
| Source repository edited; imported body is still A | Same result as before the edit |
| Same body under a different test-id | Unestablished until the changed interface is approved |
| Same body, changed required execution set | Unestablished until the changed interface is approved |

## Out of scope / follow-up work

- Golden vectors for the interface hash, frozen together with the consumer
  implementation that computes it.
- Per-language/framework adapters (Rust/proptest, Python/pytest,
  JavaScript/TypeScript with Vitest, ...): extraction rules and runner
  integration.
- Environment metadata (such as lockfile hashes) and warning/error policies for
  environment changes.
- Signed attestations authenticating executors.
