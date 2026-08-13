//! Command-line interface for inka.

mod check;
mod cli;
mod render;

use std::io::{self, IsTerminal, Write};
use std::process::ExitCode;

use clap::Parser;

use crate::check::Outcome;
use crate::cli::{Cli, Command};

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(Outcome::Ok) => ExitCode::SUCCESS,
        Ok(Outcome::Failed) => ExitCode::FAILURE,
        Err(error) => {
            // Failing to even write diagnostics is itself a hard error.
            let _ = writeln!(io::stderr(), "error: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Dispatches a parsed command, writing any diagnostics to standard error.
fn run(cli: Cli) -> io::Result<Outcome> {
    // `NO_COLOR` counts only when present and non-empty, matching the wider
    // ecosystem's reading of the convention.
    let no_color = std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty());
    let color_choice = cli.color;
    match cli.command {
        Command::Check { path } => {
            let color = color_choice.resolve(no_color, io::stderr().is_terminal());
            // ariadne emits a report as many small writes and stderr is
            // unbuffered, so batch them and flush once at the end.
            let mut err = io::BufWriter::new(io::stderr().lock());
            let outcome = check::run(&path, &mut err, color)?;
            err.flush()?;
            Ok(outcome)
        }
    }
}
