//! Byte-offset spans to LSP positions.
//!
//! Core spans are byte offsets; LSP positions count UTF-16 code units within
//! a line, the protocol's default encoding and the only one VS Code speaks.
//! [`inka_core::source::SourceFile::line_col`] supplies the line and byte
//! column, and re-measuring the line prefix in UTF-16 bridges the rest.

use inka_core::source::{SourceFile, Span};
use lsp_types::{Position, Range};

/// Converts a span to an LSP range within its file.
pub fn range(file: &SourceFile, span: Span) -> Range {
    Range {
        start: position(file, span.start),
        end: position(file, span.end),
    }
}

/// Converts a byte offset to an LSP position.
///
/// Coordinates saturate at `u32::MAX` rather than panic: the protocol caps
/// them at 32 bits, and a client is required to clamp out-of-range values.
/// A malformed offset (past the end of the file, or not on a character
/// boundary) degrades to a clamped or byte-counted column for the same
/// reason — a bad span from analysis must not take the whole server down.
///
/// Known divergence: core counts lines by `\n` alone, while LSP also treats
/// a lone `\r` as a line break, so old-Mac line endings report lower line
/// numbers than a strict client would use. CRLF and LF agree everywhere.
pub fn position(file: &SourceFile, offset: usize) -> Position {
    let offset = offset.min(file.text.len());
    let (line, byte_column) = file.line_col(offset);
    let line_start = offset - byte_column;
    let character = file
        .text
        .get(line_start..offset)
        .map_or(byte_column, |prefix| prefix.encode_utf16().count());
    Position {
        line: u32::try_from(line).unwrap_or(u32::MAX),
        character: u32::try_from(character).unwrap_or(u32::MAX),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inka_core::source::SourceMap;

    fn map_of(text: &str) -> (SourceMap, Span) {
        let mut map = SourceMap::new();
        let id = map.add_file("a.inka".into(), text.to_owned());
        let span = Span {
            file_id: id,
            start: 0,
            end: 0,
        };
        (map, span)
    }

    #[test]
    fn ascii_positions_count_bytes_and_utf16_alike() {
        // Arrange
        let (map, span) = map_of("ab\ncd");
        let file = map.file(span.file_id).expect("file exists");

        // Act & Assert
        assert_eq!(position(file, 0), Position::new(0, 0));
        assert_eq!(position(file, 4), Position::new(1, 1));
    }

    #[test]
    fn multibyte_positions_count_utf16_code_units_not_bytes() {
        // Arrange: "あ" is 3 bytes / 1 UTF-16 unit; "𝄞" is 4 bytes / 2 units.
        let (map, span) = map_of("あ𝄞x");
        let file = map.file(span.file_id).expect("file exists");

        // Act & Assert
        assert_eq!(position(file, 3), Position::new(0, 1)); // after あ
        assert_eq!(position(file, 7), Position::new(0, 3)); // after 𝄞
        assert_eq!(position(file, 8), Position::new(0, 4)); // after x
    }

    #[test]
    fn range_spans_lines() {
        // Arrange
        let (map, span) = map_of("one\ntwo\n");
        let file = map.file(span.file_id).expect("file exists");

        // Act
        let converted = range(
            file,
            Span {
                start: 1,
                end: 5,
                ..span
            },
        );

        // Assert
        assert_eq!(converted.start, Position::new(0, 1));
        assert_eq!(converted.end, Position::new(1, 1));
    }
}
