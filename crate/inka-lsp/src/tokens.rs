//! Semantic tokens: block ids colored as types, and the `CommonMark` inside
//! descriptions tokenized by a real parser.
//!
//! The `TextMate` grammar in `editors/vscode` deliberately keeps fence
//! contents flat — a line-regex grammar cannot parse `CommonMark` — so this
//! module supplies the markup structure: headings, emphasis, code, and links,
//! each as a `markup*` token type the extension maps onto the `TextMate`
//! scopes every theme already styles for Markdown. Ids are `type` tokens —
//! the one color for named nodes, definition and reference alike — with the
//! `declaration` modifier marking the definition.
//!
//! Clients do not take multiline tokens, so every token is split per line
//! before the protocol's delta encoding.

use lsp_types::{
    SemanticToken, SemanticTokenModifier, SemanticTokenType, SemanticTokens, SemanticTokensLegend,
};
use pulldown_cmark::{Event, Options, Parser, Tag};

use inka_core::project::Project;
use inka_core::source::{FileId, SourceFile, Span};

use crate::position;

/// Legend indices; [`legend`] lists the types in exactly this order.
const TYPE_TOKEN: u32 = 0;
const HEADING_TOKEN: u32 = 1;
const BOLD_TOKEN: u32 = 2;
const ITALIC_TOKEN: u32 = 3;
const CODE_TOKEN: u32 = 4;
const LINK_TOKEN: u32 = 5;

/// Bit of the `declaration` modifier, set on a definition's id.
const DECLARATION_BIT: u32 = 1;

/// The legend `initialize` announces; the indices the token stream carries
/// point into these lists.
pub fn legend() -> SemanticTokensLegend {
    SemanticTokensLegend {
        token_types: vec![
            SemanticTokenType::TYPE,
            SemanticTokenType::new("markupHeading"),
            SemanticTokenType::new("markupBold"),
            SemanticTokenType::new("markupItalic"),
            SemanticTokenType::new("markupCode"),
            SemanticTokenType::new("markupLink"),
        ],
        token_modifiers: vec![SemanticTokenModifier::DECLARATION],
    }
}

/// A token before delta encoding: absolute UTF-16 line/column coordinates.
struct Absolute {
    line: u32,
    start: u32,
    length: u32,
    token_type: u32,
    modifiers: u32,
}

/// Every semantic token of one file, in the protocol's delta encoding.
pub fn full(project: &Project, file: FileId) -> Option<SemanticTokens> {
    let source = project.source_map.file(file)?;
    let mut found: Vec<Absolute> = Vec::new();
    for block in &project.blocks {
        let common = block.common();
        push(
            &mut found,
            source,
            file,
            common.id.span,
            TYPE_TOKEN,
            DECLARATION_BIT,
        );
        for child in block.children() {
            push(&mut found, source, file, child.span, TYPE_TOKEN, 0);
        }
        description_tokens(&mut found, source, file, common.description.span());
    }
    found.sort_unstable_by_key(|token| (token.line, token.start));
    Some(SemanticTokens {
        result_id: None,
        data: encode(&found),
    })
}

/// Appends `span` as tokens of `token_type`, one per line it touches.
fn push(
    found: &mut Vec<Absolute>,
    source: &SourceFile,
    file: FileId,
    span: Span,
    token_type: u32,
    modifiers: u32,
) {
    if span.file_id != file {
        return;
    }
    for (start, end) in line_segments(source, span) {
        let from = position::position(source, start);
        let to = position::position(source, end);
        let length = to.character.saturating_sub(from.character);
        if length == 0 {
            continue;
        }
        found.push(Absolute {
            line: from.line,
            start: from.character,
            length,
            token_type,
            modifiers,
        });
    }
}

