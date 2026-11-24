//! Loading a project all the way to the shared view, as one operation.
//!
//! Every caller that presents an argument — the commands, the server — needs
//! the same thing: a project that loads, a graph that builds, and a shape that
//! validates. [`load_argument`] does that once and answers with data: either
//! the [`Argument`] or the findings that [`Rejected`] it. What to do with the
//! findings — print them, send them to a browser — is the caller's business,
//! so nothing here writes anywhere.
//!
//! The bar is deliberately `inka check`'s graph stage: errors refuse, warnings
//! do not. An argument that fails it would present as something no consumer
//! expects — a graph with a cycle, or one with no objective to hang from — and
//! every renderer downstream assumes those away.

use std::path::Path;

use crate::evidence::EvidenceSet;
use crate::graph::Graph;
use crate::manifest::Manifest;
use crate::project::{LoadError, LoadOutcome, load_project_with};
use crate::read::{DiskText, ReadText};
use crate::source::{Diagnostics, Error, SourceMap, Warning};
use crate::view::ArgumentView;

/// A loaded project reduced to what its consumers read: the manifest's
/// rule-side state, the imported test evidence, and the shared argument view.
#[derive(Debug, Clone)]
pub struct Argument {
    pub manifest: Manifest,
    pub evidence: EvidenceSet,
    pub view: ArgumentView,
    /// Resolves the spans of [`Argument::warnings`].
    pub source_map: SourceMap,
    /// Suspicions raised on the way; the argument is well-formed despite them.
    pub warnings: Vec<Warning>,
}

/// Why a project has no presentable argument.
#[derive(Debug, Clone)]
pub struct Rejected {
    /// Resolves the spans of the findings; empty when the failure is
    /// structural and nothing was parsed.
    pub source_map: SourceMap,
    /// At least one error, plus every warning the run saw before it stopped.
    pub diagnostics: Diagnostics,
}

/// Loads and projects the project rooted at `project_dir`.
///
/// # Errors
///
/// Returns [`Rejected`] when the project cannot be loaded, its graph cannot be
/// built, or its shape does not validate. A structural failure — a missing
/// manifest, an unreadable file — is one unanchored error.
pub fn load_argument(project_dir: &Path) -> Result<Argument, Rejected> {
    load_argument_with(project_dir, &DiskText)
}

/// [`load_argument`] with the content source swapped out; see
/// [`load_project_with`].
///
/// # Errors
///
/// As [`load_argument`].
pub fn load_argument_with(project_dir: &Path, reader: &dyn ReadText) -> Result<Argument, Rejected> {
    let LoadOutcome { warnings, result } = load_project_with(project_dir, reader);
    let project = match result {
        Ok(project) => project,
        Err(LoadError::Invalid { source_map, errors }) => {
            return Err(rejected(source_map, errors, warnings));
        }
        Err(other) => {
            return Err(rejected(
                SourceMap::new(),
                vec![Error::unanchored(other.to_string())],
                warnings,
            ));
        }
    };

    let graph = match Graph::build(&project.blocks) {
        Ok(graph) => graph,
        Err(errors) => return Err(rejected(project.source_map, errors, warnings)),
    };
    let shape = graph.validate();
    let warnings = [warnings, shape.warnings].concat();
    if !shape.errors.is_empty() {
        return Err(rejected(project.source_map, shape.errors, warnings));
    }

    let view = ArgumentView::build(&graph, &project.source_map);
    Ok(Argument {
        manifest: project.manifest,
        evidence: project.evidence,
        view,
        source_map: project.source_map,
        warnings,
    })
}

fn rejected(source_map: SourceMap, errors: Vec<Error>, warnings: Vec<Warning>) -> Rejected {
    Rejected {
        source_map,
        diagnostics: Diagnostics { errors, warnings },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs;

    fn project(argument: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("a temporary directory");
        fs::write(
            dir.path().join("inka.toml"),
            "[project]\nname = \"t\"\n\n[phases]\norder = [\"M1\"]\ncurrent = \"M1\"\n\n[values]\nfiles = []\n",
        )
        .expect("the manifest is written");
        fs::create_dir(dir.path().join("argument")).expect("the argument directory is created");
        fs::write(dir.path().join("argument/a.inka"), argument).expect("the argument is written");
        dir
    }

    #[test]
    fn a_checked_project_loads_to_its_view() {
        // Arrange
        let dir = project(
            "objective root {\n    ```\n    the claim\n    ```\n    children:\n        leaf\n}\n\n\
             fiat leaf {\n    ```\n    given\n    ```\n}\n",
        );

        // Act
        let argument = load_argument(dir.path()).expect("the project loads");

        // Assert
        assert_eq!(argument.view.roots, ["root"]);
        assert_eq!(argument.view.nodes.len(), 2);
        assert_eq!(argument.manifest.phases.current, "M1");
        assert_eq!(argument.warnings, []);
    }

    #[test]
    fn a_syntax_error_is_rejected_with_its_span_resolvable() {
        // Arrange
        let dir = project("objective root {\n");

        // Act
        let rejected = load_argument(dir.path()).expect_err("the project is broken");

        // Assert
        let error = rejected.diagnostics.errors.first().expect("an error");
        let span = error.span.expect("a syntax error is anchored");
        assert!(rejected.source_map.file(span.file_id).is_some());
    }

    #[test]
    fn an_undefined_child_is_rejected_at_the_graph_stage() {
        // Arrange
        let dir = project(
            "objective root {\n    ```\n    the claim\n    ```\n    children:\n        absent\n}\n",
        );

        // Act
        let rejected = load_argument(dir.path()).expect_err("the reference dangles");

        // Assert
        assert!(rejected.diagnostics.has_errors());
    }

    #[test]
    fn a_missing_manifest_is_one_unanchored_error() {
        // Arrange
        let dir = tempfile::tempdir().expect("a temporary directory");

        // Act
        let rejected = load_argument(dir.path()).expect_err("there is no project");

        // Assert
        assert_eq!(rejected.diagnostics.errors.len(), 1);
        assert!(rejected.diagnostics.errors[0].span.is_none());
        assert_eq!(rejected.diagnostics.warnings, []);
    }
}
