//! The `inka check` command: load a project, gather every diagnostic across
//! the load and graph stages, render them, and report an outcome.
//!
//! The command is deliberately thin. All judgement lives in the core: loading
//! surfaces syntax, value, and interpolation diagnostics, and `graph::check`
//! adds the structural ones. This module only decides how a project directory
//! maps to those two sources and folds the result into a single exit outcome.

use std::io::{self, Write};
use std::path::Path;

use inka_core::graph;
use inka_core::project::{ProjectError, load_project};
use inka_core::source::{Diagnostic, Severity, SourceMap};

use crate::render::render;

/// Result of a check, mapped to a process exit code by the binary entry point.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// No errors: the argument is clean or carries only warnings.
    Ok,
    /// At least one error diagnostic, or a structural failure to load.
    Failed,
}

/// Runs `inka check` against `project_dir`, writing diagnostics to `out`.
///
/// Load-stage diagnostics (syntax, values, interpolation) and graph-stage
/// diagnostics (duplicate ids, dangling references, cycles, reachability) are
/// rendered through the same path. A structural load failure that produces no
/// diagnostics — a missing manifest, an unreadable file — is reported as a
/// single error line.
///
/// `color` is forwarded to the renderer to colour anchored snippets; callers
/// pass `false` when `out` is not a terminal.
///
/// # Errors
///
/// Propagates any I/O error from writing to `out`.
pub fn run(project_dir: &Path, out: &mut impl Write, color: bool) -> io::Result<Outcome> {
    match load_project(project_dir) {
        Ok(project) => report(&project.source_map, &graph::check(&project), out, color),
        Err(ProjectError::Diagnostics {
            source_map,
            diagnostics,
        }) => report(&source_map, &diagnostics, out, color),
        Err(other) => {
            writeln!(out, "error: {other}")?;
            Ok(Outcome::Failed)
        }
    }
}

/// Renders a batch of diagnostics and derives the outcome from their severity.
fn report(
    source_map: &SourceMap,
    diagnostics: &[Diagnostic],
    out: &mut impl Write,
    color: bool,
) -> io::Result<Outcome> {
    render(source_map, diagnostics, out, color)?;
    let has_error = diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error);
    Ok(if has_error {
        Outcome::Failed
    } else {
        Outcome::Ok
    })
}
