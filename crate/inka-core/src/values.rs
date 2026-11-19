//! Values files: TOML maps whose entries resolve `{$var}` and dotted
//! `{$table.key}` paths in descriptions.
//!
//! The files listed under `[values] files` are merged in order. A key defined
//! by more than one file is an error rather than a last-one-wins rule:
//! contracts are hashed after interpolation, so a silent override would
//! change hashes without any visible signal about which definition applied.
//! The rule applies to top-level keys only: one file owns a whole subtree.
//!
//! A nested table is addressable by a dotted path through its keys, so a file
//! grouped by area (`[instrument]`, `[requirement]`) can be used as it is.
//! Paths are resolved here, once: the merged map holds an entry for every
//! reachable path, which keeps interpolation a plain lookup.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::interpolate::is_variable_name;
use crate::read::ReadText;

/// A reason values files could not be loaded or merged.
#[derive(Debug, thiserror::Error)]
pub enum ValuesError {
    #[error("failed to read values file {}: {source}", .path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid values file {}: {source}", .path.display())]
    Toml {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("value key `{key}` is defined in both {} and {}", .first.display(), .second.display())]
    DuplicateKey {
        key: String,
        first: PathBuf,
        second: PathBuf,
    },
    #[error(
        "value key `{key}` in {} cannot be referenced: `{{$var}}` names match [A-Za-z_][A-Za-z0-9_-]*",
        .path.display()
    )]
    InvalidKey { key: String, path: PathBuf },
}

/// Reads and merges the values files listed in a manifest.
///
/// Relative paths are resolved against `manifest_dir`, the directory holding
/// `inka.toml`. Texts come through `reader`, so an editor can substitute its
/// open buffers for the disk.
///
/// # Errors
///
/// Returns a [`ValuesError`] when a file cannot be read, is not valid TOML,
/// or defines a key that another values file already defined.
pub fn load_values(
    manifest_dir: &Path,
    files: &[PathBuf],
    reader: &dyn ReadText,
) -> Result<HashMap<String, String>, ValuesError> {
    let texts = files
        .iter()
        .map(|relative| {
            let path = manifest_dir.join(relative);
            let text = reader
                .read_text(&path)
                .map_err(|source| ValuesError::Read {
                    path: path.clone(),
                    source,
                })?;
            Ok((path, text))
        })
        .collect::<Result<Vec<_>, ValuesError>>()?;
    merge_value_files(&texts)
}

/// Merges already-read values files into one map from value path to text.
///
/// Values are interpolated into description text, so everything becomes a
/// string here: a string value is taken verbatim, and any other TOML value —
/// including arrays and tables, e.g. a vector requirement — keeps its TOML
/// spelling (`42`, `true`, `[1, 2, 3]`). Each top-level key must match the
/// `{$var}` name grammar — a key that could never be referenced would
/// otherwise sit as a silently dead entry.
///
/// A table value is also entered under the dotted path of each of its keys,
/// recursively (`instrument.diameter_cm`), next to its own entry, which still
/// holds the whole table in TOML notation. A nested key that does not match
/// the name grammar has no path, but stays visible in its table's text.
/// Arrays are not indexed: an array, even of tables, is one value.
///
/// Pure so that merging and validation rules are testable without touching
/// the filesystem.
///
/// # Errors
///
/// Returns a [`ValuesError`] when a file is not valid TOML, a key is defined
/// by more than one file, or a key is not a valid `{$var}` name.
pub fn merge_value_files(
    files: &[(PathBuf, String)],
) -> Result<HashMap<String, String>, ValuesError> {
    let mut values = HashMap::new();
    let mut origins: HashMap<String, &Path> = HashMap::new();
    for (path, text) in files {
        let table: toml::Table = toml::from_str(text).map_err(|source| ValuesError::Toml {
            path: path.clone(),
            source,
        })?;
        for (key, value) in table {
            if !is_variable_name(&key) {
                return Err(ValuesError::InvalidKey {
                    key,
                    path: path.clone(),
                });
            }
            if let Some(first) = origins.get(key.as_str()) {
                return Err(ValuesError::DuplicateKey {
                    key,
                    first: first.to_path_buf(),
                    second: path.clone(),
                });
            }
            origins.insert(key.clone(), path);
            insert_paths(&mut values, key, &value);
        }
    }
    Ok(values)
}

