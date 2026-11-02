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
- a test excluded by a name filter (`-t`, `.only`) stays declared and
  reports `skip` (Vitest still collects it); only file-level selection
  removes tests from the declaration entirely;
- a run with unhandled errors outside the tests suppresses emission: the
  recorded outcomes would be untrustworthy.

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
capture elsewhere. The resolved call is cross-checked against the task —
a literal test name must match (each patterns placeholder-wise) — and a
location the slicer cannot resolve or verify is an explicit error. A
dynamic test name (a variable, a template with substitutions) cannot be
cross-checked and is recorded as collected.

### Merging run configurations

The package exports `merge(inputs, options?)` and ships an
`inka-evidence-merge` bin that unions per-configuration evidence files
into one declared set; a `label=file` input prefixes that file's case ids
as `label/case`. Give each Vitest run a distinct `case` option instead
when the case axis is free.

### Topology and version limits

- Vitest >= 5 on Node ^22.18 or >= 24.11 (the `--target nodejs` wasm
  build; `@babel/parser` is loaded through `require(esm)`).
- Vite is pinned to 8.3.4 through `overrides` in this package only: with
  Vitest 5.0.5 and Vite 8.3.5 an error thrown by a reporter crashes
  Vitest's error formatting (`code.charCodeAt is not a function`,
  vitest-dev/vitest#11682), which hides the reporter's explicit
  failures. Drop the override once a Vitest release ships the fix.
- `includeTaskLocation: true` is required; its absence is an explicit
  error.
- The tagged-template form of `each` is rejected explicitly: Vitest
  reports untrustworthy locations for it, which could slice a neighboring
  test's body. Use the array form.
- Watch mode and filtered re-runs are unsupported: every re-run rewrites
  the file, so partial evidence would replace full evidence. Run the
  evidence pass as a one-shot `vitest run`.
- Retries (`retry`) record the final result.
- Multi-project workspaces are supported through the `[project]` prefix;
  an unnamed project in more than one config colliding on the same file
  fails explicitly as a duplicate id.
- `--merge-reports` and blob reports are unsupported (untested paths;
  run the evidence pass as a plain `vitest run`).
- Tests that wrap subprocesses or browsers appear only as the outer
  Vitest test.