/// Splits a byte span at line breaks, dropping the breaks themselves and any
/// `\r` of a CRLF ending.
fn line_segments(source: &SourceFile, span: Span) -> Vec<(usize, usize)> {
    let text = &source.text;
    let mut segments = Vec::new();
    let mut start = span.start.min(text.len());
    let span_end = span.end.min(text.len());
    while start < span_end {
        let line_end = text[start..span_end]
            .find('\n')
            .map_or(span_end, |found| start + found);
        let mut end = line_end;
        if end > start && text.as_bytes()[end - 1] == b'\r' {
            end -= 1;
        }
        if end > start {
            segments.push((start, end));
        }
        start = line_end + 1;
    }
    segments
}

/// Tokenizes the `CommonMark` of one description.
///
/// The parser sees the text the way `normalize::description` reads it —
/// trailing whitespace stripped and the common space indent removed, since
/// four leading spaces would turn prose into an indented code block — while
/// every emitted range is mapped back onto the raw fence bytes on screen.
/// Only the outermost construct of a nested piece of markup is emitted, so
/// tokens never overlap.
fn description_tokens(found: &mut Vec<Absolute>, source: &SourceFile, file: FileId, span: Span) {
    if span.file_id != file || span.end > source.text.len() || span.start > span.end {
        return;
    }
    let raw = &source.text[span.start..span.end];
    let (dedented, lines) = dedent(raw);
    let mut skip_until = 0;
    for (event, range) in Parser::new_ext(&dedented, Options::empty()).into_offset_iter() {
        if range.start < skip_until {
            continue;
        }
        let token_type = match event {
            Event::Start(Tag::Heading { .. }) => HEADING_TOKEN,
            Event::Start(Tag::Strong) => BOLD_TOKEN,
            Event::Start(Tag::Emphasis) => ITALIC_TOKEN,
            Event::Start(Tag::CodeBlock(_)) | Event::Code(_) => CODE_TOKEN,
            Event::Start(Tag::Link { .. } | Tag::Image { .. }) => LINK_TOKEN,
            _ => continue,
        };
        skip_until = range.end;
        for (start, end) in map_back(&lines, range.start, range.end) {
            push(
                found,
                source,
                file,
                Span {
                    file_id: file,
                    start: span.start + start,
                    end: span.start + end,
                },
                token_type,
                0,
            );
        }
    }
}

/// One dedented line's offsets: where it starts in the dedented text, and
/// where its kept content starts in the raw fence slice.
struct DedentedLine {
    dedented_start: usize,
    raw_start: usize,
    length: usize,
}

/// Rebuilds the fence content close to the way `normalize::description`
/// reads it — line endings unified, trailing whitespace stripped, the common
/// space indent removed — keeping per line the way back into the raw slice.
/// The indent rule mirrors `normalize::common_indent`: only spaces count,
/// measured over non-blank lines. NFC composition is deliberately skipped:
/// the mapping must stay byte-exact, and composition never changes markup
/// structure.
fn dedent(raw: &str) -> (String, Vec<DedentedLine>) {
    let raw_lines = split_lines(raw);
    let indent = raw_lines
        .iter()
        .filter(|(_, line)| !line.is_empty())
        .map(|(_, line)| line.len() - line.trim_start_matches(' ').len())
        .min()
        .unwrap_or(0);

    let mut dedented = String::with_capacity(raw.len());
    let mut lines = Vec::with_capacity(raw_lines.len());
    for (index, (raw_offset, line)) in raw_lines.iter().enumerate() {
        let kept = if line.is_empty() {
            line
        } else {
            &line[indent..]
        };
        lines.push(DedentedLine {
            dedented_start: dedented.len(),
            raw_start: raw_offset + (line.len() - kept.len()),
            length: kept.len(),
        });
        dedented.push_str(kept);
        if index + 1 < raw_lines.len() {
            dedented.push('\n');
        }
    }
    (dedented, lines)
}

