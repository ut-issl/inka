//! Project loading: `inka.toml`, the `argument/` convention, and `{$var}`
//! expansion, combined into one loaded project — and, over the same files and
//! values, formatting every argument file.
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
use crate::evidence::{EvidenceError, EvidenceSet, load_files};
use crate::format::format_source;
use crate::interpolate::{RenderedDescription, interpolate};
use crate::lexer::Lexer;
use crate::manifest::{Manifest, ManifestError, parse_manifest};
use crate::parser::parse;
use crate::read::{DiskText, ReadText};
use crate::source::{Error, SourceMap, Warning};
use crate::values::{ValuesError, load_values};

/// A fully loaded project: validated manifest, interpolated blocks, and the
/// test evidence the manifest imports, plus the source map that resolves
/// block spans.
///
/// Blocks carry [`RenderedDescription`], so the raw pre-interpolation fence
/// text is not representable here: a later pass cannot read it by mistake.
#[derive(Debug, Clone)]
pub struct Project {
    pub manifest: Manifest,
    pub source_map: SourceMap,
    /// Blocks in file order; consumers must not rely on this order.
    pub blocks: Vec<Block<RenderedDescription>>,
    /// Every test the manifest's `[evidence]` files provide; empty when the
    /// manifest imports none. Loaded here so that a missing or malformed
    /// evidence file is an input error for every command, not only `eval`.
    pub evidence: EvidenceSet,
}

/// The outcome of loading a project directory.
///
/// Errors and the artifact are mutually exclusive — that is what the inner
/// `Result` says — while warnings are orthogonal to success and therefore
/// live outside it, exactly once. A caller that only wants the project can
/// take `outcome.result?`; a caller that reports must read `warnings` first
/// so a failed load still surfaces every finding of the run.
#[derive(Debug)]
pub struct LoadOutcome {
    /// Suspicions raised while loading; the project (if any) is well-formed
    /// despite them.
    pub warnings: Vec<Warning>,
    /// The loaded project, or the reason there is none.
    pub result: Result<Project, LoadError>,
}

