//! Project loading: `inka.toml`, the `argument/` convention, and `{$var}`
//! expansion, combined into one loaded project.
//!
//! The argument is read by convention — every `*.inka` directly under
//! `argument/` — so splitting or renaming files never needs a manifest edit.
//! Subdirectories are rejected rather than skipped: namespace resolution is
//! out of proof-of-concept scope, and silently ignoring files would let the
//! same tree mean something different once namespaces exist.
//!
//! Files are read in sorted path order, but nothing downstream may depend on
//! registration order: graph construction sorts blocks by ID, so file layout
//! and declaration order never change an output.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::ast::Block;
use crate::interpolate::{RenderedDescription, interpolate};
use crate::lexer::Lexer;
use crate::manifest::{Manifest, ManifestError, parse_manifest};
use crate::parser::parse;
use crate::source::{Diagnostic, SourceMap};
use crate::values::{ValuesError, load_values};

/// A fully loaded project: validated manifest and interpolated blocks, plus
/// the source map that resolves their spans.
///
/// Blocks carry [`RenderedDescription`], so the raw pre-interpolation fence
/// text is not representable here: a later pass cannot read it by mistake.
#[derive(Debug, Clone)]
pub struct Project {
    pub manifest: Manifest,
    pub source_map: SourceMap,
    /// Blocks in file order; consumers must not rely on this order.
    pub blocks: Vec<Block<RenderedDescription>>,
}

/// A reason a project directory could not be loaded.
#[derive(Debug, thiserror::Error)]
pub enum ProjectError {
    #[error("failed to read {}: {source}", .path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid {}: {source}", .path.display())]
    Manifest {
        path: PathBuf,
        #[source]
        source: ManifestError,
    },
    #[error(transparent)]
    Values(#[from] ValuesError),
    #[error("argument directory not found: {}", .path.display())]
    MissingArgumentDir { path: PathBuf },
    #[error("subdirectories under argument/ are not supported: {}", .path.display())]
    UnsupportedSubdirectory { path: PathBuf },
    #[error("{}", render_diagnostics(.source_map, .diagnostics))]
    Diagnostics {
        source_map: SourceMap,
        diagnostics: Vec<Diagnostic>,
    },
}

/// Loads the project rooted at `project_dir` (the directory holding
/// `inka.toml`).
///
/// Loading is staged: manifest, then values, then syntax over every argument
/// file, then interpolation over every description. Each stage reports all of
/// its findings before the next runs, so one broken file does not hide
/// problems in another.
///
/// # Errors
///
/// Returns a [`ProjectError`] when `inka.toml` is missing or invalid, values
/// files fail to load or collide, `argument/` is missing or contains a
/// subdirectory, a file cannot be read, or any file has lexer, parser, or
/// interpolation diagnostics.
pub fn load_project(project_dir: &Path) -> Result<Project, ProjectError> {
    let manifest_path = project_dir.join("inka.toml");
    let manifest_text = read_file(&manifest_path)?;
    let manifest = parse_manifest(&manifest_text).map_err(|source| ProjectError::Manifest {
        path: manifest_path,
        source,
    })?;
    let values = load_values(project_dir, &manifest.values.files)?;

    let mut source_map = SourceMap::new();
    let mut blocks = Vec::new();
    let mut diagnostics = Vec::new();
    for path in argument_files(&project_dir.join("argument"))? {
        let text = read_file(&path)?;
        let (file_blocks, file_diagnostics) = lex_and_parse(&mut source_map, path, text);
        blocks.extend(file_blocks);
        diagnostics.extend(file_diagnostics);
    }
    if !diagnostics.is_empty() {
        return Err(ProjectError::Diagnostics {
            source_map,
            diagnostics,
        });
    }

    let (blocks, interpolation_errors) = interpolate_blocks(blocks, &values);
    if !interpolation_errors.is_empty() {
        return Err(ProjectError::Diagnostics {
            source_map,
            diagnostics: interpolation_errors,
        });
    }

    Ok(Project {
        manifest,
        source_map,
        blocks,
    })
}

/// Every `*.inka` directly under `argument/`, sorted by path.
fn argument_files(argument_dir: &Path) -> Result<Vec<PathBuf>, ProjectError> {
    // No existence pre-check: reading directly and classifying the error
    // avoids reporting a directory that vanished in between as a plain read
    // failure, and matches how the manifest read is handled.
    let entries = match std::fs::read_dir(argument_dir) {
        Ok(entries) => entries,
        Err(source)
            if matches!(
                source.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            return Err(ProjectError::MissingArgumentDir {
                path: argument_dir.to_path_buf(),
            });
        }
        Err(source) => {
            return Err(ProjectError::Read {
                path: argument_dir.to_path_buf(),
                source,
            });
        }
    };

    let mut paths = Vec::new();
    for entry in entries {
        paths.push(
            entry
                .map_err(|source| ProjectError::Read {
                    path: argument_dir.to_path_buf(),
                    source,
                })?
                .path(),
        );
    }
    // Sorted before the subdirectory check as well, so which offender an
    // error names never depends on OS iteration order.
    paths.sort();

    let mut files = Vec::new();
    for path in paths {
        if path.is_dir() {
            return Err(ProjectError::UnsupportedSubdirectory { path });
        }
        if path.extension().is_some_and(|ext| ext == "inka") {
            files.push(path);
        }
    }
    Ok(files)
}

/// Registers one argument file and lexes/parses the map-owned copy of its
/// text, so the file contents are never cloned.
fn lex_and_parse(
    source_map: &mut SourceMap,
    path: PathBuf,
    text: String,
) -> (Vec<Block>, Vec<Diagnostic>) {
    let file_id = source_map.add_file(path, text);
    let text = &source_map
        .file(file_id)
        .expect("file was just added to the map")
        .text;
    match Lexer::new(file_id, text).lex() {
        Ok(tokens) => {
            let parsed = parse(file_id, &tokens);
            (parsed.blocks, parsed.diagnostics)
        }
        Err(lex_error) => (Vec::new(), vec![lex_error]),
    }
}

fn read_file(path: &Path) -> Result<String, ProjectError> {
    std::fs::read_to_string(path).map_err(|source| ProjectError::Read {
        path: path.to_path_buf(),
        source,
    })
}

/// Renders every description, collecting undefined-variable diagnostics
/// across all blocks instead of stopping at the first one.
fn interpolate_blocks(
    blocks: Vec<Block>,
    values: &HashMap<String, String>,
) -> (Vec<Block<RenderedDescription>>, Vec<Diagnostic>) {
    let mut rendered = Vec::with_capacity(blocks.len());
    let mut errors = Vec::new();
    for block in blocks {
        match interpolate(&block.common().description, values) {
            Ok(description) => rendered.push(block.with_description(description)),
            Err(block_errors) => errors.extend(block_errors),
        }
    }
    (rendered, errors)
}

/// Formats diagnostics as `path:line:column: message` lines (1-based), the
/// plain-text fallback used until the CLI grows a richer renderer.
fn render_diagnostics(source_map: &SourceMap, diagnostics: &[Diagnostic]) -> String {
    let lines: Vec<String> = diagnostics
        .iter()
        .map(
            |diagnostic| match source_map.file(diagnostic.span.file_id) {
                Some(file) => {
                    let (line, column) = file.line_col(diagnostic.span.start);
                    format!(
                        "{}:{}:{}: {}",
                        file.path.display(),
                        line + 1,
                        column + 1,
                        diagnostic.message
                    )
                }
                None => diagnostic.message.clone(),
            },
        )
        .collect();
    lines.join("\n")
}
