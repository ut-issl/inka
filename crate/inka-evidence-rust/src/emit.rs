//! The adapter pipeline: plan, extract, run, merge, emit.

use std::io::Write;
use std::path::Path;

use inka_evidence::{EvidenceBuilder, Execution, Outcome};

use crate::cli::Cli;
use crate::error::Error;
use crate::nextest::Scope;
use crate::{extract, metadata, nextest};

/// Runs the whole pipeline and writes the evidence file.
///
/// The test-id is `<nextest binary-id>::<test path>`, for example
/// `sample::props::add_commutes`. Should two suites collide once joined —
/// a module in one target shadowing another target's name — the shared
/// builder rejects the duplicate id and the adapter fails explicitly.
pub fn run(cli: &Cli) -> Result<(), Error> {
    let plan = nextest::plan(cli)?;
    let workspace = metadata::load(cli)?;

    // Extract every body before running: a test whose source cannot be
    // extracted is an adapter error, better reported before the run than
    // after the time it takes.
    let mut planned = Vec::new();
    for test in &plan.tests {
        let root = workspace.root_file(
            &test.binary_id,
            &test.package_id,
            &test.kind,
            &test.binary_name,
        )?;
        let test_id = format!("{}::{}", test.binary_id, test.name);
        let body = extract::body(root, &test_id, &test.name)?;
        planned.push((test, test_id, body));
    }

    let report = nextest::run(cli, &plan)?;

    let mut builder = EvidenceBuilder::new();
    builder.capsula_env();
    for (key, value) in &cli.provenance {
        builder.provenance(key.clone(), value.clone())?;
    }
    for (test, test_id, body) in planned {
        // A test the run never reported — aborted or cancelled midway —
        // keeps its declared execution with a missing outcome.
        let outcome = match test.scope {
            Scope::Ignored => Some(Outcome::Skip),
            Scope::Runs => report.outcome(&test.binary_id, &test.name),
        };
        builder.add_test(
            test_id,
            body,
            vec![Execution::new(cli.case.clone(), outcome)],
        )?;
    }

    write_output(&cli.output, &builder.finish())
}

/// Writes the finished evidence to the chosen destination.
fn write_output(output: &Path, evidence: &str) -> Result<(), Error> {
    if output == Path::new("-") {
        return std::io::stdout()
            .write_all(evidence.as_bytes())
            .map_err(|source| Error::Write {
                path: output.to_owned(),
                source,
            });
    }
    std::fs::write(output, evidence).map_err(|source| Error::Write {
        path: output.to_owned(),
        source,
    })
}
