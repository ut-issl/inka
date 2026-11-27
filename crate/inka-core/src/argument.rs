//! Loading a project all the way to the shared view, as one operation.
//!
//! Every caller that presents an argument — the commands, the server — needs
//! the same thing: a project that loads, a graph that builds, and a shape that
//! validates. [`load_argument`] does that once and answers with data: either
//! the [`Argument`] or the findings that [`Rejected`] it, with the stage that
//! refused it as a typed [`RejectionCause`]. What to do with the
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
///
/// The findings are for a reader; [`Rejected::cause`] is for a caller that
/// has to act on the kind of failure without parsing a message.
#[derive(Debug)]
pub struct Rejected {
    /// Which stage refused the project.
    pub cause: RejectionCause,
    /// Resolves the spans of the findings; empty when the failure is
    /// structural and nothing was parsed.
    pub source_map: SourceMap,
    /// At least one error, plus every warning the run saw before it stopped.
    pub diagnostics: Diagnostics,
}

/// The stage at which a project was refused.
///
/// The stages run in this order and the first to find an error ends the
/// load, so a cause also says how far the project got.
#[derive(Debug, thiserror::Error)]
pub enum RejectionCause {
    /// The project could not be read at all: a missing or invalid manifest,
    /// values or evidence that do not load, a missing `argument/`, an
    /// unreadable file. The loader's own error is kept whole, so its variant
    /// and its underlying cause stay available. Never
    /// [`LoadError::Invalid`], which is [`RejectionCause::Sources`].
    #[error(transparent)]
    Load(Box<LoadError>),
    /// The files were read, but their content has syntax or interpolation
    /// errors.
    #[error("the argument's sources have errors")]
    Sources,
    /// The blocks parse, but do not form a graph: a duplicate id, or a
    /// reference to a block that does not exist.
    #[error("the argument's blocks do not form a graph")]
    Graph,
    /// The graph builds, but its shape is not an argument's: no objective,
    /// or a dependency cycle.
    #[error("the argument's graph is not a valid argument")]
    Shape,
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
            return Err(rejected(
                RejectionCause::Sources,
                source_map,
                errors,
                warnings,
            ));
        }
        Err(other) => {
            let errors = vec![Error::unanchored(other.to_string())];
            return Err(rejected(
                RejectionCause::Load(Box::new(other)),
                SourceMap::new(),
                errors,
                warnings,
            ));
        }
    };

    let graph = match Graph::build(&project.blocks) {
        Ok(graph) => graph,
        Err(errors) => {
            return Err(rejected(
                RejectionCause::Graph,
                project.source_map,
                errors,
                warnings,
            ));
        }
    };
    let shape = graph.validate();
    let warnings = [warnings, shape.warnings].concat();
    if !shape.errors.is_empty() {
        return Err(rejected(
            RejectionCause::Shape,
            project.source_map,
            shape.errors,
            warnings,
        ));
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

fn rejected(
    cause: RejectionCause,
    source_map: SourceMap,
    errors: Vec<Error>,
    warnings: Vec<Warning>,
) -> Rejected {
    Rejected {
        cause,
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
        assert!(matches!(rejected.cause, RejectionCause::Sources));
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
        assert!(matches!(rejected.cause, RejectionCause::Graph));
        assert!(rejected.diagnostics.has_errors());
    }

    #[test]
    fn an_argument_without_an_objective_is_rejected_for_its_shape() {
        // Arrange
        let dir = project("fiat leaf {\n    ```\n    given\n    ```\n}\n");

        // Act
        let rejected = load_argument(dir.path()).expect_err("nothing to hang from");

        // Assert
        assert!(matches!(rejected.cause, RejectionCause::Shape));
    }

    #[test]
    fn a_missing_manifest_is_one_unanchored_error() {
        // Arrange
        let dir = tempfile::tempdir().expect("a temporary directory");

        // Act
        let rejected = load_argument(dir.path()).expect_err("there is no project");

        // Assert: the loader's own error is kept, not only its message.
        let RejectionCause::Load(error) = &rejected.cause else {
            panic!("a missing manifest is a load failure: {:?}", rejected.cause);
        };
        assert!(matches!(
            &**error,
            LoadError::Read { source, .. } if source.kind() == std::io::ErrorKind::NotFound
        ));
        assert_eq!(
            rejected.diagnostics.errors[0].message,
            rejected.cause.to_string()
        );
        assert_eq!(rejected.diagnostics.errors.len(), 1);
        assert!(rejected.diagnostics.errors[0].span.is_none());
        assert_eq!(rejected.diagnostics.warnings, []);
    }
}
