//! Rendering of core diagnostics as annotated source snippets via ariadne.
//!
//! Anchored diagnostics become labelled snippets pointing at the offending
//! span; project-wide judgements (a missing objective, for instance) carry no
//! span and render as a plain `severity: message` line. Colour is disabled and
//! byte offsets are selected so the output is deterministic and matches the
//! byte-offset spans the core produces.

use std::collections::BTreeMap;
use std::io::Write;

use ariadne::{Config, IndexType, Label, Report, ReportKind, sources};
use inka_core::source::{Diagnostic, FileId, Severity, SourceMap};

/// Writes every diagnostic in `diagnostics` to `out`, resolving spans through
/// `source_map`.
///
/// `color` enables ANSI colouring of the snippets; callers pass `false` when
/// `out` is not a terminal so piped output and snapshots stay plain.
///
/// # Errors
///
/// Propagates any I/O error from writing to `out`.
pub fn render(
    source_map: &SourceMap,
    diagnostics: &[Diagnostic],
    out: &mut impl Write,
    color: bool,
) -> std::io::Result<()> {
    // ariadne identifies a file by the value it also prints as the filename,
    // so the display path doubles as the cache key.
    let names = file_names(source_map, diagnostics);
    let mut cache = sources(
        names
            .iter()
            .map(|(&file_id, name)| (name.clone(), file_text(source_map, file_id))),
    );

    for diagnostic in diagnostics {
        match diagnostic.span.and_then(|span| {
            names
                .get(&span.file_id)
                .map(|name| (name.clone(), span.start..span.end))
        }) {
            Some((name, range)) => {
                Report::build(kind(diagnostic.severity), (name.clone(), range.clone()))
                    .with_config(
                        Config::default()
                            .with_color(color)
                            .with_index_type(IndexType::Byte),
                    )
                    .with_message(&diagnostic.message)
                    .with_label(Label::new((name, range)).with_message(&diagnostic.message))
                    .finish()
                    .write(&mut cache, &mut *out)?;
            }
            // Unanchored, or a span whose file is absent from the map: fall
            // back to a bare line rather than dropping the diagnostic.
            None => writeln!(out, "{}: {}", diagnostic.severity, diagnostic.message)?,
        }
    }
    Ok(())
}

/// Display name for every file referenced by an anchored diagnostic, keyed by
/// id so both the cache and the per-diagnostic lookup agree.
fn file_names(source_map: &SourceMap, diagnostics: &[Diagnostic]) -> BTreeMap<FileId, String> {
    diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic.span.map(|span| span.file_id))
        .filter_map(|file_id| {
            source_map
                .file(file_id)
                .map(|file| (file_id, file.path.display().to_string()))
        })
        .collect()
}

/// Source text of a referenced file, or empty when it cannot be resolved (the
/// name map has already filtered those out, so this is only a safe default).
fn file_text(source_map: &SourceMap, file_id: FileId) -> String {
    source_map
        .file(file_id)
        .map(|file| file.text.clone())
        .unwrap_or_default()
}

/// Maps a core [`Severity`] onto the matching ariadne report kind.
fn kind(severity: Severity) -> ReportKind<'static> {
    match severity {
        Severity::Error => ReportKind::Error,
        Severity::Warning => ReportKind::Warning,
    }
}
