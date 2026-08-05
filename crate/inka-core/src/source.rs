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
    #[should_panic(expected = "out of range")]
    fn line_col_panics_past_end_of_file() {
        let mut map = SourceMap::new();
        let id = map.add_file("a.inka".into(), "ab".to_string());
        let file = map.file(id).unwrap();

        assert_eq!(file.line_col(2), (0, 2));
        let _ = map.file(id).unwrap().line_col(3);
    }
}
