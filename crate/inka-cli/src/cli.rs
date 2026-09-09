//! Command-line surface: the `inka` binary and the subcommands it dispatches.
//!
//! Only argument parsing lives here. Each subcommand's behavior is a separate
//! module, so the surface stays a thin description of the command tree.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

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
    /// Write the argument to standard output as cytoscape `elements` JSON.
    Export {
        /// Project directory holding `inka.toml`; defaults to the current
        /// directory.
        #[arg(default_value = ".")]
        path: PathBuf,
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
