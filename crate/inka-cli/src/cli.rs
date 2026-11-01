//! Command-line surface: the `inka` binary and the subcommands it dispatches.
//!
//! Only argument parsing lives here. Each subcommand's behavior is a separate
//! module, so the surface stays a thin description of the command tree.

use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;

use clap::{ArgGroup, Parser, Subcommand, ValueEnum};

/// Structured argumentation with accountable approvals.
#[derive(Debug, Parser)]
#[command(name = "inka", version, about)]
pub struct Cli {
    /// When to colour diagnostic output.
    #[arg(long, value_enum, default_value = "auto", global = true)]
    pub color: ColorChoice,

    #[command(subcommand)]
    pub command: Command,
}

/// A top-level `inka` subcommand.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Load a project and report every structural problem in its argument.
    Check {
        /// Project directory holding `inka.toml`; defaults to the current
        /// directory.
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Evaluate the argument: is it currently valid, and if not, why not.
    Eval {
        /// Project directory holding `inka.toml`; defaults to the current
        /// directory.
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Write the evaluation as JSON instead of the human summary.
        #[arg(long)]
        json: bool,
    },
    /// Review approval targets: list the queue, walk it interactively, or
    /// record one decision on one target.
    #[command(group = ArgGroup::new("decision").args(["approve", "decline"]))]
    Review {
        /// Project directory holding `inka.toml`; defaults to the current
        /// directory.
        #[arg(long, default_value = ".")]
        path: PathBuf,
        /// Who is reviewing; records append to `approvals/<approver>.jsonl`.
        #[arg(long = "as", value_name = "APPROVER")]
        approver: String,
        /// The key id records name; defaults to the approver name.
        #[arg(long)]
        key_id: Option<String>,
        /// Show the review queue and exit.
        #[arg(long, conflicts_with_all = ["target", "approve", "decline", "comment"])]
        list: bool,
        /// One block id to record a decision on, with --approve or --decline.
        #[arg(requires = "decision")]
        target: Option<String>,
        /// Record an approval of the target.
        #[arg(long, requires = "target")]
        approve: bool,
        /// Record a decline of the target; a comment is mandatory.
        #[arg(long, requires = "target", requires = "comment")]
        decline: bool,
        /// The comment the record carries; required by --decline.
        #[arg(long, requires = "target")]
        comment: Option<String>,
    },
    /// Write the argument to standard output as cytoscape `elements` JSON.
    Export {
        /// Project directory holding `inka.toml`; defaults to the current
        /// directory.
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Serve the interactive, read-only graph viewer for a project.
    Serve {
        /// Project directory holding `inka.toml`; defaults to the current
        /// directory.
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Address to listen on. Loopback by default: an argument under review
        /// is not published to the network unless that is asked for.
        #[arg(long, default_value_t = IpAddr::V4(Ipv4Addr::LOCALHOST))]
        host: IpAddr,
        /// Port to listen on; 0 lets the operating system choose one.
        #[arg(long, default_value_t = 8080)]
        port: u16,
        /// Directory holding the built viewer bundle; defaults to `web/dist`.
        #[arg(long)]
        assets: Option<PathBuf>,
    },
}

/// When diagnostic snippets should be coloured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ColorChoice {
    /// Colour when standard error is a terminal and `NO_COLOR` is unset.
    Auto,
    /// Always colour, even when the output is redirected or piped.
    Always,
    /// Never colour.
    Never,
}

impl ColorChoice {
    /// Resolves the final colouring decision from this choice, whether a
    /// `NO_COLOR` override is present, and whether the target stream is a
    /// terminal.
    ///
    /// An explicit `always`/`never` wins outright; `auto` honours `NO_COLOR`
    /// first and then the terminal check.
    #[must_use]
    pub fn resolve(self, no_color: bool, is_terminal: bool) -> bool {
        match self {
            ColorChoice::Always => true,
            ColorChoice::Never => false,
            ColorChoice::Auto => !no_color && is_terminal,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn always_and_never_ignore_the_environment() {
        // Arrange & Act & Assert: the explicit choices are unconditional.
        assert!(ColorChoice::Always.resolve(true, false));
        assert!(!ColorChoice::Never.resolve(false, true));
    }

    #[test]
    fn auto_colours_only_a_terminal_without_no_color() {
        assert!(ColorChoice::Auto.resolve(false, true));
        assert!(!ColorChoice::Auto.resolve(true, true)); // NO_COLOR wins
        assert!(!ColorChoice::Auto.resolve(false, false)); // not a terminal
    }
}
