//! The buffers the editor has open, as the content source for analysis.
//!
//! An entry exists exactly while the editor owns the document's content:
//! `didOpen`/`didChange` install the on-screen text, `didClose` hands the
//! file back to the disk. Analysis reads through [`ReadText`], so an open
//! buffer shadows its saved file and everything else falls through to disk.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use inka_core::read::{DiskText, ReadText};

/// Open-document texts keyed by filesystem path.
#[derive(Debug, Default)]
pub struct Overlays {
    texts: HashMap<PathBuf, String>,
}

impl Overlays {
    /// Installs or replaces the editor's text for a document.
    pub fn open(&mut self, path: PathBuf, text: String) {
        self.texts.insert(path, text);
    }

    /// Releases a document back to its on-disk content.
    pub fn close(&mut self, path: &Path) {
        self.texts.remove(path);
    }

    /// Paths of every open document, the set the server keeps publishing
    /// diagnostics for even when they are clean.
    pub fn paths(&self) -> impl Iterator<Item = &PathBuf> {
        self.texts.keys()
    }
}

impl ReadText for Overlays {
    fn read_text(&self, path: &Path) -> std::io::Result<String> {
        match self.texts.get(path) {
            Some(text) => Ok(text.clone()),
            None => DiskText.read_text(path),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_open_buffer_shadows_the_disk_until_closed() {
        // Arrange
        let temp = tempfile::tempdir().expect("temp dir");
        let path = temp.path().join("a.inka");
        std::fs::write(&path, "disk").expect("write file");
        let mut overlays = Overlays::default();

        // Act & Assert: buffer wins while open, disk returns after close.
        overlays.open(path.clone(), "buffer".to_owned());
        assert_eq!(overlays.read_text(&path).expect("overlaid read"), "buffer");
        overlays.close(&path);
        assert_eq!(overlays.read_text(&path).expect("disk read"), "disk");
    }
}
