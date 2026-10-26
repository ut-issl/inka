//! inka-evidence-merge: merge per-configuration evidence files into one.
//!
//! Language-agnostic: the inputs are test-evidence v1 JSON files from any
//! adapter. Each input may carry a label (`label=file.json`), which
//! prefixes its case ids as `label/case`; see the `inka-evidence` crate's
//! merge module for the exact semantics.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::Parser;
use inka_evidence::{MergeInput, merge};

/// Merges per-configuration evidence files into one evidence file.
///
/// Inputs are `[LABEL=]FILE` pairs; a label prefixes every case id of its
/// file as `label/case`. A part before `=` containing a path separator is
/// read as part of the file name, not as a label.
#[derive(Debug, Parser)]
#[command(name = "inka-evidence-merge", version)]
struct Cli {
    /// Where to write the merged evidence JSON; `-` means standard output.
    #[arg(long, default_value = "-", value_name = "PATH")]
    output: PathBuf,

    /// An extra provenance entry for the merged file; repeatable.
    #[arg(long, value_name = "KEY=VALUE", value_parser = parse_key_value)]
    provenance: Vec<(String, String)>,

    /// The evidence files to merge, each optionally labeled.
    #[arg(required = true, value_name = "[LABEL=]FILE")]
    inputs: Vec<String>,
}

/// Splits a `--provenance` entry on its first `=`.
fn parse_key_value(raw: &str) -> Result<(String, String), String> {
    raw.split_once('=')
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .ok_or_else(|| String::from("expected `key=value`"))
}

/// Splits one `[LABEL=]FILE` argument. The part before the first `=` is a
/// label only when it contains no path separator, so a file name with an
/// `=` still works when written with a leading `./`.
fn split_input(raw: &str) -> (Option<&str>, &str) {
    match raw.split_once('=') {
        Some((label, file)) if !label.contains('/') && !label.contains('\\') => (Some(label), file),
        _ => (None, raw),
    }
}

fn run(cli: &Cli) -> Result<(), String> {
    let mut inputs = Vec::new();
    for raw in &cli.inputs {
        let (label, file) = split_input(raw);
        let text = std::fs::read_to_string(file)
            .map_err(|error| format!("cannot read {file}: {error}"))?;
        inputs.push(MergeInput {
            label: label.map(str::to_owned),
            text,
        });
    }
    let run_id = std::env::var("CAPSULA_RUN_ID").ok();
    let merged = merge(&inputs, &cli.provenance, run_id).map_err(|error| error.to_string())?;
    if cli.output == Path::new("-") {
        io::stdout()
            .write_all(merged.as_bytes())
            .map_err(|error| format!("cannot write to standard output: {error}"))
    } else {
        std::fs::write(&cli.output, merged)
            .map_err(|error| format!("cannot write {}: {error}", cli.output.display()))
    }
}

fn main() -> ExitCode {
    match run(&Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            // Failing to even write diagnostics is itself a hard error.
            let _ = writeln!(io::stderr(), "error: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::split_input;

    #[test]
    fn splits_a_label_from_a_file() {
        assert_eq!(split_input("linux=ev.json"), (Some("linux"), "ev.json"));
    }

    #[test]
    fn a_plain_file_has_no_label() {
        assert_eq!(split_input("ev.json"), (None, "ev.json"));
    }

    #[test]
    fn a_path_separator_before_the_equals_means_no_label() {
        assert_eq!(split_input("./odd=name.json"), (None, "./odd=name.json"));
    }
}