/// Enters `value` under `path` and, when it is a table, each referenceable
/// key below it under its dotted path.
///
/// Top-level keys are unique and contain no `.`, and nested keys are only
/// followed when they match the name grammar, so no two paths can collide.
fn insert_paths(values: &mut HashMap<String, String>, path: String, value: &toml::Value) {
    if let toml::Value::Table(table) = value {
        for (key, child) in table {
            if is_variable_name(key) {
                insert_paths(values, format!("{path}.{key}"), child);
            }
        }
    }
    values.insert(path, stringify(value));
}

fn stringify(value: &toml::Value) -> String {
    match value {
        toml::Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(name: &str, text: &str) -> (PathBuf, String) {
        (PathBuf::from(name), text.to_string())
    }

    #[test]
    fn merges_keys_across_files() {
        // Arrange
        let files = [file("a.toml", "x = \"1\""), file("b.toml", "y = \"2\"")];

        // Act
        let values = merge_value_files(&files).expect("disjoint files should merge");

        // Assert
        assert_eq!(values.get("x").map(String::as_str), Some("1"));
        assert_eq!(values.get("y").map(String::as_str), Some("2"));
        assert_eq!(values.len(), 2);
    }

    #[test]
    fn accepts_hyphenated_keys() {
        // Arrange: identifiers allow `-` after the first character.
        let files = [file("a.toml", "orbit-altitude = \"500 km\"")];

        // Act
        let values = merge_value_files(&files).expect("hyphenated key should merge");

        // Assert
        assert_eq!(
            values.get("orbit-altitude").map(String::as_str),
            Some("500 km")
        );
    }

    #[test]
    fn stringifies_non_string_values_with_their_toml_spelling() {
        // Arrange
        let files = [file(
            "a.toml",
            "count = 42\nenabled = true\ninertia = [1.5, 2.0, 3.25]",
        )];

        // Act
        let values = merge_value_files(&files).expect("non-string values should merge");

        // Assert
        assert_eq!(values.get("count").map(String::as_str), Some("42"));
        assert_eq!(values.get("enabled").map(String::as_str), Some("true"));
        assert_eq!(
            values.get("inertia").map(String::as_str),
            Some("[1.5, 2.0, 3.25]")
        );
    }

    #[test]
    fn stringifies_table_values_inline_with_sorted_keys() {
        // Arrange: only the keys must be flat; a table value is just text.
        let files = [file("a.toml", "limits = { min = 1, max = 2 }")];

        // Act
        let values = merge_value_files(&files).expect("table values should merge");

        // Assert: keys come out sorted (`toml::Table` is ordered by key), so
        // the rendered text is independent of the spelling order in the file.
        assert_eq!(
            values.get("limits").map(String::as_str),
            Some("{ max = 2, min = 1 }")
        );
    }

    #[test]
    fn enters_every_nested_key_under_its_dotted_path() {
        // Arrange
        let files = [file(
            "a.toml",
            "top = 1\n\n[instrument]\ndiameter_cm = 30\n\n[instrument.optics]\nf-number = \"f/8\"\n",
        )];

        // Act
        let values = merge_value_files(&files).expect("nested tables should merge");

        // Assert
        assert_eq!(values.get("top").map(String::as_str), Some("1"));
        assert_eq!(
            values.get("instrument.diameter_cm").map(String::as_str),
            Some("30")
        );
        assert_eq!(
            values.get("instrument.optics.f-number").map(String::as_str),
            Some("f/8")
        );
        assert_eq!(
            values.get("instrument.optics").map(String::as_str),
            Some("{ f-number = \"f/8\" }")
        );
        assert_eq!(values.len(), 5);
    }

    #[test]
    fn a_table_keeps_its_toml_notation_next_to_its_paths() {
        // Arrange
        let files = [file("a.toml", "limits = { min = 1, max = 2 }")];

        // Act
        let values = merge_value_files(&files).expect("table values should merge");

        // Assert
        assert_eq!(
            values.get("limits").map(String::as_str),
            Some("{ max = 2, min = 1 }")
        );
        assert_eq!(values.get("limits.min").map(String::as_str), Some("1"));
        assert_eq!(values.get("limits.max").map(String::as_str), Some("2"));
    }

    #[test]
    fn a_nested_key_outside_the_name_grammar_has_no_path() {
        // Arrange: `"a.b"` would read as two segments, and `10x` is no name.
        let files = [file("a.toml", "[t]\n\"a.b\" = 1\n10x = 2\nok = 3\n")];

        // Act
        let values = merge_value_files(&files).expect("the table itself is still a value");

        // Assert
        assert_eq!(values.get("t.ok").map(String::as_str), Some("3"));
        assert!(!values.contains_key("t.a.b"));
        assert!(!values.contains_key("t.10x"));
        assert_eq!(
            values.get("t").map(String::as_str),
            Some("{ 10x = 2, \"a.b\" = 1, ok = 3 }")
        );
    }

    #[test]
    fn arrays_are_not_indexed() {
        // Arrange
        let files = [file("a.toml", "[[mode]]\nname = \"safe\"\n")];

        // Act
        let values = merge_value_files(&files).expect("arrays of tables should merge");

        // Assert
        assert_eq!(values.len(), 1);
        assert!(values.contains_key("mode"));
    }

    #[test]
    fn rejects_top_level_table_defined_in_two_files() {
        // Arrange: disjoint nested keys still collide at the top level.
        let files = [
            file("a.toml", "[instrument]\nmass = 1\n"),
            file("b.toml", "[instrument]\npower = 2\n"),
        ];

        // Act
        let error = merge_value_files(&files).expect_err("one file owns a subtree");

        // Assert
        let ValuesError::DuplicateKey { key, .. } = error else {
            panic!("expected DuplicateKey, got {error:?}");
        };
        assert_eq!(key, "instrument");
    }

    #[test]
    fn rejects_key_that_cannot_be_referenced() {
        // Arrange: `10x` is a legal TOML key but never matches `{$10x}`.
        let files = [file("a.toml", "10x = \"1\"")];

        // Act
        let error = merge_value_files(&files).expect_err("unreferenceable key should be rejected");

        // Assert
        let ValuesError::InvalidKey { key, path } = error else {
            panic!("expected InvalidKey, got {error:?}");
        };
        assert_eq!(key, "10x");
        assert_eq!(path, PathBuf::from("a.toml"));
    }

    #[test]
    fn rejects_key_defined_in_two_files() {
        // Arrange
        let files = [file("a.toml", "x = \"1\""), file("b.toml", "x = \"2\"")];

        // Act
        let error = merge_value_files(&files).expect_err("colliding key should be rejected");

        // Assert
        let ValuesError::DuplicateKey { key, first, second } = error else {
            panic!("expected DuplicateKey, got {error:?}");
        };
        assert_eq!(key, "x");
        assert_eq!(first, PathBuf::from("a.toml"));
        assert_eq!(second, PathBuf::from("b.toml"));
    }

    #[test]
    fn rejects_invalid_toml() {
        // Arrange
        let files = [file("a.toml", "x = ")];

        // Act
        let error = merge_value_files(&files).expect_err("invalid TOML should be rejected");

        // Assert
        assert!(matches!(error, ValuesError::Toml { .. }));
        assert!(error.to_string().contains("a.toml"));
    }
}
