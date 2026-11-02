//! Locating the project a document belongs to.
//!
//! The CLI is told its project directory; an editor is not, so the server
//! derives it: the nearest ancestor directory holding an `inka.toml` owns the
//! file. Nesting resolves to the innermost project, matching what `inka
//! check` would be run on from that directory.

use std::path::{Path, PathBuf};

/// The nearest ancestor directory of `file` that holds an `inka.toml`.
pub fn project_root(file: &Path) -> Option<PathBuf> {
    file.ancestors()
        .skip(1)
        .find(|dir| dir.join("inka.toml").is_file())
        .map(Path::to_path_buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_innermost_manifest_above_a_document() {
        // Arrange: outer and inner projects, with the document in the inner.
        let temp = tempfile::tempdir().expect("temp dir");
        let outer = temp.path();
        let inner = outer.join("sub/project");
        std::fs::create_dir_all(inner.join("argument")).expect("dirs");
        std::fs::write(outer.join("inka.toml"), "").expect("outer manifest");
        std::fs::write(inner.join("inka.toml"), "").expect("inner manifest");
        let document = inner.join("argument/a.inka");

        // Act & Assert
        assert_eq!(project_root(&document), Some(inner.clone()));
        assert_eq!(
            project_root(&inner.join("inka.toml")),
            Some(inner),
            "the manifest itself belongs to its own project"
        );
        assert_eq!(project_root(&outer.join("x.inka")), Some(outer.into()));
    }

    #[test]
    fn returns_none_outside_any_project() {
        // Arrange
        let temp = tempfile::tempdir().expect("temp dir");

        // Act & Assert
        assert_eq!(project_root(&temp.path().join("a.inka")), None);
    }
}
