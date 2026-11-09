# Calc evidence contracts

Japanese translation: [日本語版](../../ja/design/calc-evidence.md).

## Responsibility boundary

Calc evidence is evidence drawn from a calculation graph: a graph of root
models, calculations and verifications whose nodes depend on one another, as
[veriq](https://github.com/ut-issl/veriq) builds from decorated Python
functions. A bind references one verification of such a graph with
`calc: "<path>"`. veriq is the first producer, and the contract is shaped by
its graph: a definition's `kind` is one of veriq's three node kinds. Nothing
in it requires veriq, though, so another producer whose graph maps onto those
kinds can emit the same evidence.

Define the project repository as the repository holding the inka project, and
the source repository as the repository holding the calculation graph. They
may be the same repository.

The source repository evaluates the graph in its own environment. Using an
adapter — for veriq, `inka-evidence-veriq` from the `inka-evidence` Python
package — it emits an evidence JSON file that the inka project can recognize
as calc evidence. Capsula owns storage and delivery, exactly as for
[test evidence](test-evidence.md): the source repository evaluates as a
capsula run, stores the JSON as a run artifact, and the project repository
reads it at a configured path. Inka does not implement storage or delivery,
and evaluation runs offline against the imported version with no Python
interpreter, no veriq and no network.

Rust never parses Python. The adapter is the only component that reads the
graph's source; everything inka needs is in the JSON.

Whether an evidence target corresponds to an inka proposition is judged by
review of the bind node (human approval). What the reviewer sees is defined
below: not one function but everything it rests on.

## Evidence JSON for binds

A bind references evidence with `calc: "<path>"`. The path is the target
verification's path in the producer's graph; for veriq it is
`Scope::?verification_name`, e.g. `calc: "Power::?verify_margin"`. Inka
compares it with the paths in loaded evidence byte for byte and interprets
nothing else about it. Duplicate target paths in loaded evidence are an input
error.

The evidence JSON follows `schemas/calc-evidence-v1.schema.json` at the
repository root. For each target it records:

- **path** — the target path binds reference.
- **definitions** — the target's **definition-level upstream subgraph**, the
  target itself included: every root model, calculation and verification the
  target depends on, each exactly once, with its `path`, its `kind` (`model`
  / `calculation` / `verification`) and its human-reviewable `source` string.
  How much of a definition's text to include (decorators, signatures,
  docstrings) is the adapter's extraction responsibility; inka never parses
  or executes language-specific syntax. Definition-level means one entry per
  definition, not one per output leaf: a calculation that produces a table is
  one definition however many of its cells the target consumes.
