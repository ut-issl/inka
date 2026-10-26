//! Python binding over the shared `inka-evidence` emission layer.
//!
//! Exposes the crate's [`inka_evidence::EvidenceBuilder`] and
//! [`inka_evidence::Execution`] to Python test adapters as the
//! `inka_evidence` module. Validation and serialization stay in the Rust
//! crate — this layer only translates types and errors, so the wire schema
//! is never restated in Python. Even the outcome strings are converted
//! through the crate's own serde vocabulary for the same reason.

use pyo3::create_exception;
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

create_exception!(
    inka_evidence,
    InvalidEvidenceError,
    PyValueError,
    "The input would produce evidence the v1 schema rejects."
);

/// Translates a shared-crate defect into the Python-facing exception.
fn invalid(error: &inka_evidence::InvalidEvidence) -> PyErr {
    InvalidEvidenceError::new_err(error.to_string())
}

/// Reads an outcome through the crate's serde vocabulary, so neither the
/// accepted strings nor the error's expected-variant list can drift from
/// the wire format.
fn outcome_of_str(text: &str) -> PyResult<inka_evidence::Outcome> {
    serde_json::from_value(serde_json::Value::String(text.to_owned()))
        .map_err(|error| InvalidEvidenceError::new_err(format!("unknown outcome: {error}")))
}

/// Writes an outcome back through the same vocabulary.
fn outcome_to_string(outcome: inka_evidence::Outcome) -> String {
    match serde_json::to_value(outcome) {
        Ok(serde_json::Value::String(text)) => text,
        _ => unreachable!("an outcome serializes as a JSON string"),
    }
}

/// One declared execution and its observed result. `outcome` is `None` when
/// the execution was declared but no result was reported.
#[pyclass(frozen, module = "inka_evidence")]
struct Execution {
    inner: inka_evidence::Execution,
}

#[pymethods]
impl Execution {
    #[new]
    #[pyo3(signature = (case, outcome=None))]
    fn new(case: String, outcome: Option<&str>) -> PyResult<Self> {
        let outcome = outcome.map(outcome_of_str).transpose()?;
        Ok(Self {
            inner: inka_evidence::Execution::new(case, outcome),
        })
    }

    #[getter]
    fn case(&self) -> &str {
        &self.inner.case
    }

    #[getter]
    fn outcome(&self) -> Option<String> {
        self.inner.outcome.map(outcome_to_string)
    }

    fn __repr__(&self) -> String {
        match self.inner.outcome {
            Some(outcome) => format!(
                "Execution({:?}, {:?})",
                self.inner.case,
                outcome_to_string(outcome)
            ),
            None => format!("Execution({:?})", self.inner.case),
        }
    }
}

/// Assembles one test evidence v1 file, validating as it goes. Invalid input
/// raises `InvalidEvidenceError` and leaves the builder unchanged, so
/// whatever `finish` returns is guaranteed to load.
#[pyclass(module = "inka_evidence")]
#[derive(Default)]
struct EvidenceBuilder {
    inner: inka_evidence::EvidenceBuilder,
}

#[pymethods]
impl EvidenceBuilder {
    #[new]
    fn new() -> Self {
        Self::default()
    }

    /// Records one descriptive provenance entry, replacing an earlier value
    /// under the same key. Rejects a blank key or value.
    fn provenance(&mut self, key: &str, value: &str) -> PyResult<()> {
        self.inner
            .provenance(key, value)
            .map_err(|error| invalid(&error))
    }

    /// Fills `capsula_run_id` from the `CAPSULA_RUN_ID` environment variable
    /// that `capsula run` exports, when it is set and not already recorded.
    fn capsula_env(&mut self) {
        self.inner.capsula_env();
    }

    /// Fills `capsula_run_id` with `run_id`, unless it is `None` or blank or
    /// an explicit provenance entry already claimed the key.
    // The explicit signature keeps `run_id` a required argument; without it
    // pyo3 would let a bare `capsula_run_id()` default the Option to None.
    #[pyo3(signature = (run_id))]
    fn capsula_run_id(&mut self, run_id: Option<String>) {
        self.inner.capsula_run_id(run_id);
    }

    /// Records one test and its declared required executions. Rejects —
    /// leaving the builder unchanged — a blank test or case id, an empty
    /// body, an empty execution list, and a duplicate case or test id.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "pyo3 extracts function arguments as an owned Vec"
    )]
    fn add_test(&mut self, id: &str, body: &str, executions: Vec<Py<Execution>>) -> PyResult<()> {
        let executions = executions
            .iter()
            .map(|execution| execution.get().inner.clone())
            .collect();
        self.inner
            .add_test(id, body, executions)
            .map_err(|error| invalid(&error))
    }

    /// Serializes the file: pretty-printed v1 JSON ending in a newline,
    /// guaranteed to be accepted by the consumer.
    fn finish(&self) -> String {
        self.inner.finish()
    }
}

/// The private extension module; `inka_evidence/__init__.py` re-exports
/// its public names.
#[pymodule(name = "_native")]
fn python_module(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("VERSION", inka_evidence::VERSION)?;
    m.add(
        "InvalidEvidenceError",
        m.py().get_type::<InvalidEvidenceError>(),
    )?;
    m.add_class::<Execution>()?;
    m.add_class::<EvidenceBuilder>()?;
    Ok(())
}