/// Lines of `raw` with their start offsets, split at `\n`, `\r\n`, and lone
/// `\r` — the same three endings `normalize::description` unifies — each
/// already trimmed of trailing whitespace.
fn split_lines(raw: &str) -> Vec<(usize, &str)> {
    let bytes = raw.as_bytes();
    let mut lines = Vec::new();
    let mut start = 0;
    let mut index = 0;
    while index < bytes.len() {
        let advance = match bytes[index] {
            b'\r' if bytes.get(index + 1) == Some(&b'\n') => 2,
            b'\n' | b'\r' => 1,
            _ => {
                index += 1;
                continue;
            }
        };
        lines.push((start, raw[start..index].trim_end()));
        index += advance;
        start = index;
    }
    lines.push((start, raw[start..].trim_end()));
    lines
}

/// Maps a dedented byte range back to raw-slice ranges, one per line it
/// touches. Line breaks separate segments, so nothing here can span one.
/// `lines` is ordered by `dedented_start`, so the scan starts at the first
/// line reaching `start` and stops past `end`.
fn map_back(lines: &[DedentedLine], start: usize, end: usize) -> Vec<(usize, usize)> {
    let mut segments = Vec::new();
    let first = lines.partition_point(|line| line.dedented_start + line.length <= start);
    for line in &lines[first..] {
        if line.dedented_start >= end {
            break;
        }
        let line_end = line.dedented_start + line.length;
        let segment_start = start.max(line.dedented_start);
        let segment_end = end.min(line_end);
        if segment_start < segment_end {
            let raw = line.raw_start + (segment_start - line.dedented_start);
            segments.push((raw, raw + (segment_end - segment_start)));
        }
    }
    segments
}

