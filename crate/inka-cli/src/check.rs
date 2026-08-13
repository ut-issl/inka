//! The `inka check` command: load a project, gather every finding across the
//! load and graph stages, render them, and report an outcome.
//!
//! The command is deliberately thin. All judgement lives in the core: loading
//! surfaces syntax, value, and interpolation errors plus any warnings, and
//! `graph::check` adds the structural findings. This module only folds those
//! sources into one rendered batch and a single exit outcome.

use std::io::{self, Write};
use std::path::Path;

use inka_core::graph;
use inka_core::project::{LoadError, LoadOutcome, load_project};
use inka_core::source::{Diagnostics, Error, SourceMap};

use crate::render::render;

/// Result of a check, mapped to a process exit code by the binary entry point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// No errors: the argument is clean or carries only warnings.
    Ok,
    /// At least one error, or a structural failure to load.
    Failed,
}

/// Runs `inka check` against `project_dir`, writing findings to `out`.
///
/// Load warnings are reported whether or not loading succeeded, so one run
/// surfaces everything it saw. The outcome never inspects a severity: a
/// loaded project fails only when the graph stage found errors, and a load
/// failure is a failure by construction.
///
/// `color` is forwarded to the renderer to colour anchored snippets; callers
/// pass `false` when `out` is not a terminal.
///
/// # Errors
///
/// Propagates any I/O error from writing to `out`.
pub fn run(project_dir: &Path, out: &mut impl Write, color: bool) -> io::Result<Outcome> {
    let LoadOutcome { warnings, result } = load_project(project_dir);
    match result {
        Ok(project) => {
            let findings = graph::check(&project);
            let diagnostics = Diagnostics {
                errors: findings.errors,
                warnings: [warnings, findings.warnings].concat(),
            };
            render(&project.source_map, &diagnostics, out, color)?;
            Ok(verdict(&diagnostics))
        }
        Err(LoadError::Invalid { source_map, errors }) => {
            let diagnostics = Diagnostics { errors, warnings };
            render(&source_map, &diagnostics, out, color)?;
            Ok(Outcome::Failed)
        }
        // A structural failure — a missing manifest, an unreadable file —
        // becomes an unanchored error through the same rendering path.
        Err(other) => {
            let diagnostics = Diagnostics {
                errors: vec![Error::unanchored(other.to_string())],
                warnings,
            };
            render(&SourceMap::new(), &diagnostics, out, color)?;
            Ok(Outcome::Failed)
        }
    }
}

/// Verdict for a run whose project loaded: it fails exactly when errors were
/// found. A future `--deny-warnings` policy belongs here and nowhere else.
fn verdict(diagnostics: &Diagnostics) -> Outcome {
    if diagnostics.has_errors() {
        Outcome::Failed
    } else {
        Outcome::Ok
    }
}
