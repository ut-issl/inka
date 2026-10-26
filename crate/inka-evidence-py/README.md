# inka-evidence (Python)

The Python binding of the shared [inka test
evidence](../../docs/en/design/test-evidence.md) emission layer, plus the
pytest adapter built on top of it. Validation and serialization live in
the Rust crate `inka-evidence`; this package never restates the wire
schema.

## Emission API

```python
from inka_evidence import EvidenceBuilder, Execution

builder = EvidenceBuilder()
builder.capsula_env()
builder.add_test("suite::case", "def test_x(): ...", [Execution("default", "pass")])
text = builder.finish()
```

## pytest adapter

Installing the package registers a pytest plugin (entry point
`inka_evidence`) that stays inert unless asked to emit:

```console
$ pytest --inka-evidence=evidence.json \
    --inka-evidence-provenance source_revision=$(git rev-parse HEAD)
```

The run itself is untouched: test failures keep pytest's own exit status,
and the evidence file is written for every completed session, failures
included. The `CAPSULA_RUN_ID` environment variable that `capsula run`
exports fills the `capsula_run_id` provenance entry unless an explicit
`--inka-evidence-provenance` already claimed it.

### What is declared and what is reported

The required execution set is declared from the finally selected items —
after `-k`/`-m` deselection — never inferred from results:

- a test that ran reports `pass` or `fail` (a setup or teardown error is
  a fail);
- a skipped or xfailed test reports `skip`; a non-strict xpass is a pass
  and a strict xpass a fail, following pytest's own verdict;
- a declared test the session never reported — `-x`, an interrupt —
  keeps a missing outcome, which never establishes a bind;
- a deselected test is not declared at all.

### Test-id construction

```text
<nodeid without the parametrization part>
```

For example `tests/test_hash.py::TestRoundtrip::test_roundtrip`. Each
parametrization is one required execution whose case is the parameter id
(`3-ascii`, ...); an unparametrized test has the single case `default`.
The parameter set is part of the approval interface: adding or removing
parametrizations requires fresh approval.

### Contract bodies

The body is the test function's own definition, decorators included,
exactly as `inspect.getsource` reads it (indentation and trailing newline
preserved). Fixture and helper bodies stay excluded: the contract assigns
dependency capture elsewhere.

### Topology and version limits

- pytest >= 8 on Python >= 3.10.
- pytest-xdist distribution (`-n`/`--dist`) is rejected explicitly; run
  the evidence pass without it.
- Items that are not function-based tests — doctests among them — are
  rejected explicitly rather than guessed at.
- With pytest-rerunfailures, the final attempt is the recorded outcome.
- Collection errors suppress emission entirely: a partially declared set
  would silently narrow the evidence.
- Tests that wrap subprocesses appear only as the outer pytest test.
