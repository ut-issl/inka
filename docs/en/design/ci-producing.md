# Producing test evidence

This page describes the internal collection interface. The [integration design](test-evidence-integration.md)
defines workflow onboarding; preparation, target discovery and report assembly are performed by the
integration, not manually by test authors.

The source project owns test execution. The inka CLI records content before the
run and combines it with independently supplied results after the run.
Japanese translation: [日本語版](../../ja/design/ci-producing.md).

## File preparation

The collection integration generates a JSON plan whose paths are relative to the plan file:

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
inka test-export prepare --plan plan.json --out receipt.json
```

This writes a local receipt containing a v2 bundle with no results, plus source
file guards. The receipt's absolute paths are local preparation data; deliver
only the finished bundle. Preparation never executes a test.

## Extracted contracts

Rust, pytest and Vitest integrations discover declarations and cases and extract
reviewable content, including the relevant attributes or decorators. The common
producer does not parse languages or execute tests. Integrations generate these
inputs; test authors do not maintain plans, case lists or extraction code.

Instead of `{id, file, required}`, a plan target can contain
`{id, kind, profile, content, required, source_guards}`. `kind` is `file` or
`function`; `profile` identifies the extraction rule. The common producer
computes the interface hash from the supplied contract and required cases.
A plan may contain both target forms, but each target must use exactly one form.
Unknown fields are rejected.

`source_guards` is a nonempty array of `{path, sha256}`. Paths are relative to
the plan file. The collector hashes the exact raw file bytes used for extraction,
before executing tests: `sha256:` followed by 64 lowercase hexadecimal digits.
Unlike the interface digest, a file guard does not normalize line endings.
Preparation checks these hashes and records absolute paths in the local receipt;
completion checks them again. A source change between extraction and preparation
or between preparation and completion is an error, including changes outside
the extracted function in a guarded file.

The collector is responsible for truthful extraction, required-case discovery
and correspondence to the actual execution. File guards detect changes at the
checkpoints; they do not prove extraction correctness or capture an execution
closure. A result report must reuse the prepared interface hash. No whole-job
success inference or test rerun is performed by this interface.

A preparation receipt requires at least one target and source guard. Empty
preparation plans are errors, not successful evidence. Absolute receipt paths
refer to the producer workspace; only the finished bundle is portable. Moving
collection between jobs requires the source integration to transfer the needed
source/snapshot data rather than treating local paths as shared storage.

## Results and completion

Execute tests with the existing workflow. Its adapter produces a JSON result
report with `v: 1`, the same `source` and `execution`, and `targets` entries:
`{id, interface_hash, results: [{case, result}]}`. Use the interface hash from
the prepared target. Results must come from actual test execution, not from
manually treating an entire job as proof that selected functions ran.

```sh
inka test-export finish --receipt receipt.json --results results.json --out test-export.json
inka test-export validate test-export.json
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
