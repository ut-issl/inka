# inka pytest adapter

Implementation status: this prototype does not yet implement the revised
[workflow-only integration contract](../../docs/en/design/test-evidence-integration.md).
Manual plans, explicit producer commands and project reporter/plugin setup
below describe internal/prototype interfaces, not the required onboarding.
Existing test steps must remain independently runnable.

Install this directory in the **source project's** environment:

```sh
python -m pip install ./adapters/pytest
```

The plugin is inactive unless `--inka-plan` is supplied. The inka CLI must also
be available, via `--inka-bin` or `INKA_BIN`. For development without installing
the plugin, put this directory on `PYTHONPATH` and load `-p inka_pytest`.

A selection plan specifies the actual source revision and execution attempt:

```json
{
  "v": 1,
  "source": { "id": "github:example/python", "revision": "SOURCE_COMMIT" },
  "execution": { "id": "github-actions:RUN_ID", "attempt": 1 },
  "targets": [{ "id": "positive-values", "file": "tests/test_values.py", "selector": "test_values" }]
}
```

Paths are relative to the plan. Select methods as `TestClass::test_method`.
The source-side runner collects the required parameterized cases and snapshots
the decorated function with Python AST spans. It delegates hashing/preparation
to the common CLI and observes setup, call and teardown results separately.

```sh
pytest --inka-plan selection.json --inka-output evidence-run --inka-environment linux
inka ci-export finish --receipt evidence-run/receipt.json --results evidence-run/results.json --out ci-export.json
```

Use a fresh output directory per run. In CI, execute finish even after pytest
fails while preserving the test step's failure. Finish writes valid non-passing
evidence and exits nonzero. It refuses source files changed after capture.

`pytest-source:v1` includes the function/method definition and decorators, not
fixture bodies, parent-class definitions, imports or external helper bodies.
Parameterized cases that remain after explicit runner selection are all required;
changing that selection changes the interface hash and requires reapproval.
Early termination leaves required cases unreported. Skip/xfail cannot become pass,
and teardown failure overrides a passing call. A selected function with no
collected cases is an explicit preparation error.

The initial adapter requires single-process pytest, without xdist, rerun plugins
or externally wrapped/generated test functions. Duplicate stage reports cannot
produce passing evidence. Unsupported selectors and ambiguous definitions fail
explicitly. This is source-side extraction, not execution-closure capture.

Run the adapter's real-runner tests with an environment containing pytest:

```sh
cargo build -p inka-cli --locked
INKA_BIN="$PWD/target/debug/inka" python -m unittest discover -s adapters/pytest/tests -v
```
