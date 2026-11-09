//! The formatter against the checked-in fixture projects: formatting never
//! changes what a project means, and `format_project` reports per file.

use std::io;
use std::path::{Path, PathBuf};

use inka_core::graph::Graph;
use inka_core::project::{LoadError, format_project, load_project_with};
use inka_core::read::{DiskText, ReadText};
use inka_core::view::ArgumentView;

mod common;
use common::fixture_dir;

/// The disk, except that some files read as different text.
struct Overlay {
    files: Vec<(PathBuf, String)>,
}

impl ReadText for Overlay {
    fn read_text(&self, path: &Path) -> io::Result<String> {
        match self.files.iter().find(|(overlaid, _)| overlaid == path) {
            Some((_, text)) => Ok(text.clone()),
            None => DiskText.read_text(path),
        }
    }
}

fn view_through(project_dir: &Path, reader: &dyn ReadText) -> ArgumentView {
    let project = load_project_with(project_dir, reader)
        .result
        .unwrap_or_else(|error| panic!("{} should load: {error}", project_dir.display()));
    let graph = Graph::build(&project.blocks).expect("fixture builds a graph");
    ArgumentView::build(&graph, &project.source_map)
}

/// Every argument file of `project_dir` in its formatted form, whether or
/// not it was already formatted.
fn formatted_files(project_dir: &Path) -> Vec<(PathBuf, String)> {
    let format = format_project(project_dir, &DiskText).expect("fixture formats");
    format
        .files
        .into_iter()
        .map(|file| {
            let text = match file.result.expect("fixture parses") {
                Some(formatted) => formatted,
                None => std::fs::read_to_string(&file.path).expect("fixture reads"),
            };
            (file.path, text)
        })
        .collect()
}

#[test]
fn formatting_a_fixture_changes_no_node_of_its_view() {
    for name in ["aocs", "dogfood"] {
        // Arrange
        let project_dir = fixture_dir(name);
        let formatted = Overlay {
            files: formatted_files(&project_dir),
        };

        // Act
        let before = view_through(&project_dir, &DiskText);
        let after = view_through(&project_dir, &formatted);

        // Assert: the view is every input of every view hash.
        assert_eq!(after, before, "{name}: formatting changed the view");
    }
}

#[test]
fn formatting_a_formatted_fixture_changes_nothing() {
    for name in ["aocs", "dogfood"] {
        // Arrange
        let project_dir = fixture_dir(name);
        let formatted = Overlay {
            files: formatted_files(&project_dir),
        };

        // Act
        let again = format_project(&project_dir, &formatted).expect("fixture formats");

        // Assert
        for file in again.files {
            let result = file.result.expect("formatted text parses");
            assert_eq!(result, None, "{} is not a fixed point", file.path.display());
        }
    }
}

#[test]
fn a_file_that_does_not_parse_is_reported_and_the_others_are_still_formatted() {
    // Arrange
    let project_dir = fixture_dir("aocs");
    let broken = project_dir.join("argument/momentum.inka");
    let reader = Overlay {
        files: vec![(broken.clone(), "structure {\n".to_owned())],
    };

    // Act
    let format = format_project(&project_dir, &reader).expect("the project loads");

    // Assert
    assert!(format.files.len() > 1);
    for file in &format.files {
        if file.path == broken {
            let errors = file
                .result
                .as_ref()
                .expect_err("the broken file is reported");
            assert_ne!(errors.as_slice(), []);
        } else {
            assert!(file.result.is_ok(), "{} should format", file.path.display());
        }
    }
}

#[test]
fn a_project_without_a_manifest_is_not_formatted() {
    // Arrange
    let project_dir = fixture_dir("no-such-project");

    // Act
    let result = format_project(&project_dir, &DiskText);

    // Assert
    assert!(matches!(result, Err(LoadError::Read { .. })));
}
