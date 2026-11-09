//! Fence content: re-indented to the fence's level, unless that would change
//! the contract.
//!
//! The view hash reads a description after interpolation and normalization,
//! and normalization already strips the common indent, so moving every line
//! of the fence by the same amount usually changes nothing. It can change the
//! hash when a value spanning several lines is interpolated: the value's own
//! continuation lines do not move with the fence, so the common indent of the
//! rendered text can shift. Rather than reason about when that happens, the
//! candidate is rendered and normalized against the project's values and kept
//! only when it reads exactly like the original.

use std::borrow::Cow;
use std::collections::HashMap;
use std::hash::BuildHasher;

use crate::ast::Description;
use crate::interpolate::interpolate;
use crate::normalize;

/// The text to write between the fences of `description`: its content
/// re-indented to `indent`, or, when that would change the contract or the
/// contract cannot be rendered, the content exactly as written.
pub(super) fn content<'a, S: BuildHasher>(
    description: &'a Description,
    indent: &str,
    values: &HashMap<String, String, S>,
) -> Cow<'a, str> {
    let candidate = reindent(&description.raw, indent);
    if candidate == description.raw || preserves_contract(description, &candidate, values) {
        Cow::Owned(candidate)
    } else {
        Cow::Borrowed(&description.raw)
    }
}

/// Removes the common leading-space indent of `raw` and puts every non-blank
/// line at `indent`; blank lines become empty. Every line ends with `\n`.
fn reindent(raw: &str, indent: &str) -> String {
    let lines: Vec<&str> = split_lines(raw).map(str::trim_end).collect();
    let common = lines
        .iter()
        .filter(|line| !line.is_empty())
        .map(|line| line.len() - line.trim_start_matches(' ').len())
        .min()
        .unwrap_or(0);
    let mut out = String::with_capacity(raw.len() + lines.len() * indent.len());
    for line in lines {
        if !line.is_empty() {
            // `common` never exceeds this line's run of leading spaces, so
            // the slice starts on a character boundary.
            out.push_str(indent);
            out.push_str(&line[common..]);
        }
        out.push('\n');
    }
    out
}

/// The lines of `raw`, split at LF, CRLF, or a lone CR — the line breaks the
/// lexer recognizes — without the terminators. A final terminator ends the
/// last line rather than starting an empty one.
fn split_lines(raw: &str) -> impl Iterator<Item = &str> {
    let mut rest = raw;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let Some(end) = rest.find(['\r', '\n']) else {
            let line = rest;
            rest = "";
            return Some(line);
        };
        let line = &rest[..end];
        let terminator = if rest[end..].starts_with("\r\n") {
            2
        } else {
            1
        };
        rest = &rest[end + terminator..];
        Some(line)
    })
}

/// Whether `candidate` renders and normalizes to the same contract as the
/// original content of `description`. A content that cannot be rendered —
/// it names an undefined value — is never judged preserved.
fn preserves_contract<S: BuildHasher>(
    description: &Description,
    candidate: &str,
    values: &HashMap<String, String, S>,
) -> bool {
    let moved = Description {
        raw: candidate.to_owned(),
        ..description.clone()
    };
    match (
        interpolate(description, values),
        interpolate(&moved, values),
    ) {
        (Ok(before), Ok(after)) => {
            normalize::description(before.rendered()) == normalize::description(after.rendered())
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::{SourceMap, Span};

    const INDENT: &str = "    ";

    fn description(raw: &str) -> Description {
        let mut map = SourceMap::new();
        let file_id = map.add_file("test.inka".into(), raw.to_owned());
        Description {
            raw: raw.to_owned(),
            info: None,
            fence_length: 3,
            span: Span {
                file_id,
                start: 0,
                end: raw.len(),
            },
        }
    }

    fn values(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
            .collect()
    }

    #[test]
    fn reindents_to_the_fence_level_keeping_relative_indent() {
        // Arrange
        let raw = "  first\n    nested\n\n  last\n";

        // Act
        let out = reindent(raw, INDENT);

        // Assert
        assert_eq!(out, "    first\n      nested\n\n    last\n");
    }

    #[test]
    fn strips_trailing_whitespace_and_empties_blank_lines() {
        // Arrange
        let raw = "a  \n \t \nb\t\n";

        // Act
        let out = reindent(raw, INDENT);

        // Assert
        assert_eq!(out, "    a\n\n    b\n");
    }

    #[test]
    fn treats_crlf_and_lone_cr_as_line_breaks() {
        // Arrange
        let raw = "  a\r\n  b\r  c\n";

        // Act
        let out = reindent(raw, INDENT);

        // Assert
        assert_eq!(out, "    a\n    b\n    c\n");
    }

    #[test]
    fn a_tab_is_content_so_it_pins_the_common_indent_to_zero() {
        // Arrange
        let raw = "\ttabbed\n  spaced\n";

        // Act
        let out = reindent(raw, INDENT);

        // Assert
        assert_eq!(out, "    \ttabbed\n      spaced\n");
    }

    #[test]
    fn empty_content_stays_empty() {
        assert_eq!(reindent("", INDENT), "");
    }

    #[test]
    fn content_is_reindented_when_single_line_values_are_interpolated() {
        // Arrange
        let description = description("at most {$limit}\n");

        // Act
        let out = content(&description, INDENT, &values(&[("limit", "5 m")]));

        // Assert
        assert_eq!(out, "    at most {$limit}\n");
    }

    #[test]
    fn content_is_kept_as_written_when_a_multi_line_value_would_shift_the_indent() {
        // Arrange: the value's second line stays at column 0 while the fence
        // line would move to column 4, so the rendered text would read
        // "    - a\n- b" instead of "- a\n- b".
        let raw = "{$list}\n";
        let description = description(raw);

        // Act
        let out = content(&description, INDENT, &values(&[("list", "- a\n- b")]));

        // Assert
        assert_eq!(out, raw);
    }

    #[test]
    fn content_naming_an_undefined_value_is_kept_as_written() {
        // Arrange
        let raw = "  uses {$missing}\n";
        let description = description(raw);

        // Act
        let out = content(&description, INDENT, &values(&[]));

        // Assert
        assert_eq!(out, raw);
    }

    #[test]
    fn already_formatted_content_naming_an_undefined_value_is_unchanged() {
        // Arrange
        let raw = "    uses {$missing}\n";
        let description = description(raw);

        // Act
        let out = content(&description, INDENT, &values(&[]));

        // Assert
        assert_eq!(out, raw);
    }
}
