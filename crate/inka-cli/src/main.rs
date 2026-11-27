//! Command-line interface for inka.

mod check;
mod cli;
mod eval;
mod export;
mod fmt;
mod init;
mod load;
mod render;
mod review;
mod serve;

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
        Command::Init { path, name, phases } => {
            let color = color_choice.resolve(no_color, io::stderr().is_terminal());
            run_init(&path, &init::Options { name, phases }, color)
        }
        Command::Check { path } => {
            let color = color_choice.resolve(no_color, io::stderr().is_terminal());
            // ariadne emits a report as many small writes and stderr is
            // unbuffered, so batch them and flush once at the end.
            let mut err = io::BufWriter::new(io::stderr().lock());
            let outcome = check::run(&path, &mut err, color)?;
            err.flush()?;
            Ok(outcome)
        }
        Command::Serve {
            path,
            host,
            port,
            assets,
            approver,
            key_id,
        } => {
            let color = color_choice.resolve(no_color, io::stderr().is_terminal());
            let options = serve::Options {
                host,
                port,
                assets,
                reviewer: approver.map(|approver| serve::ReviewerOptions { approver, key_id }),
            };
            // Unbuffered: the listening address and any warning must reach the
            // operator before the server blocks.
            let mut err = io::stderr().lock();
            serve::run(&path, &options, &mut err, color)
        }
        Command::Eval { path, json } => {
            let color = color_choice.resolve(no_color, io::stderr().is_terminal());
            let mut out = io::BufWriter::new(io::stdout().lock());
            let mut err = io::BufWriter::new(io::stderr().lock());
            let outcome = eval::run(&path, json, &mut out, &mut err, color)?;
            err.flush()?;
            out.flush()?;
            Ok(outcome)
        }
        Command::Review {
            path,
            approver,
            key_id,
            list,
            target,
            approve,
            decline,
            comment,
        } => {
            let color = color_choice.resolve(no_color, io::stderr().is_terminal());
            let action = match target {
                Some(target) if approve => review::Action::Single {
                    target,
                    decision: review::Decision::Approve { comment },
                },
                Some(target) if decline => review::Action::Single {
                    target,
                    decision: review::Decision::Decline {
                        comment: comment.expect("clap makes --decline require --comment"),
                    },
                },
                // clap makes a target require --approve or --decline.
                Some(_) => unreachable!("a target always carries a decision"),
                None if list => review::Action::List,
                None => review::Action::Interactive,
            };
            let request = review::Request {
                approver,
                key_id,
                action,
            };
            run_review(
                &path,
                &request,
                color,
                review_screen(color_choice, no_color),
            )
        }
        Command::Fmt { path, check } => {
            let color = color_choice.resolve(no_color, io::stderr().is_terminal());
            let mut out = io::BufWriter::new(io::stdout().lock());
            let mut err = io::BufWriter::new(io::stderr().lock());
            let outcome = fmt::run(&path, check, &mut out, &mut err, color)?;
            err.flush()?;
            out.flush()?;
            Ok(outcome)
        }
        Command::Export { path } => {
            let color = color_choice.resolve(no_color, io::stderr().is_terminal());
            let mut out = io::BufWriter::new(io::stdout().lock());
            let mut err = io::BufWriter::new(io::stderr().lock());
            let outcome = export::run(&path, &mut out, &mut err, color)?;
            err.flush()?;
            out.flush()?;
            Ok(outcome)
        }
    }
}

/// Runs `inka init`, listing the created paths on standard output.
/// How `inka review` presents its standard output. The queue goes there, so
/// that stream — not the one diagnostics go to — decides its colour and its
/// width.
fn review_screen(color_choice: cli::ColorChoice, no_color: bool) -> review::Screen {
    review::Screen {
        color: color_choice.resolve(no_color, io::stdout().is_terminal()),
        width: console::Term::stdout()
            .size_checked()
            .map_or(review::Screen::DEFAULT_WIDTH, |(_, columns)| {
                usize::from(columns)
            }),
    }
}

/// Runs `inka review` on the process's own terminal streams and clock.
fn run_review(
    path: &std::path::Path,
    request: &review::Request,
    color: bool,
    screen: review::Screen,
) -> io::Result<Outcome> {
    let stdin = io::stdin();
    let input_is_tty = stdin.is_terminal();
    let mut input = stdin.lock();
    let mut now = review::now;
    let session = review::Session {
        input: &mut input,
        input_is_tty,
        now: &mut now,
        screen,
    };
    let mut out = io::BufWriter::new(io::stdout().lock());
    let mut err = io::BufWriter::new(io::stderr().lock());
    let outcome = review::run(path, request, session, &mut out, &mut err, color)?;
    err.flush()?;
    out.flush()?;
    Ok(outcome)
}

fn run_init(path: &std::path::Path, options: &init::Options, color: bool) -> io::Result<Outcome> {
    let mut out = io::BufWriter::new(io::stdout().lock());
    let mut err = io::BufWriter::new(io::stderr().lock());
    let outcome = init::run(path, options, &mut out, &mut err, color)?;
    err.flush()?;
    out.flush()?;
    Ok(outcome)
}
