//! One-pass `{$var}` interpolation over description text.
//!
//! Interpolation runs before every other consumer of descriptions:
//! normalization, hashing, and display all operate on the rendered text.
//!
//! Two rules are deliberate and fixed:
//!
//! - Only the exact pattern `{$name}` with `name` matching the identifier
//!   grammar `[A-Za-z_][A-Za-z0-9_-]*` is special. Anything else — `{$}`,
//!   `{$1a}`, an unterminated `{$` — is ordinary text, so no escape syntax is
//!   needed.
//! - Expansion is a single pass and never re-scans its own output. A value
//!   containing `{$...}` stays literal; recursive expansion would need an
//!   occurs-check for little gain.

use std::collections::HashMap;

use crate::ast::Description;
use crate::source::{Error, Span};

/// A description after `{$var}` expansion.
///
/// The pre-expansion `raw` text is deliberately not carried: every later pass
/// must see only the rendered text, and a type without `raw` cannot leak it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedDescription {
    rendered: String,
    span: Span,
}

impl RenderedDescription {
    /// The description text with every `{$var}` replaced by its value.
    #[must_use]
    pub fn rendered(&self) -> &str {
        &self.rendered
    }

    /// Span of the original fence content, for diagnostics about this
    /// description as a whole. Offsets within `rendered` cannot be mapped
    /// through it once an expansion changed the text length.
    #[must_use]
    pub fn span(&self) -> Span {
        self.span
    }
}

/// Expands `{$var}` references in a description.
///
/// # Errors
///
/// Returns one [`Error`] per `{$var}` whose name is not in `values`,
/// anchored to the exact `{$var}` byte range in the source file.
pub fn interpolate<S: std::hash::BuildHasher>(
    description: &Description,
    values: &HashMap<String, String, S>,
) -> Result<RenderedDescription, Vec<Error>> {
    let raw = &description.raw;
    let mut rendered = String::with_capacity(raw.len());
    let mut errors = Vec::new();
    let mut position = 0;

    while let Some(found) = raw[position..].find("{$") {
        let marker = position + found;
        rendered.push_str(&raw[position..marker]);
        let name_start = marker + 2;

        // `{`, `$`, and `}` are ASCII, so slicing at these offsets is safe
        // even when the surrounding text is multibyte. Text full of bare `{$`
        // with no `}` rescans to the end each time — quadratic, accepted for
        // hand-authored descriptions.
        let reference = raw[name_start..]
            .find('}')
            .map(|close| (&raw[name_start..name_start + close], name_start + close + 1))
            .filter(|(name, _)| is_variable_name(name));

        let Some((name, end)) = reference else {
            // Not the exact pattern: the `{$` is ordinary text. Rescanning
            // from just past it still catches a `{$` later in the skipped
            // range, since a new marker needs a fresh `{`.
            rendered.push_str("{$");
            position = name_start;
            continue;
        };

        match values.get(name) {
            Some(value) => rendered.push_str(value),
            None => errors.push(undefined_variable(description, name, marker, end)),
        }
        position = end;
    }
    rendered.push_str(&raw[position..]);

    if errors.is_empty() {
        Ok(RenderedDescription {
            rendered,
            span: description.span,
        })
    } else {
        Err(errors)
    }
}

