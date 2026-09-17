# Test evidence integration

Japanese translation: [日本語版](../../ja/design/test-evidence-integration.md).
The [evidence contract](test-evidence.md) defines approval and evaluation; this document defines how existing
tests supply that evidence.

## Purpose and terminology

Test evidence connects existing software tests to an argument without requiring
those tests to be modelled as a veriq graph. Veriq represents structured analysis
interfaces and their connections; test evidence supplies reviewable tests and
actual outcomes. CI is an execution context, and GitHub Actions is a collection
and transport mechanism, not a kind of evidence.

The provider is called **test evidence**, selected with the DSL keyword `test:`.
Versioned protocol identifiers and hash framing are stable independently of display names.

## Required user experience

The test author writes ordinary framework tests. No inka-specific decorators,
per-test registration, hand-written selection plans or changes to the project's
test configuration files are required for the supported integration path.

The CI maintainer performs a one-time workflow integration. Adding preparation
and collection Actions and minimal report-output options/environment settings
inside the workflow is acceptable. Existing test commands remain independently
runnable steps and continue to determine the normal CI pass/fail result.
They do not have to execute inside an inka Action. Existing environment setup
remains owned by the source project; inka must not require a second test run
as the normal integration path.

Automatically publish all tests collected by the existing command. New tests
within its collection scope require no inka-specific registration. The argument
author chooses which published target to bind. Source-side publication filters
are optional; they do not change which tests the existing CI executes.
Publishing a test does not approve it or add it to an argument automatically.

## Collection paths

Prefer reusing an existing report when it provides sufficient case identity,
expected-case coverage, outcomes and a reliable association to tested source.
If it does not, enable report output or a framework integration in the workflow.
An inka-provided preparation Action may capture source and configure collection
before the ordinary test step; a collection Action consumes the results afterward.
Framework-specific machinery is maintained by inka and runs on the source runner.
The consumer never needs the source test environment.

Collection may happen in a later step of the existing job, or in a separate job
using explicitly transferred reports and source snapshots/receipts. A separate
job cannot assume access to another job's workspace. Job success/failure alone
cannot establish individual tests. Logs or generic reports are usable only when
their actual information meets the contract; no universal conversion is promised.

A wrapper that executes a command may be offered as an optional convenience,
but it cannot be the only supported integration. An explicitly requested separate
execution may produce evidence when reuse is impossible; identify it as its own
execution, never as evidence from the original CI run. Do not rerun silently.

## Coverage, source and failure rules

- Enumerate collected cases independently of successful results. An early abort
  cannot shrink the expected set to the cases that finished.
- Require all cases/environments declared for a target to pass. Skip, cancellation
  and missing outcomes remain nonpassing. Preserve run, attempt and environment
  identities; never combine successful retries implicitly.
- Bind results to the source actually associated with execution. Capture it
  beforehand or establish an equivalent link. A checkout after execution is not
  by itself proof of what was executed; reject insufficient or inconsistent input.
- Collect failing results without changing the existing test step's failure.
  Report collection/input failures explicitly. Runner loss or hard cancellation
  may prevent artifact publication; missing evidence must not become success.
- Scope the automatic catalog to the command's collected tests, not every test
  that could exist in the repository. Optional publication filters and runner
  selection are distinct. Required-set changes for an existing target still
  require reapproval; adding an unrelated target does not change that target's hash.
- Unsupported or ambiguous mappings must not silently disappear from the promised
  publication scope or become pass evidence. Document the limitation and surface
  an error; an explicit publication filter can narrow the supported scope.

## Evidence semantics

Both file and function contracts are supported. Supported framework integrations cover
Rust/libtest/proptest, Python/pytest and JavaScript/TypeScript/Vitest; initial
Actions support Linux. Exact extraction profiles include relevant attributes or
decorators and test declarations. External helper/fixture bodies are outside the
function contract; execution-dependency coverage is a separate concern.

Source ID, target ID, kind, extraction profile, CRLF-only normalized content and
the required-case set determine the interface hash. Identity or required-set
changes require reapproval. Outcome-only changes do not. Bind establishment
requires both valid approval and passing evidence. Invalid evidence is a CLI
input error; valid missing/nonpassing evidence is bind blame.

Evaluation uses the imported version offline. No upstream checkout, test runner
or network is required. Unimported upstream edits have no effect. Environment
and lockfile metadata remain optional. Signatures, approval interfaces, auto
synchronization and dependency-closure policies remain separate work.

## Validation principles

Integration tests exercise an independent existing test step, automatic publication of newly collected tests,
source/result correspondence, preservation of test failure and offline evaluation after transport. Distinguish
same-job collection, cross-job artifact transfer and cross-repository import.

A property test supports only its scoped proposition, not general hash soundness. Use synthetic approvals only
inside tests; publishing evidence never grants human approval.