/// The protocol's delta encoding over position-sorted tokens.
fn encode(found: &[Absolute]) -> Vec<SemanticToken> {
    let mut data = Vec::with_capacity(found.len());
    let (mut line, mut start) = (0, 0);
    for token in found {
        let delta_line = token.line - line;
        let delta_start = if delta_line == 0 {
            token.start - start
        } else {
            token.start
        };
        data.push(SemanticToken {
            delta_line,
            delta_start,
            length: token.length,
            token_type: token.token_type,
            token_modifiers_bitset: token.modifiers,
        });
        line = token.line;
        start = token.start;
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedent_strips_the_common_indent_and_maps_offsets_back() {
        // Arrange: two content lines indented by two, one blank line between.
        let raw = "  # Goal\n\n  body\n";

        // Act
        let (dedented, lines) = dedent(raw);

        // Assert: the parser sees dedented text…
        assert_eq!(dedented, "# Goal\n\nbody\n");
        // …and the heading's range [0, 6) maps back behind the indent.
        assert_eq!(map_back(&lines, 0, 6), [(2, 8)]);
        // `body` sits at dedented [8, 12), raw [12, 16).
        assert_eq!(map_back(&lines, 8, 12), [(12, 16)]);
    }

    #[test]
    fn map_back_splits_a_range_spanning_lines_per_line() {
        // Arrange
        let (dedented, lines) = dedent("  one\n  two\n");
        assert_eq!(dedented, "one\ntwo\n");

        // Act: a range covering both words and the break between them.
        let segments = map_back(&lines, 0, 7);

        // Assert: two segments, neither containing the line break.
        assert_eq!(segments, [(2, 5), (8, 11)]);
    }

    #[test]
    fn lone_carriage_returns_split_lines_like_normalize() {
        // Arrange: old-Mac endings — no `\n` anywhere in the fence.
        let (dedented, lines) = dedent("  # Goal\r  body\r");

        // Assert: two lines, so the heading cannot swallow the body…
        assert_eq!(dedented, "# Goal\nbody\n");
        // …and both map back behind their own line's indent.
        assert_eq!(map_back(&lines, 0, 6), [(2, 8)]);
        assert_eq!(map_back(&lines, 7, 11), [(11, 15)]);
    }

    #[test]
    fn crlf_endings_neither_pad_lengths_nor_shift_the_mapping() {
        // Arrange
        let (dedented, lines) = dedent("  one\r\n  two\r\n");

        // Assert: `two` sits at dedented [4, 7) and raw [9, 12) — the `\r`
        // bytes are separators, never columns.
        assert_eq!(dedented, "one\ntwo\n");
        assert_eq!(map_back(&lines, 4, 7), [(9, 12)]);
    }

    #[test]
    fn a_tab_indented_line_pins_the_dedent_to_zero() {
        // Arrange: mirrors the normalize rule — a tab is content.
        let (dedented, lines) = dedent("  spaced\n\ttabbed");

        // Assert
        assert_eq!(dedented, "  spaced\n\ttabbed");
        assert_eq!(map_back(&lines, 0, 2), [(0, 2)]);
    }

    #[test]
    fn encode_writes_the_protocol_deltas() {
        // Arrange: two tokens on one line, then one on a later line.
        let found = vec![
            Absolute {
                line: 0,
                start: 10,
                length: 4,
                token_type: TYPE_TOKEN,
                modifiers: DECLARATION_BIT,
            },
            Absolute {
                line: 0,
                start: 20,
                length: 2,
                token_type: CODE_TOKEN,
                modifiers: 0,
            },
            Absolute {
                line: 3,
                start: 5,
                length: 1,
                token_type: TYPE_TOKEN,
                modifiers: 0,
            },
        ];

        // Act
        let data = encode(&found);

        // Assert: same-line deltas are column-relative, a new line resets.
        assert_eq!(
            data,
            [
                SemanticToken {
                    delta_line: 0,
                    delta_start: 10,
                    length: 4,
                    token_type: TYPE_TOKEN,
                    token_modifiers_bitset: DECLARATION_BIT,
                },
                SemanticToken {
                    delta_line: 0,
                    delta_start: 10,
                    length: 2,
                    token_type: CODE_TOKEN,
                    token_modifiers_bitset: 0,
                },
                SemanticToken {
                    delta_line: 3,
                    delta_start: 5,
                    length: 1,
                    token_type: TYPE_TOKEN,
                    token_modifiers_bitset: 0,
                },
            ]
        );
    }

    /// Loads a one-file project around `argument` and hands back its tokens.
    fn tokens_of(argument: &str) -> SemanticTokens {
        let temp = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir_all(temp.path().join("argument")).expect("argument dir");
        std::fs::write(
            temp.path().join("inka.toml"),
            "[project]\nname = \"t\"\n\n[phases]\norder = [\"P0\"]\ncurrent = \"P0\"\n\n[values]\nfiles = []\n",
        )
        .expect("manifest");
        std::fs::write(temp.path().join("argument/a.inka"), argument).expect("argument");
        let project = inka_core::project::load_project(temp.path())
            .result
            .expect("the scenario loads");
        let (file, _) = project
            .source_map
            .files()
            .next()
            .expect("one file is loaded");
        full(&project, file).expect("the file is in the map")
    }

    #[test]
    fn nested_markup_emits_only_the_outermost_construct() {
        // Arrange & Act: a bold span containing inline code, in one block.
        let tokens = tokens_of("fiat leaf {\n  ```\n  holds **`tight`**\n  ```\n}\n");

        // Assert: the id plus one bold token covering `**`tight`**` whole —
        // no separate code token inside it.
        let types: Vec<u32> = tokens.data.iter().map(|token| token.token_type).collect();
        assert_eq!(types, [TYPE_TOKEN, BOLD_TOKEN]);
        // The whole of ``**`tight`**``, delimiters included: 11 columns.
        assert_eq!(tokens.data[1].length, 11);
    }

    #[test]
    fn multibyte_text_maps_tokens_onto_utf16_columns() {
        // Arrange & Act: three CJK characters (3 bytes, 1 UTF-16 unit each)
        // sit before the bold span.
        let tokens = tokens_of("fiat leaf {\n  ```\n  日本語 **太字** です\n  ```\n}\n");

        // Assert: the bold token starts at UTF-16 column 6 (two spaces, three
        // CJK units, one space) and is 6 units long — byte offsets would put
        // it at column 12 with length 10.
        let bold = &tokens.data[1];
        assert_eq!(
            (
                bold.delta_line,
                bold.delta_start,
                bold.length,
                bold.token_type
            ),
            (2, 6, 6, BOLD_TOKEN)
        );
    }
}
