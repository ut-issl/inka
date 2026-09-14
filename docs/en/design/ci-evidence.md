# Test evidence contracts

Japanese translation: [日本語版](../../ja/design/ci-evidence.md).
The [integration design](test-evidence-integration.md) defines workflow-only onboarding and automatic test
publication. Source-side processing is inka-maintained machinery running alongside independently executed
tests.

## Responsibility boundary

A source project selects and executes tests in its own environment. A producer
adapter delivers the reviewable test contract and the corresponding execution
results. GitHub Actions transports the evidence. Inka imports it and evaluates
the imported version against the argument and human approvals, offline.

The source project and argument can be in different repositories. Inka does not
need the source checkout, a compiler, a test runner, or a network connection to
evaluate already imported evidence. Updating upstream alone does not change an
evaluation: the changed evidence must first be imported.

An evidence result means that the selected test executed with that result. It
does not prove that the test is suitable for the proposition. Human bind
approval supplies that judgment.

## Reviewable contract

Both whole-file and individual-test contracts are supported by the interface.
Extraction is a source-side responsibility. Inka sees delivered content, not a
language-specific expression it must resolve or execute.

A contract is a nonempty UTF-8 source string. For a file, it is
the complete file; for an individual test, it is the test definition including
its relevant attributes and test parameters. Each adapter documents its exact
extraction boundary. It must not claim to support syntax it cannot extract
reliably, or guess a function boundary using a line-oriented regular expression.

For example, a Rust proptest contract includes the selected macro test's
parameter strategies and body. A Python contract includes the selected test's
decorators. A JavaScript test callback includes the surrounding test declaration
needed to explain the selected case. Extraction profiles define precise boundaries independently of the parser
or result-capture mechanism.

External helper bodies are not implicitly part of the interface. Capturing changes to execution dependencies
is a separate responsibility. A producer must document context
that its extraction omits, including fixture bodies or dynamically generated
inputs where applicable.

The content must be available for human review, not only represented by a
digest. The consumer exposes that content to the approval interface.

## Content addressing

The interface hash binds the source identity, target identity and reviewable
content. Source content is normalized by replacing CRLF with LF only: a lone CR,
comments, Unicode normalization, indentation and trailing whitespace remain
content. The precise framing, extraction-profile identity and domain tag are
specified by the wire contract, not inferred independently by each adapter.

The bind view hash keeps its frozen v1 layout: the proposition description and
annotations, the raw interface digest, and the currently empty assumption list.
Run IDs, outcomes, environment metadata and closure hashes do not enter the
approval contract. Identity is bound through the provider's interface digest;
existing bind-hash vectors are not rewritten.

Changing the selected source or target requires fresh approval, even when source
text is identical. Renaming only a local routing alias does not change the
underlying source/target identity. A new result for the same source, target and
contract can be evaluated using the existing approval. An adapter must preserve
actual source identity rather than pretending that evidence from two repositories
has the same origin.

## Evidence identity and version

Keep the following concepts distinct:

| Concept | Purpose |
| --- | --- |
| Source identity | Identify the evidence-producing repository/project |
| Target identity | Select one contract within that source |
| Source revision | Identify the version tested, not the argument's revision |
| Execution identity | Distinguish workflow runs and attempts |
| Interface hash | Identify the reviewable contract content |
| Case/environment identity | Identify executions required for this target |

Duplicate local aliases or conflicting resolutions are input errors. Inka must
not choose an arbitrary matching source, silently fall back to another revision,
or look for a successful older run when the selected evidence fails.

Provenance is descriptive metadata in this unsigned protocol. Recomputing a
digest detects inconsistent content; it does not authenticate who ran the test
or prove that a producer reported every execution. Signed attestations provide a separate authentication mechanism.

## Execution coverage and outcomes

The producer explicitly declares the cases and environments required
for a target and reports the outcome of each. A selection must contain at least
one execution. All required executions must pass for the target to pass.

| Observed state | Meaning for the bind |
| --- | --- |
| Every required execution passed | Evidence may establish the bind |
| Any required execution failed | Evidence does not establish the bind |
| A required execution skipped or was cancelled | Evidence does not establish the bind |
| A required execution has no result | Evidence does not establish the bind |
| Conflicting/duplicate records for one execution | Malformed input; abort evaluation |

The manifest of expected executions must not be inferred only from successful
results. A test run that aborts after its first case cannot silently reduce the
required set to the one case that happened to finish. The producer is responsible
for correctly declaring that set; inka cannot independently discover cases in a
source project it does not execute.

Retries and workflow attempts remain distinct. Select an attempt
explicitly rather than combining successes across attempts. Matrix environments
may form one target only with a declared required set and consistent source
revision and contract content. Do not infer individual function pass results
from a whole job's exit code, especially when tests can be filtered or skipped.

Record the content actually associated with execution. If source files mutate
during a run, a post-run hash alone must not relabel the earlier result as a test
of the new content. Capture at the appropriate execution boundary and reject
inconsistent pre/post content, or otherwise document an equally strong binding.

## Evaluation examples

The table assumes the node is not expired and has no live decline. Existing
approval quorum, veto and expiry rules continue to apply.

| Imported evidence and approvals | Evaluation |
| --- | --- |
| File content A, matching approval, all required cases pass | Established |
| Function content A, matching approval, all required cases pass | Established |
| Matching approval, required result fails | Unestablished; evidence failure |
| Matching approval, required case missing or skipped | Unestablished; evidence incomplete/non-passing |
| Passing evidence, no approval | Unestablished; missing approval |
| Content B replaces A; only A was approved | Unestablished; stale approval |
| Same content A, new passing execution | Approval stays valid; established |
| Same content A, new failing execution | Approval stays valid; unestablished |
| Source changed upstream; imported evidence is still A | Same result as before the upstream edit |
| Same content under a different selected source/target | Unestablished until the changed interface is approved |
| Delivered content and its declared digest disagree | Invalid bundle; abort evaluation |
| Referenced evidence is absent | Unestablished; missing evidence |

Report approval and evidence causes independently when enough valid input exists
to compute both. Do not call a stale approval an execution failure, or request
reapproval just because a test started failing with the same interface content.

## Environment metadata

Environment information, including lockfile hashes, is optional and does not
change approval validity or evidence establishment in the initial interface.
It can be recorded without selecting a specific language/package manager.
Warning/error policies for environment changes are future work. A lockfile hash
does not claim to capture the full execution environment.

## Language adapters

Initial adapters target Rust/proptest, Python/pytest and JavaScript/TypeScript
with Vitest. Their extraction boundaries and runner integration are specified
by each framework integration. No adapter may silently broaden a
passing result to tests that were not executed.
