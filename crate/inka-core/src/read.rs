//! How project loading obtains file *contents*, separated from which files
//! exist.
//!
//! The seam exists for editors: a language server must analyze what is on
//! screen, not what was last saved, so it loads through a reader that serves
//! open buffers and falls back to the disk. Directory enumeration is not part
//! of the seam — which files exist is filesystem state either way, and an
//! unsaved buffer always shadows a file that already has a path.

use std::path::Path;

/// Supplies the text of a file by path.
pub trait ReadText {
    /// Reads the full text of `path`.
    ///
    /// # Errors
    ///
    /// Returns an [`std::io::Error`] when the text cannot be produced, shaped
    /// exactly like a filesystem read failure so callers classify overlay and
    /// disk errors identically.
    fn read_text(&self, path: &Path) -> std::io::Result<String>;
}

/// The disk itself: what [`crate::project::load_project`] reads through.
#[derive(Debug, Clone, Copy, Default)]
pub struct DiskText;

impl ReadText for DiskText {
    fn read_text(&self, path: &Path) -> std::io::Result<String> {
        std::fs::read_to_string(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_text_reads_an_existing_file_and_reports_a_missing_one() {
        // Arrange
        let dir = std::env::temp_dir();
        let path = dir.join(format!("inka-read-text-{}", std::process::id()));
        std::fs::write(&path, "contents").expect("write test file");

        // Act
        let read = DiskText.read_text(&path);
        let missing = DiskText.read_text(&dir.join("inka-read-text-missing"));

        // Assert
        assert_eq!(read.expect("file exists"), "contents");
        assert_eq!(
            missing.expect_err("file is missing").kind(),
            std::io::ErrorKind::NotFound
        );
        std::fs::remove_file(&path).expect("remove test file");
    }
}
