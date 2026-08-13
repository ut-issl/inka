//! Source-file bookkeeping: file identity, byte-offset spans, and the table
//! that resolves them back to paths, text, and line/column positions.
//!
//! Spans store byte offsets, not line/column: offsets are unambiguous (no
//! "what is a column" question), sliceable (`&text[start..end]`), and cheap.
//! Line/column is derived on demand via [`SourceFile::line_col`] only when a
//! diagnostic is rendered.

use std::path::PathBuf;

/// Index into a [`SourceMap`]. A newtype rather than an alias so file ids
/// cannot be confused with other integers; the inner index is private so ids
/// can only be minted by [`SourceMap::add_file`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct FileId(usize);

/// One loaded source file plus its derived line-start table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile {
    pub path: PathBuf,
    pub text: String,
    /// Byte offset of the start of each line; `line_starts[0] == 0`.
    line_starts: Vec<usize>,
}

impl SourceFile {
    /// Converts a byte offset into a zero-based `(line, column)` pair.
    ///
    /// The column is a byte offset within the line; converting it to a display
    /// width is the renderer's responsibility. `offset == self.text.len()` is
    /// valid, allowing diagnostics to point to the end of the file.
    ///
    /// # Panics
    ///
    /// Panics if `offset` is greater than the length of the source text.
    #[must_use]
    pub fn line_col(&self, offset: usize) -> (usize, usize) {
        assert!(
            offset <= self.text.len(),
            "offset {offset} out of range for {} ({} bytes)",
            self.path.display(),
            self.text.len()
        );
        let line = self.line_starts.partition_point(|&s| s <= offset) - 1;
        (line, offset - self.line_starts[line])
    }
}

/// Owner of all loaded source files. Built once by the loader; everything
/// downstream carries [`FileId`]s and resolves them here at render time.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SourceMap {
    files: Vec<SourceFile>,
}

impl SourceMap {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a file and returns its id. Line starts are computed here,
    /// once. Counting only `\n` is also correct for CRLF input: a line
    /// starts right after the `\n`.
    pub fn add_file(&mut self, path: PathBuf, text: String) -> FileId {
        let line_starts = std::iter::once(0)
            .chain(text.match_indices('\n').map(|(i, _)| i + 1))
            .collect();
        let file_id = FileId(self.files.len());
        self.files.push(SourceFile {
            path,
            text,
            line_starts,
        });
        file_id
    }

    #[must_use]
    pub fn file(&self, file_id: FileId) -> Option<&SourceFile> {
        self.files.get(file_id.0)
    }
}

/// A problem that invalidates the argument: whatever produced it cannot hand
/// back a usable artifact.
///
/// Errors and artifacts are mutually exclusive by convention: a pass returns
/// `Result<Artifact, Vec<Error>>` (or accumulates into [`Diagnostics`]), so
/// "failed with no errors" is not a representable outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    /// The source range the message is about, or `None` for a judgement on
    /// the project as a whole — a missing objective, an empty argument.
    pub span: Option<Span>,
    /// Human-readable description of the problem.
    pub message: String,
}

impl Error {
    /// An error anchored to the source range it concerns.
    #[must_use]
    pub fn new(span: Span, message: impl Into<String>) -> Self {
        Self {
            span: Some(span),
            message: message.into(),
        }
    }

    /// An error about the project as a whole, with no source range to
    /// anchor to.
    #[must_use]
    pub fn unanchored(message: impl Into<String>) -> Self {
        Self {
            span: None,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "error: {}", self.message)
    }
}

/// A suspicion about the argument that leaves it well-formed: the artifact
/// still exists, so warnings accompany success instead of replacing it.
///
/// If a finding should ever prevent the artifact from being produced, that is
/// by definition an [`Error`] and must be emitted as one at its source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warning {
    /// The source range the message is about, or `None` for a judgement on
    /// the project as a whole.
    pub span: Option<Span>,
    /// Human-readable description of the suspicion.
    pub message: String,
}

impl Warning {
    /// A warning anchored to the source range it concerns.
    #[must_use]
    pub fn new(span: Span, message: impl Into<String>) -> Self {
        Self {
            span: Some(span),
            message: message.into(),
        }
    }
}

impl std::fmt::Display for Warning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "warning: {}", self.message)
    }
}

