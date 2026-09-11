# Vitest evidence adapter

Implementation status: this prototype does not yet implement the revised
[workflow-only integration contract](../../docs/en/design/test-evidence-integration.md).
Manual plans, explicit producer commands and project reporter/plugin setup
below describe internal/prototype interfaces, not the required onboarding.
Existing test steps must remain independently runnable.

Source-side reporter for JavaScript/TypeScript tests using Vitest 5.0.0 and
TypeScript 5.9.3. The source project installs this adapter and runs its own tests.
The consumer only needs inka and the resulting v2 bundle. Package publication is
separate; this repository currently supplies the adapter source and lockfile.

Create a selection plan (paths relative to the plan):

```json
{
  "v": 1,
  "source": {"id": "github:example/source", "revision": "source-commit"},
  "execution": {"id": "github-actions:123", "attempt": 1},
  "targets": [{"id": "sum", "file": "sum.test.ts", "selector": "math > sums"}]
}
```

Configure the reporter alongside the project's existing reporters:

```js
import InkaReporter from './inka/adapters/vitest/reporter.mjs';

export default {
  test: {
    includeTaskLocation: true,
    reporters: ['default', new InkaReporter({
      plan: 'selection.json', out: '.vitest/inka-run-123-1', environment: 'linux',
      inka: '/path/to/inka',
    })],
  },
};
```

Run `vitest run` using the source project's own environment and configuration.
Then, even after test failure, finish the evidence:

```sh
inka ci-export finish --receipt .vitest/inka-run-123-1/receipt.json \
  --results .vitest/inka-run-123-1/results.json --out evidence.json
```

Use a fresh directory and bundle path for each attempt. A valid failing bundle
is written with exit status 1; an input error writes no new bundle and explains
the problem on stderr. Publish only an existing, validated bundle from this run.

The selector is the exact collected `fullName`, including ` > ` between suites.
The AST extractor includes the static `test`/`it` call, its arguments and inline
callback (`vitest-source:v1`). Helpers, imports and enclosing suite bodies are
outside that contract. Required identity includes environment, file and full
test name. Source snapshots are captured before collection and checked during
preparation and finish. They detect observed file changes, without claiming to
freeze every execution dependency (#39).

Only explicit, uniquely named declarations with source locations are supported.
Dynamic names, external callbacks, aliases, `.each`, `.fails`, retries, repeats,
watch mode and selecting the same file from multiple projects are rejected.
Use a separate run/environment for each project. Pending tests produce missing
results; skipped tests never count as pass. Enclosing hook failures invalidate
the result, and interrupted runs or unhandled errors produce cancelled results.

The implementation uses Vitest's [reporter hooks](https://vitest.dev/guide/advanced/reporters)
and [TestCase API](https://vitest.dev/api/advanced/test-case). These advanced APIs
can change between minor releases, so the initial adapter pins Vitest exactly.

Run integration tests with `INKA_BIN=/path/to/inka npm test` after `npm ci` in this
directory. The tests execute real Vitest tests, including source mutations,
collection errors and teardown failure.
