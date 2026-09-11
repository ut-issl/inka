# Test evidence: integration contract and implementation status

Confirmed with the maintainer on 2026-09-11, under [#21](https://github.com/ut-issl/inka/issues/21).
Japanese translation: [日本語版](../../ja/design/test-evidence-integration.md).
This document defines the integration requirements. The [semantic contract](ci-evidence.md)
defines approval and evaluation. Implementation examples must be read together
with the status below; a working prototype is not evidence of meeting these requirements.

## Purpose and terminology

Test evidence connects existing software tests to an argument without requiring
those tests to be modelled as a veriq graph. Veriq represents structured analysis
interfaces and their connections; test evidence supplies reviewable tests and
actual outcomes. CI is an execution context, and GitHub Actions is a collection
and transport mechanism, not a kind of evidence.

The public concept is **test evidence** and the intended DSL keyword is `test:`.
Migrate existing `ci:` references and retire `ci:` after migration; it is not a
permanent alias. Syntax implementation, fixture migration and removal timing
belong to #66/#73. Current implementation branches still accept `ci:`; do not
present `test:` examples as executable before that implementation lands.
A terminology change alone must not alter frozen hash bytes, record layouts or
bundle versions. Internal versioned identifiers containing `ci` are protocol
identifiers, not a reason to invalidate approvals.

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

## Agreements retained

Both file and function contracts are supported. Initial frameworks remain
Rust/libtest/proptest, Python/pytest and JavaScript/TypeScript/Vitest; initial
Actions support Linux. Exact extraction profiles include relevant attributes or
decorators and test declarations. External helper/fixture bodies are outside the
function contract; execution-dependency coverage remains #39.

Source ID, target ID, kind, extraction profile, CRLF-only normalized content and
the required-case set determine the interface hash. Identity or required-set
changes require reapproval. Outcome-only changes do not. Bind establishment
requires both valid approval and passing evidence. Invalid evidence is a CLI
input error; valid missing/nonpassing evidence is bind blame.

Evaluation uses the imported version offline. No upstream checkout, test runner
or network is required. Unimported upstream edits have no effect. Environment
and lockfile metadata remain optional. Signatures (#38), review CLI (#27), auto
synchronization and dependency-closure policies remain separate work.

## Delivery plan and current implementation status

| Issue | PR | Disposition and remaining work |
| --- | --- | --- |
| #65 | #76 | Retain: this integration contract and evidence semantics |
| #66 | #77 | Retain Draft: v2 format; add `test:` migration and catalog identity examples |
| #72 | #79 | Retain Draft: offline loading/validation; align terminology as needed |
| #73 | #80 | Retain Draft: approval/evidence evaluation; implement syntax migration |
| #67 | #81 | Retain Draft: prepare/finish primitives are internal machinery, not onboarding |
| #68 | #82 | Close without merge: replace explicit selection with automatic Rust collection |
| #69 | #83 | Close without merge: provide workflow-only pytest integration and discovery |
| #70 | #84 | Close without merge: remove required project reporter edits; automatic collection |
| #71 | #85 | Close without merge: implement preparation/collection and tool setup, not only upload |
| #74 | #86 | Retain Draft: receiver import is reusable; update source integration and tool revision |
| #75 | #87 | Close without merge: demonstrate reuse of an existing independent test job |

Closed implementations may supply reusable parser/validation code, but are not
the replacement design. Their issues stay open. Retained code PRs are not
merge-ready merely because prototype tests passed. Rebuild the dependency chain
before merging descendants of closed PRs; do not merge a closed layer transitively.

Logical dependencies: #65 -> #66 -> #67 and #72; #67 -> #68/#69/#70;
Issues #72 -> #73; #71 uses #66/#67 and a supported framework integration;
Issues #71 + #73 -> #74; #68 + #71 + #73 + #74 -> #75.
Design the Action-facing integration before implementing framework onboarding;
framework feasibility work can proceed before the complete Action is finished.

Dogfood retains the broad `local_view_hash_sound` fiat and adds only a narrowly
scoped normalization-idempotence bind. Synthetic approvals belong only in tests.
The existing prototype passed a same-repository artifact round trip; it did not
validate this revised onboarding experience or a distinct receiver repository.

## Decisions still required before implementation

The product requirements above are confirmed. Exact Action names/inputs and
packaging, per-framework report/hook mechanisms, stable automatic target/case ID
rules, supported execution topologies (including containers/multiple binaries),
and the `ci:` removal release/timing require concrete proposals and maintainer
confirmation. Do not silently decide them from prototype behavior. Preserve these
questions in the responsible issues (#66, #68–#71) as implementation progresses.
