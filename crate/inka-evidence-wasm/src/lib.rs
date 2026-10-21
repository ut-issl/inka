//! TypeScript (wasm) binding over the shared `inka-evidence` emission layer.
//!
//! Exposes the crate's [`inka_evidence::EvidenceBuilder`] to JavaScript test
//! adapters. Validation and serialization stay in the Rust crate — this
//! layer only translates types and errors, so the wire schema is never
//! restated at runtime: executions arrive as plain `{ case, outcome? }`
//! objects and deserialize through the crate's own serde definitions,
//! unknown fields and outcome spellings included.
//!
//! Unlike the Python binding, `capsula_env` is deliberately not exposed:
//! wasm has no process environment, so adapters read it themselves and pass
//! `process.env.CAPSULA_RUN_ID` to [`EvidenceBuilder::capsula_run_id`].

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

/// The `name` carried by errors raised for semantic evidence defects.
const INVALID_EVIDENCE: &str = "InvalidEvidenceError";

/// Translates a shared-crate defect into a JS `Error` whose `name` is
/// `InvalidEvidenceError`, so adapters can tell it from a type mistake.
fn invalid(error: &inka_evidence::InvalidEvidence) -> JsValue {
    let js = js_sys::Error::new(&error.to_string());
    js.set_name(INVALID_EVIDENCE);
    js.into()
}

/// Wraps a `JSON.stringify` exception (a `BigInt`, a cycle, a throwing
/// `toJSON`) in a `TypeError` that keeps the original message, so the
/// adapter bug stays diagnosable.
fn not_serializable(cause: &JsValue) -> js_sys::TypeError {
    let message = cause.dyn_ref::<js_sys::Error>().map_or_else(
        || String::from("unknown error"),
        |error| String::from(error.message()),
    );
    js_sys::TypeError::new(&format!("executions is not JSON-serializable: {message}"))
}

#[wasm_bindgen(typescript_custom_section)]
const EXECUTION_INIT: &'static str = r#"
/**
 * One declared execution: its case id and, when a result was reported, the
 * observed outcome. Omit `outcome` for a declared-but-missing result (at
 * runtime a `null` outcome is read the same way). The runtime vocabulary is
 * the `inka-evidence` crate's; this type mirrors it for editor support.
 */
export interface ExecutionInit {
    case: string;
    outcome?: "pass" | "fail" | "skip" | "cancelled";
}
"#;

#[wasm_bindgen]
extern "C" {
    /// A plain array of `ExecutionInit` objects, deserialized through the
    /// shared crate's serde definitions.
    #[wasm_bindgen(typescript_type = "ReadonlyArray<ExecutionInit>")]
    pub type ExecutionInitArray;
}

/// Assembles one test evidence v1 file, validating as it goes. Invalid
/// input throws an `Error` named `InvalidEvidenceError` and leaves the
/// builder unchanged, so whatever `finish` returns is guaranteed to load.
#[wasm_bindgen]
#[derive(Default)]
pub struct EvidenceBuilder {
    inner: inka_evidence::EvidenceBuilder,
}

#[wasm_bindgen]
impl EvidenceBuilder {
    /// An empty file: no provenance, no tests.
    #[wasm_bindgen(constructor)]
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records one descriptive provenance entry, replacing an earlier value
    /// under the same key. Rejects a blank key or value.
    ///
    /// # Errors
    ///
    /// Throws an `InvalidEvidenceError` when the key or the value is blank.
    pub fn provenance(&mut self, key: &str, value: &str) -> Result<(), JsValue> {
        self.inner
            .provenance(key, value)
            .map_err(|error| invalid(&error))
    }

    /// Fills `capsula_run_id` with `runId` — typically
    /// `process.env.CAPSULA_RUN_ID`, which `capsula run` exports — unless it
    /// is `undefined` or blank or an explicit provenance entry already
    /// claimed the key.
    #[wasm_bindgen(js_name = capsulaRunId)]
    pub fn capsula_run_id(&mut self, run_id: Option<String>) {
        self.inner.capsula_run_id(run_id);
    }

    /// Records one test and its declared required executions, given as plain
    /// `{ case, outcome? }` objects.
    ///
    /// A `null` outcome is treated exactly like an omitted one: a declared
    /// result that is missing. `JSON.stringify` already folds the JS-only
    /// non-values (`NaN` becomes `null`, an `undefined` property vanishes),
    /// and the wire deserializer reads both spellings as "no result".
    ///
    /// # Errors
    ///
    /// Throws a `TypeError` when `executions` does not serialize to JSON
    /// text (`undefined`, a function, a `BigInt`, a cycle) or is not an
    /// array of objects the wire format's serde definitions accept (an
    /// unknown outcome spelling or a stray field included), and an
    /// `InvalidEvidenceError` on a blank test or case id, an empty body, an
    /// empty execution list, or a duplicate case or test id. Every error
    /// leaves the builder unchanged and usable.
    #[wasm_bindgen(js_name = addTest)]
    #[expect(
        clippy::needless_pass_by_value,
        reason = "wasm-bindgen moves extern-typed arguments in by value"
    )]
    pub fn add_test(
        &mut self,
        id: &str,
        body: &str,
        executions: ExecutionInitArray,
    ) -> Result<(), JsValue> {
        // The round trip through JSON text deliberately applies the exact
        // wire-format deserializer, `deny_unknown_fields` included, which a
        // field-by-name reader over the live JS object could not enforce.
        // `JSON.stringify` returns `undefined` (not an error) for a value
        // with no JSON form, so both the Err and the non-string Ok must map
        // to a TypeError; unwrapping the Ok blindly would abort in wasm and
        // leave the builder's `&mut self` borrow poisoned forever.
        let text = js_sys::JSON::stringify(executions.as_ref())
            .map_err(|cause| not_serializable(&cause))?
            .as_string()
            .ok_or_else(|| {
                js_sys::TypeError::new(
                    "executions has no JSON form (undefined, a function or a symbol)",
                )
            })?;
        let executions: Vec<inka_evidence::Execution> = serde_json::from_str(&text)
            .map_err(|error| js_sys::TypeError::new(&format!("invalid executions: {error}")))?;
        self.inner
            .add_test(id, body, executions)
            .map_err(|error| invalid(&error))
    }

    /// Serializes the file: pretty-printed v1 JSON ending in a newline,
    /// guaranteed to be accepted by the consumer.
    #[must_use]
    pub fn finish(&self) -> String {
        self.inner.finish()
    }
}

/// The evidence format version this binding writes.
///
/// # Panics
///
/// Never in practice: the crate's version constant fits in a `u32`.
#[wasm_bindgen]
#[must_use]
pub fn version() -> u32 {
    u32::try_from(inka_evidence::VERSION).expect("the format version fits in a u32")
}