/// Findings of a pass that produces no artifact of its own, with errors and
/// warnings in separate lanes so no caller ever inspects a severity at run
/// time: "does this fail the check?" is [`Diagnostics::has_errors`], decided
/// by which lane a finding was pushed into at its source.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Diagnostics {
    pub errors: Vec<Error>,
    pub warnings: Vec<Warning>,
}

impl Diagnostics {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Whether the findings invalidate the argument.
    #[must_use]
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.errors.is_empty() && self.warnings.is_empty()
    }

    /// Every finding in source order: anchored ones sorted by file and byte
    /// range, unanchored ones after them. The sort is stable, so findings on
    /// the same span keep errors before warnings and otherwise preserve
    /// emission order.
    #[must_use]
    pub fn in_source_order(&self) -> Vec<DiagnosticRef<'_>> {
        let mut refs: Vec<DiagnosticRef<'_>> = self
            .errors
            .iter()
            .map(DiagnosticRef::Error)
            .chain(self.warnings.iter().map(DiagnosticRef::Warning))
            .collect();
        refs.sort_by_key(|diagnostic| {
            let span = diagnostic.span();
            (
                span.is_none(),
                span.map(|span| (span.file_id, span.start, span.end)),
            )
        });
        refs
    }
}

/// A view of one finding for rendering, where the error/warning distinction
/// matters only as a display kind. This is the sole place the two lanes are
/// handled through one shape.
#[derive(Debug, Clone, Copy)]
pub enum DiagnosticRef<'a> {
    Error(&'a Error),
    Warning(&'a Warning),
}

impl<'a> DiagnosticRef<'a> {
    #[must_use]
    pub fn span(self) -> Option<Span> {
        match self {
            DiagnosticRef::Error(error) => error.span,
            DiagnosticRef::Warning(warning) => warning.span,
        }
    }

    #[must_use]
    pub fn message(self) -> &'a str {
        match self {
            DiagnosticRef::Error(error) => &error.message,
            DiagnosticRef::Warning(warning) => &warning.message,
        }
    }
}

impl std::fmt::Display for DiagnosticRef<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DiagnosticRef::Error(error) => error.fmt(f),
            DiagnosticRef::Warning(warning) => warning.fmt(f),
        }
    }
}

/// Half-open byte range `[start, end)` within one source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub file_id: FileId,
    pub start: usize,
    pub end: usize,
}

/// A value together with the span it was parsed from, so any later pass can
/// point a diagnostic at the exact source location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spanned<T> {
    pub span: Span,
    pub value: T,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_col_handles_lf_and_crlf() {
        // Arrange: bytes are a(0) b(1) \n(2) c(3) d(4) \r(5) \n(6) e(7) f(8)
        let mut map = SourceMap::new();
        let id = map.add_file("a.inka".into(), "ab\ncd\r\nef".to_string());
        let file = map.file(id).unwrap();

        // Act & Assert
        assert_eq!(file.line_col(0), (0, 0)); // 'a'
        assert_eq!(file.line_col(1), (0, 1)); // 'b'
        assert_eq!(file.line_col(3), (1, 0)); // 'c'
        assert_eq!(file.line_col(7), (2, 0)); // 'e', right after CRLF
        assert_eq!(file.line_col(9), (2, 2)); // end of file is allowed
    }

    #[test]
    fn in_source_order_interleaves_lanes_and_puts_unanchored_last() {
        // Arrange: a warning anchored earlier in the file than an error, plus
        // an unanchored error.
        let mut map = SourceMap::new();
        let file_id = map.add_file("a.inka".into(), "abcdef".to_string());
        let span = |start, end| Span {
            file_id,
            start,
            end,
        };
        let diagnostics = Diagnostics {
            errors: vec![Error::unanchored("whole"), Error::new(span(4, 5), "late")],
            warnings: vec![Warning::new(span(1, 2), "early")],
        };

        // Act
        let ordered: Vec<String> = diagnostics
            .in_source_order()
            .iter()
            .map(ToString::to_string)
            .collect();

        // Assert: file position decides, not lane; unanchored goes last.
        assert_eq!(ordered, ["warning: early", "error: late", "error: whole"]);
    }

    #[test]
    #[should_panic(expected = "out of range")]
    fn line_col_panics_past_end_of_file() {
        let mut map = SourceMap::new();
        let id = map.add_file("a.inka".into(), "ab".to_string());
        let file = map.file(id).unwrap();

        assert_eq!(file.line_col(2), (0, 2));
        let _ = map.file(id).unwrap().line_col(3);
    }
}
