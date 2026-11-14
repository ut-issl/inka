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

use crate::provenance::{self, Provenance};
use crate::schema::{self, Execution, InvalidEvidence, Test, VERSION};

/// Assembles one test evidence v1 file, validating as it goes.
#[derive(Debug, Default)]
pub struct EvidenceBuilder {
    provenance: Provenance,
    tests: Vec<Test>,
    ids: BTreeSet<String>,
}

/// The envelope [`EvidenceBuilder::finish`] serializes, borrowing the
/// builder's state so finishing does not consume or clone it.
#[derive(Serialize)]
struct WireFileRef<'a> {
    v: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    provenance: Option<&'a Provenance>,
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
    ///
    /// # Errors
    ///
    /// Returns [`InvalidEvidence::BlankProvenance`] when the key or the
    /// value is blank: the schema requires its named provenance entries to
    /// be nonblank, and this emitter refuses to write what the schema would
    /// reject, for any key.
    pub fn provenance(
        &mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<(), InvalidEvidence> {
        let key = key.into();
        if !provenance::record(&mut self.provenance, &key, value.into()) {
            return Err(InvalidEvidence::BlankProvenance { key });
        }
        Ok(())
    }

    /// Fills `capsula_run_id` from the `CAPSULA_RUN_ID` environment variable
    /// that `capsula run` exports, when it is set and not already recorded.
    pub fn capsula_env(&mut self) {
        provenance::capsula_env(&mut self.provenance);
    }

    /// Fills `capsula_run_id` with `run_id`, unless it is absent or blank or
    /// an explicit provenance entry already claimed the key.
    pub fn capsula_run_id(&mut self, run_id: Option<String>) {
        provenance::capsula_run_id(&mut self.provenance, run_id);
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