/// A reason a project directory could not be loaded.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
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
    #[error(transparent)]
    Evidence(#[from] EvidenceError),
    #[error("argument directory not found: {}", .path.display())]
    MissingArgumentDir { path: PathBuf },
    #[error("subdirectories under argument/ are not supported: {}", .path.display())]
    UnsupportedSubdirectory { path: PathBuf },
    /// The sources were readable but their content is broken. Constructed
    /// only from a non-empty error batch: a stage that found no error keeps
    /// loading instead.
    #[error("{}", render_errors(.source_map, .errors))]
    Invalid {
        source_map: SourceMap,
        errors: Vec<Error>,
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
/// The `result` is a [`LoadError`] when `inka.toml` is missing or invalid,
/// values files fail to load or collide, `argument/` is missing or contains
/// a subdirectory, a file cannot be read, or any file has lexer, parser, or
/// interpolation errors.
#[must_use]
pub fn load_project(project_dir: &Path) -> LoadOutcome {
    load_project_with(project_dir, &DiskText)
}

/// Loads the project rooted at `project_dir`, reading file contents through
/// `reader` instead of straight from disk.
///
/// This is [`load_project`] with the content source swapped out: an editor
/// passes a reader that serves its open buffers so analysis sees what is on
/// screen. Which files make up the project is still decided by the
/// filesystem — see [`ReadText`].
#[must_use]
pub fn load_project_with(project_dir: &Path, reader: &dyn ReadText) -> LoadOutcome {
    // No load-stage pass emits warnings yet: the lane exists so every caller
    // is already shaped for the day one does.
    LoadOutcome {
        warnings: Vec::new(),
        result: load(project_dir, reader),
    }
}

fn load(project_dir: &Path, reader: &dyn ReadText) -> Result<Project, LoadError> {
    let manifest_path = project_dir.join("inka.toml");
    let manifest_text = read_file(&manifest_path, reader)?;
    let manifest = parse_manifest(&manifest_text).map_err(|source| LoadError::Manifest {
        path: manifest_path,
        source,
    })?;
    let values = load_values(project_dir, &manifest.values.files, reader)?;
    let evidence = match &manifest.evidence {
        Some(section) => load_files(project_dir, &section.files, reader)?,
        None => EvidenceSet::default(),
    };

    let mut source_map = SourceMap::new();
    let mut blocks = Vec::new();
    let mut errors = Vec::new();
    for path in argument_files(&project_dir.join("argument"))? {
        let text = read_file(&path, reader)?;
        let (file_blocks, file_errors) = lex_and_parse(&mut source_map, path, text);
        blocks.extend(file_blocks);
        errors.extend(file_errors);
    }
    if !errors.is_empty() {
        return Err(LoadError::Invalid { source_map, errors });
    }

    let (blocks, interpolation_errors) = interpolate_blocks(blocks, &values);
    if !interpolation_errors.is_empty() {
        return Err(LoadError::Invalid {
            source_map,
            errors: interpolation_errors,
        });
    }

    Ok(Project {
        manifest,
        source_map,
        blocks,
        evidence,
    })
}

/// The canonical layout of every argument file of a project.
#[derive(Debug)]
pub struct ProjectFormat {
    /// Every argument file as read, which the errors' spans point into.
    pub source_map: SourceMap,
    /// One entry per argument file, in sorted path order.
    pub files: Vec<FormattedFile>,
}

/// One argument file's formatting.
#[derive(Debug)]
pub struct FormattedFile {
    pub path: PathBuf,
    /// The canonical text when it differs from the file, `None` when the
    /// file is already formatted, or the errors that keep it from being
    /// formatted at all.
    pub result: Result<Option<String>, Vec<Error>>,
}

/// Formats every `*.inka` file of the project rooted at `project_dir`
/// without writing anything.
///
/// The manifest and values are loaded because whether a fence may be
/// re-indented depends on the values interpolated into it. A file that does
/// not parse is reported, never formatted, and does not stop the others.
///
/// # Errors
///
/// Returns a [`LoadError`] when `inka.toml` or a values file is missing or
/// invalid, or when `argument/` or one of its files cannot be read; nothing
/// is formatted then.
pub fn format_project(
    project_dir: &Path,
    reader: &dyn ReadText,
) -> Result<ProjectFormat, LoadError> {
    let manifest_path = project_dir.join("inka.toml");
    let manifest_text = read_file(&manifest_path, reader)?;
    let manifest = parse_manifest(&manifest_text).map_err(|source| LoadError::Manifest {
        path: manifest_path,
        source,
    })?;
    let values = load_values(project_dir, &manifest.values.files, reader)?;

    let mut source_map = SourceMap::new();
    let mut files = Vec::new();
    for path in argument_files(&project_dir.join("argument"))? {
        let text = read_file(&path, reader)?;
        // The map keeps its own copy so errors can be rendered against it.
        let file_id = source_map.add_file(path.clone(), text.clone());
        let result = format_source(file_id, &text, &values)
            .map(|formatted| (formatted != text).then_some(formatted));
        files.push(FormattedFile { path, result });
    }
    Ok(ProjectFormat { source_map, files })
}

/// Every `*.inka` directly under `argument/`, sorted by path.
fn argument_files(argument_dir: &Path) -> Result<Vec<PathBuf>, LoadError> {
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
            return Err(LoadError::MissingArgumentDir {
                path: argument_dir.to_path_buf(),
            });
        }
        Err(source) => {
            return Err(LoadError::Read {
                path: argument_dir.to_path_buf(),
                source,
            });
        }
    };

    let mut paths = Vec::new();
    for entry in entries {
        paths.push(
            entry
                .map_err(|source| LoadError::Read {
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
            return Err(LoadError::UnsupportedSubdirectory { path });
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
) -> (Vec<Block>, Vec<Error>) {
    let file_id = source_map.add_file(path, text);
    let text = &source_map
        .file(file_id)
        .expect("file was just added to the map")
        .text;
    match Lexer::new(file_id, text).lex() {
        Ok(tokens) => {
            let parsed = parse(file_id, &tokens);
            (parsed.blocks, parsed.errors)
        }
        Err(lex_error) => (Vec::new(), vec![lex_error]),
    }
}

fn read_file(path: &Path, reader: &dyn ReadText) -> Result<String, LoadError> {
    reader.read_text(path).map_err(|source| LoadError::Read {
        path: path.to_path_buf(),
        source,
    })
}

/// Renders every description, collecting undefined-variable diagnostics
/// across all blocks instead of stopping at the first one.
fn interpolate_blocks(
    blocks: Vec<Block>,
    values: &HashMap<String, String>,
) -> (Vec<Block<RenderedDescription>>, Vec<Error>) {
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

/// Formats errors as `path:line:column: error: message` lines (1-based), the
/// plain-text fallback used when [`LoadError::Invalid`] is displayed as a
/// plain error. Unanchored errors render as `error: message` alone.
fn render_errors(source_map: &SourceMap, errors: &[Error]) -> String {
    let lines: Vec<String> = errors
        .iter()
        .map(|error| {
            let located = error
                .span
                .and_then(|span| source_map.file(span.file_id).map(|file| (file, span)));
            match located {
                Some((file, span)) => {
                    let (line, column) = file.line_col(span.start);
                    format!(
                        "{}:{}:{}: {error}",
                        file.path.display(),
                        line + 1,
                        column + 1,
                    )
                }
                None => error.to_string(),
            }
        })
        .collect();
    lines.join("\n")
}