/// Whether `name` matches the identifier grammar `[A-Za-z_][A-Za-z0-9_-]*`,
/// shared with values-file key validation so every defined key is referenceable.
pub(crate) fn is_variable_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Builds the diagnostic for an undefined `{$var}`. `start` and `end` are
/// byte offsets within the raw text; the description span locates them in
/// the source file.
fn undefined_variable(description: &Description, name: &str, start: usize, end: usize) -> Error {
    Error::new(
        Span {
            file_id: description.span.file_id,
            start: description.span.start + start,
            end: description.span.start + end,
        },
        format!("undefined variable `{name}`"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::SourceMap;

    /// Builds a description whose fence content starts at byte `start` of a
    /// synthetic file, mirroring how the parser fills in `Description`.
    fn description(raw: &str, start: usize) -> Description {
        let mut map = SourceMap::new();
        let file_id = map.add_file("test.inka".into(), format!("{}{raw}", " ".repeat(start)));
        Description {
            raw: raw.to_string(),
            info: None,
            span: Span {
                file_id,
                start,
                end: start + raw.len(),
            },
        }
    }

    fn values(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn expands_defined_variables() {
        // Arrange
        let description = description("uses {$scheme} and {$_curve}", 0);
        let values = values(&[("scheme", "sshsig"), ("_curve", "ed25519")]);

        // Act
        let rendered = interpolate(&description, &values).expect("defined variables expand");

        // Assert
        assert_eq!(rendered.rendered(), "uses sshsig and ed25519");
        assert_eq!(rendered.span(), description.span);
    }

    #[test]
    fn expands_hyphenated_variable_names() {
        // Arrange: identifiers allow `-` after the first character.
        let description = description("orbit: {$orbit-altitude}", 0);
        let values = values(&[("orbit-altitude", "500 km")]);

        // Act
        let rendered = interpolate(&description, &values).expect("hyphenated name expands");

        // Assert
        assert_eq!(rendered.rendered(), "orbit: 500 km");
    }

    #[test]
    fn reports_undefined_variable_with_source_span() {
        // Arrange: `{$missing}` occupies bytes 4..14 of the raw text.
        let description = description("see {$missing}!", 100);

        // Act
        let errors = interpolate(&description, &HashMap::new())
            .expect_err("undefined variable should be an error");

        // Assert
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].message, "undefined variable `missing`");
        let span = errors[0].span.expect("interpolation errors are anchored");
        assert_eq!(span.start, 104);
        assert_eq!(span.end, 114);
    }

    #[test]
    fn reports_every_undefined_variable() {
        // Arrange
        let description = description("{$a} {$b}", 0);

        // Act
        let errors = interpolate(&description, &HashMap::new())
            .expect_err("undefined variables should be errors");

        // Assert
        assert_eq!(errors.len(), 2);
        assert_eq!(errors[0].message, "undefined variable `a`");
        assert_eq!(errors[1].message, "undefined variable `b`");
    }

    #[test]
    fn locates_undefined_variable_after_multibyte_text_by_byte_offset() {
        // Arrange: "日本語 " is 10 bytes, so `{$v}` occupies bytes 10..14.
        let description = description("日本語 {$v}", 7);

        // Act
        let errors = interpolate(&description, &HashMap::new())
            .expect_err("undefined variable should be an error");

        // Assert
        let span = errors[0].span.expect("interpolation errors are anchored");
        assert_eq!(span.start, 17);
        assert_eq!(span.end, 21);
    }

    #[test]
    fn leaves_inexact_patterns_literal() {
        // Arrange
        let description = description("{$} {$1a} {$a b} {$tail", 0);

        // Act
        let rendered = interpolate(&description, &HashMap::new())
            .expect("inexact patterns are plain text, not errors");

        // Assert
        assert_eq!(rendered.rendered(), "{$} {$1a} {$a b} {$tail");
    }

    #[test]
    fn expands_marker_that_follows_an_inexact_pattern() {
        // Arrange: the scan must resume inside the skipped `{$1a}` range.
        let description = description("{$1a{$x}", 0);
        let values = values(&[("x", "1")]);

        // Act
        let rendered = interpolate(&description, &values).expect("second marker expands");

        // Assert
        assert_eq!(rendered.rendered(), "{$1a1");
    }

    #[test]
    fn does_not_reexpand_expanded_values() {
        // Arrange
        let description = description("{$x}", 0);
        let values = values(&[("x", "{$y}"), ("y", "z")]);

        // Act
        let rendered = interpolate(&description, &values).expect("single pass expands");

        // Assert
        assert_eq!(rendered.rendered(), "{$y}");
    }
}
