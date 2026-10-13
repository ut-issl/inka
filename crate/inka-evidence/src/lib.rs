//! The test evidence v1 wire format, shared by producers and the consumer.
//!
//! `docs/en/design/test-evidence.md` fixes the contract and
//! `schemas/test-evidence-v1.schema.json` the shape. This crate is the single
//! implementation of both sides of that boundary: [`parse`] is the strict
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
mod schema;

pub use builder::EvidenceBuilder;
pub use schema::{Execution, InvalidEvidence, Outcome, ParseError, Test, VERSION, parse};
