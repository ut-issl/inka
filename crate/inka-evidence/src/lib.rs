//! The evidence wire formats, shared by producers and the consumer.
//!
//! `docs/en/design/test-evidence.md` fixes the test evidence contract and
//! `schemas/test-evidence-v1.schema.json` the shape; the [`calc`] module does
//! the same for calc evidence (`docs/en/design/calc-evidence.md`,
//! `schemas/calc-evidence-v1.schema.json`). This crate is the single
//! implementation of both sides of each boundary: [`parse`] is the strict
//! reader inka's consumer builds on, and [`EvidenceBuilder`] is the emission
//! layer adapters build on. Because both share one validation pass, an
//! adapter cannot emit a file the consumer rejects, and no other language
//! needs to restate the schema — Python and TypeScript adapters call this
//! crate through bindings instead of assembling JSON by hand.
//!
//! Deliberately absent: file I/O (the caller decides where evidence lands),
//! interface hashing (the consumer's job, in `inka-core`), test execution and
//! language-specific extraction (each adapter's job).

mod builder;
pub mod calc;
mod merge;
mod provenance;
mod schema;

pub use builder::EvidenceBuilder;
pub use merge::{MergeError, MergeInput, merge};
pub use schema::{Execution, InvalidEvidence, Outcome, ParseError, Test, VERSION, parse};
