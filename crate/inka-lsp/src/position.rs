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

/// The position just past the last character of `text`, counted the way the
/// protocol counts: `\n`, `\r\n`, and a lone `\r` each end a line.
///
/// An edit that replaces a whole document must end exactly there. Unlike
/// [`position`], which inherits core's `\n`-only line count, this never
/// lands on the wrong line in a file with old-Mac line endings — where a
/// client would clamp a too-low end to its first line and splice the new
/// text into the old.
pub fn end_of(text: &str) -> Position {
    let bytes = text.as_bytes();
    let mut line = 0usize;
    let mut last_line_start = 0usize;
    let mut index = 0;
    while index < bytes.len() {
        let width = match bytes[index] {
            b'\r' if bytes.get(index + 1) == Some(&b'\n') => 2,
            b'\r' | b'\n' => 1,
            _ => {
                index += 1;
                continue;
            }
        };
        index += width;
        line += 1;
        last_line_start = index;
    }
    let character = text[last_line_start..].encode_utf16().count();
    Position {
        line: u32::try_from(line).unwrap_or(u32::MAX),
        character: u32::try_from(character).unwrap_or(u32::MAX),
    }
}

/// Converts an LSP position back to a byte offset, the inverse of
/// [`position`].
///
/// Out-of-range coordinates clamp instead of failing — a line past the end
/// of the file resolves to the end of the file, a character past the end of
/// its line to the end of that line — matching how the protocol tells
/// servers to interpret them. A position inside a surrogate pair resolves to
/// the following character boundary.
pub fn offset_of(file: &SourceFile, position: Position) -> usize {
    let mut line_start = 0usize;
    for _ in 0..position.line {
        match file.text[line_start..].find('\n') {
            Some(newline) => line_start += newline + 1,
            None => return file.text.len(),
        }
    }
    let line_end = file.text[line_start..]
        .find('\n')
        .map_or(file.text.len(), |newline| line_start + newline);
    let line = &file.text[line_start..line_end];

    let mut units: u32 = 0;
    for (byte_index, character) in line.char_indices() {
        if units >= position.character {
            return line_start + byte_index;
        }
        units += u32::try_from(character.len_utf16()).unwrap_or(u32::MAX);
    }
    line_end
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
    fn offset_of_round_trips_positions_and_clamps_out_of_range_ones() {
        // Arrange: multibyte first line, ASCII second.
        let (map, span) = map_of("あ𝄞x\ncd");
        let file = map.file(span.file_id).expect("file exists");

        // Act & Assert: every char boundary round-trips through both
        // directions.
        for offset in [0usize, 3, 7, 8, 9, 10, 11] {
            assert_eq!(
                offset_of(file, position(file, offset)),
                offset,
                "offset {offset} should round-trip"
            );
        }
        // Past-the-end coordinates clamp instead of failing.
        assert_eq!(offset_of(file, Position::new(0, 99)), 8); // end of line 0
        assert_eq!(offset_of(file, Position::new(9, 0)), 11); // end of file
        // Inside the 𝄞 surrogate pair resolves to the next boundary.
        assert_eq!(offset_of(file, Position::new(0, 2)), 7);
    }

    #[test]
    fn end_of_counts_every_protocol_line_break() {
        assert_eq!(end_of(""), Position::new(0, 0));
        assert_eq!(end_of("ab"), Position::new(0, 2));
        assert_eq!(end_of("a\nb\n"), Position::new(2, 0));
        assert_eq!(end_of("a\r\nb"), Position::new(1, 1));
        assert_eq!(end_of("a\rb\r"), Position::new(2, 0));
        assert_eq!(end_of("a\r\rb"), Position::new(2, 1));
        assert_eq!(end_of("x\n日本𝄞"), Position::new(1, 4));
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
