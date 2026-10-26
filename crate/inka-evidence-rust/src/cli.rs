//! Command-line surface of the adapter.

use std::path::PathBuf;

use clap::Parser;

/// Emits inka test evidence v1 JSON from a cargo-nextest run.
///
/// The adapter lists the tests nextest would run, extracts each test's
/// contract body from the source tree, runs the tests, and writes one
/// evidence file for the whole run. Test failures are evidence, not
/// errors: the exit status is zero whenever the evidence was written.
#[derive(Debug, Parser)]
#[command(name = "inka-evidence-rust", version)]
pub struct Cli {
    /// Where to write the evidence JSON; `-` means standard output.
    #[arg(long, default_value = "-", value_name = "PATH")]
    pub output: PathBuf,

    /// The case ID recorded for this run's execution configuration.
    #[arg(long, default_value = "default", value_name = "CASE")]
    pub case: String,

    /// An extra provenance entry (for example
    /// `source_revision=$GITHUB_SHA`); repeatable.
    #[arg(long, value_name = "KEY=VALUE", value_parser = parse_key_value)]
    pub provenance: Vec<(String, String)>,

    /// Path to the Cargo.toml of the workspace under test.
    #[arg(long, value_name = "PATH")]
    pub manifest_path: Option<PathBuf>,

    /// Arguments forwarded verbatim to `cargo nextest list` and
    /// `cargo nextest run` (filters, `--features`, ...).
    #[arg(last = true, value_name = "NEXTEST_ARGS")]
    pub nextest_args: Vec<String>,
}

/// Splits a `--provenance` entry on its first `=`.
fn parse_key_value(raw: &str) -> Result<(String, String), String> {
    raw.split_once('=')
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .ok_or_else(|| String::from("expected `key=value`"))
}

#[cfg(test)]
mod tests {
    use super::parse_key_value;

    #[test]
    fn splits_on_the_first_equals_sign() {
        assert_eq!(
            parse_key_value("source_revision=abc=def"),
            Ok((String::from("source_revision"), String::from("abc=def")))
        );
    }

    #[test]
    fn rejects_an_entry_without_equals() {
        assert!(parse_key_value("source_revision").is_err());
    }
}
