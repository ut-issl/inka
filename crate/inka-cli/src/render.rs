//! Rendering of core findings as annotated source snippets via ariadne.
//!
//! Errors and warnings are rendered interleaved in source order — the order
//! [`Diagnostics::in_source_order`] defines — so a reader walks the file top
//! to bottom regardless of which lane a finding sits in. Anchored findings
//! become labelled snippets pointing at the offending span; project-wide
//! judgements (a missing objective, for instance) carry no span and render as
//! a plain `severity: message` line. Byte offsets are selected so the output
//! matches the byte-offset spans the core produces.

use std::collections::BTreeMap;
use std::io::Write;

use ariadne::{Config, IndexType, Label, Report, ReportKind, sources};
use inka_core::source::{DiagnosticRef, Diagnostics, FileId, SourceMap};

/// Writes every finding in `diagnostics` to `out` in source order, resolving
/// spans through `source_map`.
///
/// `color` enables ANSI colouring of the snippets; callers pass `false` when
/// `out` is not a terminal so piped output and snapshots stay plain.
///
/// # Errors
///
/// Propagates any I/O error from writing to `out`.
pub fn render(
    source_map: &SourceMap,
    diagnostics: &Diagnostics,
    out: &mut impl Write,
    color: bool,
) -> std::io::Result<()> {
    let ordered = diagnostics.in_source_order();

    // Display name and borrowed text per referenced file, in one pass.
    // ariadne identifies a file by the value it also prints as the filename,
    // so the display path doubles as the cache key.
    let files: BTreeMap<FileId, (String, &str)> = ordered
        .iter()
        .filter_map(|diagnostic| diagnostic.span().map(|span| span.file_id))
        .filter_map(|file_id| {
            source_map.file(file_id).map(|file| {
                (
                    file_id,
                    (file.path.display().to_string(), file.text.as_str()),
                )
            })
        })
        .collect();
    let mut cache = sources(files.values().map(|(name, text)| (name.clone(), *text)));

    for diagnostic in ordered {
        match diagnostic.span().and_then(|span| {
            files
                .get(&span.file_id)
                .map(|(name, _)| (name.clone(), span.start..span.end))
        }) {
            Some((name, range)) => {
                Report::build(kind(diagnostic), (name.clone(), range.clone()))
                    .with_config(
                        Config::default()
                            .with_color(color)
                            .with_index_type(IndexType::Byte),
                    )
                    .with_message(diagnostic.message())
                    .with_label(Label::new((name, range)).with_message(diagnostic.message()))
                    .finish()
                    .write(&mut cache, &mut *out)?;
            }
            // Unanchored, or a span whose file is absent from the map: fall
            // back to a bare line rather than dropping the finding.
            None => writeln!(out, "{diagnostic}")?,
        }
    }
    Ok(())
}

/// Maps a finding's lane onto the matching ariadne report kind — the one
/// point where the error/warning distinction becomes a display attribute.
fn kind(diagnostic: DiagnosticRef<'_>) -> ReportKind<'static> {
    match diagnostic {
        DiagnosticRef::Error(_) => ReportKind::Error,
        DiagnosticRef::Warning(_) => ReportKind::Warning,
    }
}
