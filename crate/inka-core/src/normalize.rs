//! The canonical normal form a contract is read and hashed in.
//!
//! Normalization sits between interpolation and everything downstream: a
//! description is rendered, normalized once here, and only then displayed or
//! hashed. Because both sides share this function, what a reviewer reads and
//! what their approval is bound to cannot drift apart.
//!
//! The steps are fixed by `spec.md` and, once golden vectors land, frozen:
//! NFC, CRLF/CR to LF, per-line trailing whitespace stripped, leading and
//! trailing blank lines dropped, the common space indent removed, joined with
//! `\n`. Every step is idempotent, so normalizing normalized text is a no-op.
//!
//! What is deliberately *not* normalized is as much of the contract as what
//! is. Internal newlines and internal spacing are content: a list that breaks
//! across lines says something a single run-on line does not, so the text is
//! kept near-verbatim and no markdown-AST normalization happens. Only the
//! artefacts of writing text inside an indented fence are removed.

use unicode_normalization::UnicodeNormalization;

/// Puts a rendered description into its canonical form.
///
/// The input is post-interpolation text: hashing never sees a `{$var}`, and
/// neither does this.
#[must_use]
pub fn description(text: &str) -> String {
    // NFC first: composition can change byte lengths, and every later step
    // measures the text in bytes.
    let composed: String = text.nfc().collect();
    let unified = composed.replace("\r\n", "\n").replace('\r', "\n");
    let trimmed: Vec<&str> = unified.lines().map(str::trim_end).collect();

    // Blank lines around the contract are fence padding, not content.
    let Some(first) = trimmed.iter().position(|line| !line.is_empty()) else {
        return String::new();
    };
    let last = trimmed
        .iter()
        .rposition(|line| !line.is_empty())
        .unwrap_or(first);
    let lines = &trimmed[first..=last];

    let indent = common_indent(lines);
    lines
        .iter()
        .map(|line| {
            if line.is_empty() {
                line
            } else {
                &line[indent..]
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Leading spaces every non-blank line shares.
///
/// Only spaces count, as `spec.md` says: a tab is content, not indentation,
/// so a tab-indented line contributes an indent of zero and pins the common
/// indent to zero for the whole description. Because the result never exceeds
/// any non-blank line's own run of leading spaces, slicing a line at it always
/// lands on a character boundary.
fn common_indent(lines: &[&str]) -> usize {
    lines
        .iter()
        .filter(|line| !line.is_empty())
        .map(|line| line.len() - line.trim_start_matches(' ').len())
        .min()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_the_fence_indent_and_keeps_the_relative_one() {
        // Arrange: a description indented inside its block, with a nested list
        // whose relative indent is contract content.
        let raw = "\n  In safe mode the satellite holds:\n\n  - the sun angle within 30 deg\n    - measured from the array normal\n";

        // Act
        let normalized = description(raw);

        // Assert
        assert_eq!(
            normalized,
            "In safe mode the satellite holds:\n\n- the sun angle within 30 deg\n  - measured from the array normal"
        );
    }

    #[test]
    fn unifies_line_endings_and_strips_trailing_whitespace() {
        // Arrange: CRLF, a lone CR, trailing spaces, and blank padding lines.
        let raw = "\r\n  first line   \r\n\r\n    second line\r  \r\n";

        // Act
        let normalized = description(raw);

        // Assert
        assert_eq!(normalized, "first line\n\n  second line");
    }

    #[test]
    fn composes_decomposed_characters_to_nfc() {
        // Arrange: "é" written as `e` plus a combining acute accent.
        let raw = "the de\u{0301}bris limit";

        // Act
        let normalized = description(raw);

        // Assert
        assert_eq!(normalized, "the d\u{00e9}bris limit");
    }

    #[test]
    fn is_idempotent() {
        let raw = "\r\n  first line   \r\n\r\n    de\u{0301}bris\r\n  \r\n";

        let once = description(raw);
        let twice = description(&once);

        assert_eq!(twice, once);
    }

    #[test]
    fn keeps_internal_blank_lines_and_spacing_as_content() {
        // Arrange: paragraph breaks and a hanging indent are part of what was
        // approved, so neither is collapsed.
        let raw = "first\n\n\nsecond\n    third";

        assert_eq!(description(raw), "first\n\n\nsecond\n    third");
    }

    #[test]
    fn a_description_of_only_blank_lines_normalizes_to_nothing() {
        assert_eq!(description("\n   \n\n"), "");
    }

    #[test]
    fn a_tab_indented_line_pins_the_common_indent_to_zero() {
        // Arrange: only spaces are fence indentation; a tab is content.
        let raw = "  spaced\n\ttabbed";

        assert_eq!(description(raw), "  spaced\n\ttabbed");
    }

    #[test]
    fn slices_a_multibyte_line_at_a_character_boundary() {
        // Arrange: the indent is measured in bytes, and the first non-space
        // byte of each line starts a character.
        let raw = "  日本語\n  ascii";

        assert_eq!(description(raw), "日本語\nascii");
    }
}
