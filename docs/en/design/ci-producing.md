# Producing CI evidence

The source project owns test execution. The inka CLI records content before the
run and combines it with independently supplied results after the run.
Japanese translation: [日本語版](../../ja/design/ci-producing.md).

## File preparation

Create a JSON plan whose paths are relative to the plan file:

```json
{
  "v": 1,
  "source": { "id": "github:example/source", "revision": "SOURCE_COMMIT" },
  "execution": { "id": "github-actions:RUN_ID", "attempt": 1 },
  "targets": [{
    "id": "tests/example.rs",
    "file": "tests/example.rs",
    "required": ["linux/works"]
  }]
}
```

Use the actual checked-out source revision and current execution identity.
Declare required cases independently of the successful results. Run:

```sh
inka ci-export prepare --plan plan.json --out receipt.json
```

This writes a local receipt containing a v2 bundle with no results, plus source
file guards. The receipt's absolute paths are local preparation data; deliver
only the finished bundle. Preparation never executes a test.

## Results and completion

Execute tests with the existing workflow. Its adapter produces a JSON result
report with `v: 1`, the same `source` and `execution`, and `targets` entries:
`{id, interface_hash, results: [{case, result}]}`. Use the interface hash from
the prepared target. Results must come from actual test execution, not from
manually treating an entire job as proof that selected functions ran.

```sh
inka ci-export finish --receipt receipt.json --results results.json --out ci-export.json
inka ci-export validate ci-export.json
```

`finish` verifies unchanged source files, source revision, execution attempt,
target identity, digest and required-case membership. It writes valid failure
or incomplete evidence too, then exits nonzero when any target does not pass.
An invalid report or changed source is an error and produces no new bundle.
Use fresh per-run output paths so an unsuccessful invocation cannot cause a
previous run's file to be uploaded accidentally.

`validate` checks bundle integrity, not test success. A valid bundle reporting
failure passes validation so that the failure can still be delivered. Preserve
the original test step's failing status in the workflow.

No environment metadata is required. Provider integrations can add descriptive
metadata through the optional v2 `environment` field; this does not alter the
interface digest or initial establishment policy.

Function-level producer adapters use the same receipt/results/bundle boundary.
They extract selected contracts and observe individual results without adding
language-specific behavior to the consumer.
