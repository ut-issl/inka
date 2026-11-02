//! Values files: flat TOML maps whose entries resolve `{$var}` in
//! descriptions.
//!
//! The files listed under `[values] files` are merged in order. A key defined
//! by more than one file is an error rather than a last-one-wins rule:
//! contracts are hashed after interpolation, so a silent override would
//! change hashes without any visible signal about which definition applied.

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

/// Merges already-read values files into one flat map.
///
/// Values are interpolated into description text, so everything becomes a
/// string here: a string value is taken verbatim, and any other TOML value —
/// including arrays and tables, e.g. a vector requirement — keeps its TOML
/// spelling (`42`, `true`, `[1, 2, 3]`). Only the keys must be flat, and each
/// key must match the `{$var}` name grammar — a key that could never be
/// referenced would otherwise sit as a silently dead entry. Pure so that
/// merging and validation rules are testable without touching the filesystem.
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
            values.insert(key, stringify(value));
        }
    }
    Ok(values)
}

fn stringify(value: toml::Value) -> String {
    match value {
        toml::Value::String(text) => text,
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
