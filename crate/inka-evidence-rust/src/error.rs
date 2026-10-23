//! Adapter-level errors.
//!
//! Test failures are not errors: they are evidence, recorded as `fail`
//! outcomes. An [`Error`] is a case where the evidence itself cannot be
//! produced faithfully — the adapter fails explicitly instead of guessing,
//! as the contract requires for unsupported or ambiguous mappings.

use std::path::PathBuf;
use std::process::ExitStatus;

/// Anything that stops the adapter from emitting evidence.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// A `cargo` invocation could not be spawned at all.
    #[error("failed to run `{command}`: {source}")]
    Spawn {
        command: String,
        #[source]
        source: std::io::Error,
    },

    /// A `cargo` invocation ran and reported failure (its own diagnostics
    /// have already reached standard error).
    #[error("`{command}` failed with {status}")]
    CommandFailed { command: String, status: ExitStatus },

    /// `cargo nextest list` or `cargo metadata` emitted JSON this adapter
    /// does not understand.
    #[error("unexpected {what} output: {source}")]
    Format {
        what: &'static str,
        #[source]
        source: serde_json::Error,
    },

    /// The run produced no `JUnit` report where this adapter expects it.
    #[error(
        "no JUnit report at {}: a repository nextest configuration that \
         overrides `junit.path` is not supported", path.display()
    )]
    JunitMissing { path: PathBuf },

    /// The `JUnit` report exists but could not be read as expected.
    #[error("malformed JUnit report at {}: {detail}", path.display())]
    JunitMalformed { path: PathBuf, detail: String },

    /// A file the adapter needs could not be read.
    #[error("failed to read {}: {source}", path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// A source file did not parse as Rust.
    #[error("failed to parse {}: {source}", path.display())]
    Parse {
        path: PathBuf,
        #[source]
        source: syn::Error,
    },

    /// A listed suite has no matching target in `cargo metadata`.
    #[error("no cargo target for nextest suite `{binary_id}` (kind `{kind}`, name `{name}`)")]
    TargetNotFound {
        binary_id: String,
        kind: String,
        name: String,
    },

    /// A listed test could not be located in the source tree, or its shape
    /// is one this adapter does not support (for example, a test generated
    /// by a macro other than `proptest!`).
    #[error("cannot extract the contract body of `{test_id}`: {detail}")]
    BodyNotFound { test_id: String, detail: String },

    /// The assembled evidence violates the wire contract — for example two
    /// suites whose ids collide once joined into test-ids.
    #[error(transparent)]
    Invalid(#[from] inka_evidence::InvalidEvidence),

    /// The evidence file could not be written.
    #[error("failed to write {}: {source}", path.display())]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}
