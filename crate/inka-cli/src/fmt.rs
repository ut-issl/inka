//! The `inka fmt` command: rewrite every argument file of a project in the
//! canonical layout, or, with `--check`, only report which would change.
//!
//! The layout itself is the core's; this module decides what to do with each
//! file's outcome. A file that does not parse is reported with the same
//! snippets as `inka check` and left untouched, while the others are still
//! formatted, so one broken file never holds the rest back.

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use inka_core::project::{FormattedFile, ProjectFormat, format_project};
use inka_core::read::DiskText;
use inka_core::source::{Diagnostics, Error, SourceMap};

use crate::check::Outcome;
use crate::render::render;

/// Runs `inka fmt` against `project_dir`.
///
/// With `check`, nothing is written: every file that would change is listed
/// on `out`, and the run fails if there is one. Without it, every such file
/// is rewritten in place. Either way the run fails when a file does not
/// parse, or when the project's manifest or values cannot be loaded — then
/// nothing is formatted at all, since whether a fence may be re-indented
/// depends on the values.
///
/// # Errors
///
/// Propagates any I/O error from writing to `out` or `err`. A failure to
/// write a formatted file is reported on `err` instead.
pub fn run(
    project_dir: &Path,
    check: bool,
    out: &mut impl Write,
    err: &mut impl Write,
    color: bool,
) -> io::Result<Outcome> {
    let ProjectFormat { source_map, files } = match format_project(project_dir, &DiskText) {
        Ok(format) => format,
        Err(error) => {
            let diagnostics = Diagnostics {
                errors: vec![Error::unanchored(error.to_string())],
                warnings: Vec::new(),
            };
            render(&SourceMap::new(), &diagnostics, err, color)?;
            return Ok(Outcome::Failed);
        }
    };

    let mut errors = Vec::new();
    let mut unformatted = false;
    for FormattedFile { path, result } in files {
        match result {
            Ok(None) => {}
            Ok(Some(_)) if check => {
                writeln!(out, "{}", path.display())?;
                unformatted = true;
            }
            Ok(Some(formatted)) => {
                if let Err(error) = replace(&path, &formatted) {
                    errors.push(Error::unanchored(format!(
                        "failed to write {}: {error}",
                        path.display()
                    )));
                }
            }
            Err(file_errors) => errors.extend(file_errors),
        }
    }

    let diagnostics = Diagnostics {
        errors,
        warnings: Vec::new(),
    };
    render(&source_map, &diagnostics, err, color)?;
    if unformatted || diagnostics.has_errors() {
        Ok(Outcome::Failed)
    } else {
        Ok(Outcome::Ok)
    }
}

/// Replaces the content of `path` with `text` without ever leaving the file
/// half written: the text goes to a sibling temporary file, which is synced
/// and then renamed over the original with the original's permissions.
///
/// The temporary file does not end in `.inka`, so one left behind by a crash
/// is never read as part of the argument.
fn replace(path: &Path, text: &str) -> io::Result<()> {
    let permissions = fs::metadata(path)?.permissions();
    let mut temporary = path.as_os_str().to_owned();
    temporary.push(".inka-fmt.tmp");
    let temporary = PathBuf::from(temporary);

    let result = write_synced(&temporary, text)
        .and_then(|()| fs::set_permissions(&temporary, permissions))
        .and_then(|()| fs::rename(&temporary, path));
    if result.is_err() {
        // Best effort: the original is untouched either way, and the error
        // that matters is the one being returned.
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn write_synced(path: &Path, text: &str) -> io::Result<()> {
    let mut file = File::create(path)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()
}
