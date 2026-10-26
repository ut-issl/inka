# @ut-issl/inka-evidence

The JavaScript/TypeScript binding of the shared [inka test
evidence](../../docs/en/design/test-evidence.md) emission layer (a wasm
build of the Rust crate `inka-evidence`), plus the Vitest adapter built on
top of it. Validation and serialization live in the Rust crate; this
package never restates the wire schema.

## Emission API

```js
const { EvidenceBuilder } = require("@ut-issl/inka-evidence");

const builder = new EvidenceBuilder();
builder.capsulaRunId(process.env.CAPSULA_RUN_ID);
builder.addTest("suite > case", 'test("x", () => {})', [
  { case: "default", outcome: "pass" },
]);
const text = builder.finish();
```

## Vitest adapter

A custom reporter exported as `@ut-issl/inka-evidence/vitest`. Task
locations must be enabled so contract bodies can be sliced from source:

```ts
export default defineConfig({
  test: {
    includeTaskLocation: true,
    reporters: [
      ["default"],
      [
        "@ut-issl/inka-evidence/vitest",
        { output: "evidence.json", provenance: { source_revision: "..." } },
      ],
    ],
  },
});
```

The run itself is untouched: test failures keep Vitest's own exit status,
and the evidence file is written for every completed run, failures
included. The `CAPSULA_RUN_ID` environment variable that `capsula run`
exports fills the `capsula_run_id` provenance entry unless an explicit
`provenance` entry already claimed it.

### What is declared and what is reported

The required execution set is the run's collected tests, never inferred
from results:

- a test that ran reports `pass` or `fail`;
- a `test.skip` / `test.todo` reports `skip`;
- a declared test the run never reported — `--bail`, an interrupt —
  keeps a missing outcome, which never establishes a bind;
- a test excluded by filters is not declared at all.

Each run records one execution per test, labeled by the `case` option
(default `default`); runs under other configurations are separate
evidence files with distinct case labels, merged later in the delivery
flow. The output path is removed when the run starts and written
atomically at the end, so after a suppressed emission (a test file that
failed to collect, an empty selection) the path is absent — a stale file
can never pass for the run's evidence.

### Test-id construction

```text
[project] <path relative to the vitest root> > <describe chain> > <name>
```

The `[project]` prefix (with its trailing space) appears only when the
project is named, matching
Vitest's own display. Each `test.each` instance is its own test under its
formatted name: changing the table changes the declared set. Dynamically
declared tests (loops, conditions) are declared exactly as collected in
this run; nondeterministic names produce new, unapproved test-ids rather
than silent reuse. Duplicate names at the same position collide and fail
explicitly.

### Contract bodies

The body is the whole test call expression — declaration and callback,
for `test.each` the full chain including the table — sliced verbatim from
the original source at the task's reported location (parsed with
`@babel/parser`; TypeScript and JSX included). Helper functions and
fixtures outside the call stay excluded: the contract assigns dependency
capture elsewhere. A location the slicer cannot resolve to a call
expression is an explicit error.

### Topology and version limits

- Vitest >= 5 on Node (the `--target nodejs` wasm build).
- `includeTaskLocation: true` is required; its absence is an explicit
  error.
- Retries (`retry`) record the final result.
- Multi-project workspaces are supported through the `[project]` prefix;
  an unnamed project in more than one config colliding on the same file
  fails explicitly as a duplicate id.
- `--merge-reports` and blob reports are unsupported (untested paths;
  run the evidence pass as a plain `vitest run`).
- Tests that wrap subprocesses or browsers appear only as the outer
  Vitest test.
