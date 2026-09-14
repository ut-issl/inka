# Rust evidence adapter

This page describes the internal collection interface. The [integration design](test-evidence-integration.md)
defines workflow onboarding; preparation, target discovery and report assembly are performed by the
integration, not manually by test authors.

The source-side CLI supports ordinary `#[test]` functions and declarations in
`proptest!` blocks, including inline modules. It parses Rust syntax and token
groups with `syn` and retains source text using parser spans. It does not infer
function boundaries from line patterns or execute Cargo.
Japanese translation: [日本語版](../../ja/design/ci-rust.md).

## Preparation and execution

Use the [preparation plan](ci-producing.md), adding `selector` to a function
target. The selector is its libtest name within this test binary, for example
`normalization_is_idempotent` or `nested::property`. Each required case must be
`ENVIRONMENT/SELECTOR`. Omit `selector` for a whole-file contract.

```json
{
  "id": "crate/inka-core/tests/hash_props.rs::normalization_is_idempotent",
  "file": "crate/inka-core/tests/hash_props.rs",
  "selector": "normalization_is_idempotent",
  "required": ["linux/normalization_is_idempotent"]
}
```

The workflow runs the tests separately, with ordinary output capture enabled:

```sh
inka ci-export prepare-rust --plan plan.json --out receipt.json
cargo test -p inka-core --test hash_props -- --format pretty --color never > libtest.txt
inka ci-export finish-rust --receipt receipt.json --report linux=libtest.txt --out ci-export.json
```

Arrange the workflow so finishing/publishing still runs after a test failure,
without hiding that failure. Use output from this same prepared execution.
Repeat `--report ENV=PATH` for explicitly required environments. Each report
must represent one test binary, not combined workspace output. Filtered-out
required tests remain missing; ignored tests remain non-passing. An interrupted
report cannot certify a completed passing run.

## Extraction profile and limits

`rust-source:v1` retains a selected ordinary test's attributes and definition.
For a proptest, it emits a `proptest!` wrapper containing the block's inner
configuration attributes and the selected property's attributes, strategies
and body. Unrelated property definitions do not enter that interface content.
External helper implementations, imports and enclosing module configuration
are not included: this is not closure capture.

Out-of-line modules must be targeted through their actual source file. Conditional
duplicate names are rejected as ambiguous. Arbitrary macros that generate tests,
unsupported proptest declaration syntax and unresolved selectors fail explicitly.
Ordinary tests using other framework attributes are not implicitly supported.

The result adapter supports the standard captured libtest pretty format with
color disabled. It verifies individual outcome counts against the harness header
and summary, and ignores captured failure-body output. Do not use `--nocapture`,
`--show-output`, JSON/terse output or combine multiple test binaries into a report.
Unexpected output shapes fail instead of guessing a passing result.

Source guards cover the whole containing file during the prepared execution.
An unrelated edit can preserve a function's interface hash across preparations,
but any file mutation during an execution prevents that receipt from finishing.
