//! The file-level provenance object both evidence formats share.
//!
//! Provenance is open descriptive metadata — the tested source revision, the
//! capsula run ID — that evaluation never reads. The two builders apply the
//! same rules to it: a blank key or value is refused for any key, because
//! the schemas require their named entries to be nonblank, and the capsula
//! run ID is filled from the environment only when nothing claimed it.

/// The provenance object as the wire carries it.
pub(crate) type Provenance = serde_json::Map<String, serde_json::Value>;

/// The capsula run identifier exported to commands under `capsula run`.
const CAPSULA_RUN_ID_VAR: &str = "CAPSULA_RUN_ID";

/// The provenance key the capsula helpers fill.
const CAPSULA_RUN_ID_KEY: &str = "capsula_run_id";

/// Records one entry, replacing an earlier value under the same key.
/// Returns `false`, recording nothing, when the key or the value is blank.
pub(crate) fn record(provenance: &mut Provenance, key: &str, value: String) -> bool {
    if key.trim().is_empty() || value.trim().is_empty() {
        return false;
    }
    provenance.insert(key.to_owned(), serde_json::Value::String(value));
    true
}

/// Fills `capsula_run_id` from `CAPSULA_RUN_ID`, when set and not already
/// recorded.
pub(crate) fn capsula_env(provenance: &mut Provenance) {
    capsula_run_id(provenance, std::env::var(CAPSULA_RUN_ID_VAR).ok());
}

/// Fills `capsula_run_id` with `run_id`, unless it is absent or blank or an
/// explicit entry already claimed the key.
pub(crate) fn capsula_run_id(provenance: &mut Provenance, run_id: Option<String>) {
    let Some(run_id) = run_id.filter(|id| !id.trim().is_empty()) else {
        return;
    };
    if !provenance.contains_key(CAPSULA_RUN_ID_KEY) {
        provenance.insert(CAPSULA_RUN_ID_KEY.to_owned(), run_id.into());
    }
}
