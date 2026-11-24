//! Loading a project into the shared view, with every failure reported the way
//! `inka check` reports it.
//!
//! The loading itself is the core's [`load_argument`]: one operation every
//! presenter of an argument shares, so they cannot disagree about what counts
//! as a presentable one. This module is its terminal face — it renders the
//! findings to a stream and hands the argument on.

use std::io::{self, Write};
use std::path::Path;

use inka_core::argument::{Argument, load_argument};
use inka_core::source::Diagnostics;
use inka_core::view::ArgumentView;

use crate::render::render;

/// Loads and projects the project rooted at `project_dir`.
///
/// The view-only convenience over [`load_project_view`], for commands that
/// render the argument and never read the manifest.
///
/// # Errors
///
/// Propagates any I/O error from writing to `err`.
pub fn load_view(
    project_dir: &Path,
    err: &mut impl Write,
    color: bool,
) -> io::Result<Option<ArgumentView>> {
    Ok(load_project_view(project_dir, err, color)?.map(|loaded| loaded.view))
}

/// Loads and projects the project rooted at `project_dir`, keeping the
/// manifest alongside the view.
///
/// Returns `None` when the argument cannot be rendered, having already written
/// the reason to `err`. Warnings are written whether or not the load succeeded,
/// so one run surfaces everything it saw.
///
/// # Errors
///
/// Propagates any I/O error from writing to `err`.
pub fn load_project_view(
    project_dir: &Path,
    err: &mut impl Write,
    color: bool,
) -> io::Result<Option<Argument>> {
    match load_argument(project_dir) {
        Ok(argument) => {
            let diagnostics = Diagnostics {
                errors: Vec::new(),
                warnings: argument.warnings.clone(),
            };
            render(&argument.source_map, &diagnostics, err, color)?;
            Ok(Some(argument))
        }
        Err(rejected) => {
            render(&rejected.source_map, &rejected.diagnostics, err, color)?;
            Ok(None)
        }
    }
}
