//! inka-evidence-rust: emit inka test evidence from a cargo-nextest run.
//!
//! The adapter runs on the source repository's runner, typically inside
//! `capsula run`. It declares the required execution set from
//! `cargo nextest list`, extracts each test's contract body from the
//! source tree, runs the tests, and writes one test-evidence v1 file.
//! See this crate's README for the test-id construction and the supported
//! topologies.

mod cli;
mod emit;
mod error;
mod extract;
mod junit;
mod metadata;
mod nextest;

use std::io::{self, Write};
use std::process::ExitCode;

use clap::Parser;

fn main() -> ExitCode {
    match emit::run(&cli::Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // Failing to even write diagnostics is itself a hard error.
            let _ = writeln!(io::stderr(), "error: {error}");
            ExitCode::FAILURE
        }
    }
}
