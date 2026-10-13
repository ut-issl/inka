//! The emission layer: assembles one schema-conformant evidence file.
//!
//! Every input is checked at [`EvidenceBuilder::add_test`] time with the same
//! pass [`parse`](crate::parse) uses, so nothing invalid accumulates and
//! whatever [`EvidenceBuilder::finish`] returns is guaranteed to load. The
//! builder never touches the filesystem — adapters decide where the file
//! lands (typically inside a capsula run, to be captured as an artifact) —
//! and never computes hashes: the interface hash is the consumer's job.

use std::collections::BTreeSet;

use serde::Serialize;

use crate::schema::{self, Execution, InvalidEvidence, Test, VERSION};

/// The capsula run identifier exported to commands under `capsula run`.
const CAPSULA_RUN_ID_VAR: &str = "CAPSULA_RUN_ID";

/// The provenance key [`EvidenceBuilder::capsula_env`] fills.
const CAPSULA_RUN_ID_KEY: &str = "capsula_run_id";

/// Assembles one test evidence v1 file, validating as it goes.
#[derive(Debug, Default)]
pub struct EvidenceBuilder {
    provenance: serde_json::Map<String, serde_json::Value>,
    tests: Vec<Test>,
    ids: BTreeSet<String>,
}

/// The envelope [`EvidenceBuilder::finish`] serializes, borrowing the
/// builder's state so finishing does not consume or clone it.
#[derive(Serialize)]
struct WireFileRef<'a> {
    v: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    provenance: Option<&'a serde_json::Map<String, serde_json::Value>>,
    tests: &'a [Test],
}

impl EvidenceBuilder {
    /// An empty file: no provenance, no tests.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one descriptive provenance entry, replacing an earlier value
    /// under the same key. Provenance is never read by evaluation, and keys
    /// beyond the schema's named ones are allowed.
    pub fn provenance(&mut self, key: impl Into<String>, value: impl Into<String>) {
        self.provenance
            .insert(key.into(), serde_json::Value::String(value.into()));
    }

    /// Fills `capsula_run_id` from the `CAPSULA_RUN_ID` environment variable
    /// that `capsula run` exports, when it is set and not already recorded.
    pub fn capsula_env(&mut self) {
        self.capsula_run_id(std::env::var(CAPSULA_RUN_ID_VAR).ok());
    }

    /// Fills `capsula_run_id` with `run_id`, unless it is absent or blank or
    /// an explicit provenance entry already claimed the key.
    pub fn capsula_run_id(&mut self, run_id: Option<String>) {
        let Some(run_id) = run_id.filter(|id| !id.trim().is_empty()) else {
            return;
        };
        if !self.provenance.contains_key(CAPSULA_RUN_ID_KEY) {
            self.provenance
                .insert(CAPSULA_RUN_ID_KEY.to_owned(), run_id.into());
        }
    }

    /// Records one test and its declared required executions.
    ///
    /// # Errors
    ///
    /// Returns an [`InvalidEvidence`] — leaving the builder unchanged — when
    /// the id or a case is blank, the body is empty, no execution is
    /// declared, a case repeats within the test, or the id repeats across
    /// the file. Nothing invalid can accumulate, so a failed call never
    /// poisons a later [`Self::finish`].
    pub fn add_test(
        &mut self,
        id: impl Into<String>,
        body: impl Into<String>,
        executions: Vec<Execution>,
    ) -> Result<(), InvalidEvidence> {
        let id = id.into();
        let body = body.into();
        schema::check(&id, &body, &executions)?;
        if self.ids.contains(&id) {
            return Err(InvalidEvidence::DuplicateTest { test_id: id });
        }

        self.ids.insert(id.clone());
        self.tests.push(Test {
            id,
            body,
            executions,
        });
        Ok(())
    }

    /// Serializes the file: pretty-printed v1 JSON ending in a newline,
    /// guaranteed to be accepted by [`parse`](crate::parse).
    ///
    /// # Panics
    ///
    /// Never in practice: the builder holds only strings and unit enums,
    /// which `serde_json` serializes infallibly.
    #[must_use]
    pub fn finish(&self) -> String {
        let wire = WireFileRef {
            v: VERSION,
            provenance: (!self.provenance.is_empty()).then_some(&self.provenance),
            tests: &self.tests,
        };
        let mut json = serde_json::to_string_pretty(&wire)
            .expect("the builder only holds serializable strings and enums");
        json.push('\n');
        json
    }
}