- **assumes** — the inka nodes the upstream subgraph assumes, as
  fully-qualified inka node IDs (`namespace::id`; a root-level node is its
  bare `id`). The adapter collects them from the producer's node metadata and
  unions them over the definitions, so each ID appears once. See
  [Assumptions](#assumptions).
- **leaves** — the required leaf results of the target: one entry per leaf of
  the verification's output, identified by `id`, with its boolean `result`.
  A scalar verification has one leaf, identified by the target path; a
  table-valued one has one leaf per cell. The whole list is the required
  set; an entry whose `result` is omitted is a declared leaf whose result is
  missing. At least one entry is required, and duplicate leaf IDs are an
  input error.
- **valid** — whether the producer's own validity conditions hold for the
  target. For veriq this is the `@vq.assume` mechanism: a verification whose
  upstream assumes another verification is invalid when that verification
  fails. The assumed verification is part of the target's upstream subgraph,
  so its definitions and the argument nodes it assumes enter the target's
  `definitions` and `assumes` like any other dependency's.
- **xfail** — whether the producer marks the target as an expected failure.
  Optional; defaults to false.

The file-level **provenance** object records descriptive metadata such as the
evaluated source revision or the capsula run ID. It is not part of the
approval contract, and inka ignores unknown keys.

```json
{
  "v": 1,
  "provenance": {
    "source_revision": "4f2a1c9",
    "capsula_run_id": "01K8WSYC91YAE21R7CWHQ4KYN2"
  },
  "targets": [
    {
      "path": "Power::?verify_margin",
      "definitions": [
        { "path": "Power::$Config", "kind": "model", "source": "class Config(BaseModel): ..." },
        { "path": "Power::@budget", "kind": "calculation", "source": "def budget(...): ..." },
        { "path": "Power::?verify_margin", "kind": "verification", "source": "def verify_margin(...): ..." }
      ],
      "assumes": ["power::eclipse_model_valid"],
      "leaves": [{ "id": "Power::?verify_margin", "result": true }],
      "valid": true
    }
  ]
}
```

The sources are included in reviewable form, not only as a digest. Inka
presents them to the approval interface. The evidence JSON carries no
declared digest: inka computes the interface hash at load time, and verifying
content in transit is capsula's responsibility.

The required leaf set is declared by the evidence JSON itself. It must not be
inferred from successful results alone, shrunk by an aborted evaluation, or
assembled from successes across different runs.

Inka reads the file strictly. A file that breaks the contract is an input
error, never weaker evidence:

- a version other than 1, or an unknown key anywhere outside `provenance`;
- a target path, definition path or leaf ID that is empty or contains
  whitespace — any character with the Unicode `White_Space` property, the same
  rule a `calc:` value is held to;
- a target with no definitions, a repeated definition path, an empty source,
  or no definition for its own path;
- an assumption that is not a fully-qualified node ID, or is repeated;
- a target with no leaves, or a repeated leaf ID;
- a `result` or `provenance` written as `null` rather than omitted;
- a target path that appears more than once in the loaded evidence.

## Assumptions

An assumption is a proposition the evidence relies on without establishing
it: a model that is taken to envelope reality, a table that is taken to be
complete. Inka already has a place for such propositions — they are nodes of
the argument, usually `fiat` nodes, discharged by approval or by their own
children. The argument is therefore the assumption registry: there is no
second list of assumption statements to keep in step with it.

A calc producer declares which argument nodes a definition assumes in that
definition's user metadata, under an `inka` key:

```python
@power.calculation(metadata={"inka": {"assumes": ["power::eclipse_model_valid"]}})
def budget(config: Config) -> PowerBudget: ...
```

Every entry is a fully-qualified inka node ID. The producer stores the strings
and does nothing else with them; whether the IDs exist is inka's business.

Assumptions aggregate through the graph by structure, not by hand: the
adapter takes the target's definition-level upstream subgraph, collects the
declared IDs of every definition in it and unions them. A node of any mode may
be assumed. The result is the target's `assumes` list, and the three things
inka does with it are:

- **Hashing.** The assumed nodes' rendered descriptions, normalized like any
  description and sorted bytewise, fill the assumption slot of the bind's view
  hash (see [Content hashing](#content-hashing-and-reapproval)). Rewording an
  assumption invalidates the bind approval. The slot holds no IDs, so renaming
  a node does not by itself — but a producer that writes the ID inside a
  hashed source, as veriq's decorator does, changes that source with the
  rename, and the interface hash with it.
- **Dependency.** A calc bind is established only if every node it assumes is
  established, in addition to its own approval and passing evidence. An
  assumption that is not yet discharged — an unapproved fiat, an expired
  deadline — leaves the bind unestablished with that node named as the cause.
- **Presentation.** The assumption set, with each node's ID and description,
  is shown at bind approval next to the sources, and its change since the
  last approval is shown as a diff (see
  [Approval snapshots](#approval-snapshots-and-diffs)).

### Load-time structure

Assumptions are edges of the argument graph, so inka reads the structural part
of calc evidence — targets, definitions, assumption IDs — when it loads the
project, not only when it evaluates. `inka check` reports as errors:

- an assumption ID that names no node of the argument;
- a dependency cycle that passes through an assumption edge, e.g. a bind
  whose evidence assumes the bind itself or one of its ancestors. The nodes
  on the cycle are named.

The leaf results, validity and `xfail` are read by evaluation only. A `calc:`
reference whose target is absent from loaded evidence is not a check error;
it is missing evidence, reported by `inka eval` as an unestablished bind.
The `calc:` value itself gets a format check at check time: non-empty, a
single line, no whitespace.

## Content hashing and reapproval

The interface hash covers the target path, every definition of the upstream
subgraph with its path and source, and the required leaf-ID set. Source
normalization replaces CRLF with LF only; comments, formatting, Unicode
normalization state, indentation and trailing whitespace remain content.
Framing uses the same rules as the view hash in the
[specification](spec.md) (`field` / `list` / `entries` with u64-LE length
prefixes).

```text
preimage = field("inka:calc-evidence:v1") ‖ field(target path)
           ‖ entries(definitions) ‖ list(required leaf IDs)
entries(definitions) = u64-LE(count) ‖ field(path[0]) ‖ field(normalized source[0]) ‖ ...
interface_hash = SHA-256(preimage)
```

Definitions are sorted bytewise by path and leaf IDs bytewise: the order a
producer lists them in is not an edit. A definition's `kind` is presentation
metadata and does not enter the hash. Results, validity, `xfail` and
provenance do not enter the hash.

The bind view hash keeps its frozen v1 layout, with the assumption slot now
filled for calc binds:

```text
bind = field("inka:bind:v1") ‖ field(desc) ‖ list(annotations) ‖ list(comments)
       ‖ field(interface_hash) ‖ list(assumptions)
assumptions = the assumed nodes' normalized descriptions, sorted bytewise
```

A test bind's slot stays empty, so the vectors frozen before calc evidence
existed are unchanged. The layouts are frozen by the vectors under
`tests/golden/hash/calc-evidence-v1/` and `tests/golden/hash/v1/`.

Consequently, fresh approval is required when:

- the source of any upstream definition changes — the approver judged the
  verification *together with* the calculations and models it rests on;
- an upstream definition is added, removed or renamed;
- the required leaf set changes;
- an assumed node's description changes, or the assumption set gains or
  loses a node.

A new result for the same interface and the same assumption set can be
evaluated with the existing approval. Stale approvals and evidence failures
are reported as independent causes, as for test evidence.

## Bind evaluation

Assuming the node is not expired and has no live decline, a calc bind is
established when a valid approval exists, the evidence passes, and every
assumed node is established. Existing approval quorum, veto and expiry rules
continue to apply.

The evidence passes when every required leaf has `result: true`, `valid` is
true and `xfail` is false.

| Observed state | Meaning for the bind |
| --- | --- |
| Every required leaf is `true`, `valid`, not `xfail` | Evidence may establish the bind |
| Any required leaf is `false` | Evidence does not establish the bind |
| A required leaf has no result | Evidence does not establish the bind |
| `valid` is false | Evidence does not establish the bind |
| `xfail` is true | Evidence does not establish the bind (expected failure) |
| The referenced target path is absent from the evidence | Missing evidence; not established |
| An assumed node is not established | Not established; the assumption is the cause |
| A file that breaks the contract, e.g. a duplicate target path | Input error at load; nothing is evaluated |

| Imported evidence, argument and approvals | Evaluation |
| --- | --- |
| Sources A, matching approval, all leaves pass, assumptions established | Established |
| Matching approval, a leaf fails | Unestablished; evidence failure |
| Passing evidence, no approval | Unestablished; missing approval |
| An upstream calculation edited; only the old sources were approved | Unestablished; stale approval |
| Same sources, new passing evaluation | Approval stays valid; established |
| Same sources, an assumed fiat not yet approved | Unestablished; assumption not established |
| Same sources, an assumed node's description edited | Unestablished until the changed contract is approved |
| An assumed node renamed, its ID kept outside the hashed sources | Same result as before the rename |
| An assumed node renamed, its ID written in a hashed source (veriq) | Unestablished; stale approval |
| Same sources, a new `assumes` entry added upstream | Unestablished until the changed contract is approved |
| Same sources under a different target path | Unestablished until the changed interface is approved |

## Approval snapshots and diffs

A bind approval is bound to a hash; a reviewer asked to approve again wants to
know what changed. For calc binds the interesting change is often the
assumption set, so `inka review` records what was approved and shows the
difference next time.

On approval of a calc bind, `inka review` writes
`approvals/snapshots/<view_hash>.json`, where `<view_hash>` is the approved
hash's hex digest. The file records the target path, the assumption set (each
node's ID and rendered description) and the definitions (path and source) as
presented. It is content-addressed: two approvers approving the same view
write the same file, and nothing is appended.

At review time, the current assumption set is diffed against the snapshot of
the most recently approved view hash of the same target — the latest
`approve` record for the target whose snapshot exists — and shown as added,
removed and reworded assumptions. When no snapshot exists the review says so
and shows the full set. Sources are shown in full; a source diff is not part
of this design.

Snapshots are presentation aids. `inka eval` never reads them, so a missing
or deleted snapshot changes what a reviewer is shown, never what is
established. They are committed with the approvals so every reviewer sees
the same baseline.

Large assumption sets are presented naively for now; grouping and diff-only
views are an [open question](open-questions.md).

## Delivery through capsula

Delivery follows the [test evidence](test-evidence.md#delivery-through-capsula)
procedure with the veriq adapter in place of a test adapter:

```console
$ capsula run -- inka-evidence-veriq path/to/project.py --input project.in.toml \
    --output calc-evidence.json --provenance source_revision=$(git rev-parse HEAD)
$ capsula push <RUN_NAME>
```

On the project side, pull the run, copy the artifact to the configured path
and check its `capsula_run_id` as for test evidence. The project names calc
evidence files separately from test evidence files:

```toml
[evidence]
tests = ["evidence/test-evidence.json"]
calcs = ["evidence/calc-evidence.json"]
```

`inka check` and `inka eval` then read the configured paths as usual.

## Out of scope / follow-up work

- Source diffs at approval time; grouping or filtering of large assumption
  sets.
- Environment metadata (lockfile hashes) and policies for environment
  changes.
- Signed attestations authenticating the evaluation.
- Metadata on producer-side requirement nodes, and typed metadata; both are
  producer concerns.
