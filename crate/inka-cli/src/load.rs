//! Loading a project into the shared view, with every failure reported the way
//! `inka check` reports it.
//!
//! Commands that render an argument — `inka export` today, the report and the
//! server next — all need the same thing: a project that loads, a graph that
//! builds, and a shape that validates. Doing that once here keeps them from
//! disagreeing about what counts as an exportable argument, and keeps their own
//! modules to the formatting they exist for.
//!
//! The bar is deliberately `inka check`'s: errors refuse, warnings do not. An
//! argument that fails the check would export as something no consumer expects
//! — a graph with a cycle, or one with no objective to hang from — and every
//! renderer downstream assumes those away.

use std::io::{self, Write};
use std::path::Path;

use inka_core::graph::Graph;
use inka_core::project::{LoadError, LoadOutcome, load_project};
use inka_core::source::{Diagnostics, Error, SourceMap, Warning};
use inka_core::view::ArgumentView;

use crate::render::render;

/// Loads and projects the project rooted at `project_dir`.
///
/// Returns `None` when the argument cannot be rendered, having already written
/// the reason to `err`. Warnings are written whether or not the load succeeded,
/// so one run surfaces everything it saw.
///
/// # Errors
///
/// Propagates any I/O error from writing to `err`.
pub fn load_view(
    project_dir: &Path,
    err: &mut impl Write,
    color: bool,
) -> io::Result<Option<ArgumentView>> {
    let LoadOutcome { warnings, result } = load_project(project_dir);
    let project = match result {
        Ok(project) => project,
        Err(LoadError::Invalid { source_map, errors }) => {
            return report(&source_map, errors, warnings, err, color);
        }
        // A structural failure — a missing manifest, an unreadable file —
        // becomes an unanchored error through the same rendering path.
        Err(other) => {
            return report(
                &SourceMap::new(),
                vec![Error::unanchored(other.to_string())],
                warnings,
                err,
                color,
            );
        }
    };

    let graph = match Graph::build(&project.blocks) {
        Ok(graph) => graph,
        Err(errors) => return report(&project.source_map, errors, warnings, err, color),
    };

    let shape = graph.validate();
    if shape.has_errors() {
        return report(
            &project.source_map,
            shape.errors,
            [warnings, shape.warnings].concat(),
            err,
            color,
        );
    }

    let diagnostics = Diagnostics {
        errors: Vec::new(),
        warnings: [warnings, shape.warnings].concat(),
    };
    render(&project.source_map, &diagnostics, err, color)?;
    Ok(Some(ArgumentView::build(&graph, &project.source_map)))
}

/// Renders one batch of findings and reports that there is no view.
fn report(
    source_map: &SourceMap,
    errors: Vec<Error>,
    warnings: Vec<Warning>,
    err: &mut impl Write,
    color: bool,
) -> io::Result<Option<ArgumentView>> {
    render(source_map, &Diagnostics { errors, warnings }, err, color)?;
    Ok(None)
}
