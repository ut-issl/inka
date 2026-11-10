//! One project's current findings, as LSP diagnostics grouped by file.
//!
//! This is `inka check` re-targeted at an editor: a full load through the
//! overlay reader, then the graph checks, with every finding mapped onto the
//! file and range its span names. Findings that have no span — a missing
//! objective, a manifest that does not parse, a broken values or evidence
//! file — are anchored to the file that carries the problem, falling back to
//! `inka.toml`: the manifest declares the project, so project-level problems
//! read as manifest problems.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use inka_core::calc_evidence::CalcEvidenceError;
use inka_core::evidence::EvidenceError;
use inka_core::graph;
use inka_core::project::{LoadError, load_project_with};
use inka_core::read::ReadText;
use inka_core::source::{DiagnosticRef, SourceMap, Span};
use inka_core::values::ValuesError;
use lsp_types::{Diagnostic, DiagnosticSeverity, Range};

use crate::position;

/// The diagnostic source label clients display next to every finding.
const SOURCE: &str = "inka";

/// Loads the project at `root` through `reader` and returns every current
/// finding, keyed by the file it anchors to. Files without findings are
/// absent; the publisher decides what to clear.
pub fn analyze(root: &Path, reader: &dyn ReadText) -> HashMap<PathBuf, Vec<Diagnostic>> {
    let mut found: HashMap<PathBuf, Vec<Diagnostic>> = HashMap::new();
    let manifest = root.join("inka.toml");
    // Load-stage warnings are not mapped: no pass emits one yet, and only
    // the success and `Invalid` paths would carry the map to resolve a span.
    match load_project_with(root, reader).result {
        Ok(project) => {
            for finding in graph::check(&project).in_source_order() {
                let severity = match finding {
                    DiagnosticRef::Error(_) => DiagnosticSeverity::ERROR,
                    DiagnosticRef::Warning(_) => DiagnosticSeverity::WARNING,
                };
                push(
                    &mut found,
                    &project.source_map,
                    &manifest,
                    finding.span(),
                    severity,
                    finding.message(),
                );
            }
        }
        Err(LoadError::Invalid { source_map, errors }) => {
            for error in errors {
                push(
                    &mut found,
                    &source_map,
                    &manifest,
                    error.span,
                    DiagnosticSeverity::ERROR,
                    &error.message,
                );
            }
        }
        Err(error) => {
            let path = anchor_of(&error).unwrap_or(manifest);
            found.entry(path).or_default().push(diagnostic(
                Range::default(),
                DiagnosticSeverity::ERROR,
                &error.to_string(),
            ));
        }
    }
    found
}

/// Appends one finding under the file its span resolves to, or under the
/// manifest when it has no span.
fn push(
    found: &mut HashMap<PathBuf, Vec<Diagnostic>>,
    source_map: &SourceMap,
    manifest: &Path,
    span: Option<Span>,
    severity: DiagnosticSeverity,
    message: &str,
) {
    let located = span.and_then(|span| source_map.file(span.file_id).map(|file| (file, span)));
    let (path, range) = match located {
        Some((file, span)) => (file.path.clone(), position::range(file, span)),
        None => (manifest.to_path_buf(), Range::default()),
    };
    found
        .entry(path)
        .or_default()
        .push(diagnostic(range, severity, message));
}

fn diagnostic(range: Range, severity: DiagnosticSeverity, message: &str) -> Diagnostic {
    Diagnostic {
        range,
        severity: Some(severity),
        source: Some(SOURCE.to_owned()),
        message: message.to_owned(),
        ..Diagnostic::default()
    }
}

/// The file a span-less load failure is about, when the error names one.
fn anchor_of(error: &LoadError) -> Option<PathBuf> {
    match error {
        LoadError::Read { path, .. } | LoadError::Manifest { path, .. } => Some(path.clone()),
        LoadError::Values(values) => Some(match values {
            ValuesError::Read { path, .. }
            | ValuesError::Toml { path, .. }
            | ValuesError::InvalidKey { path, .. } => path.clone(),
            // The second definition is the surprising one; the first stands.
            ValuesError::DuplicateKey { second, .. } => second.clone(),
        }),
        LoadError::Evidence(evidence) => Some(match evidence {
            EvidenceError::Unreadable { path, .. }
            | EvidenceError::Malformed { path, .. }
            | EvidenceError::UnsupportedVersion { path, .. }
            | EvidenceError::Invalid { path, .. } => path.clone(),
        }),
        LoadError::CalcEvidence(evidence) => Some(match evidence {
            CalcEvidenceError::Unreadable { path, .. }
            | CalcEvidenceError::Malformed { path, .. }
            | CalcEvidenceError::UnsupportedVersion { path, .. }
            | CalcEvidenceError::Invalid { path, .. } => path.clone(),
        }),
        // Directory-shape problems have no file an editor could open; the
        // manifest that implies the layout is the anchor.
        LoadError::MissingArgumentDir { .. }
        | LoadError::UnsupportedSubdirectory { .. }
        | LoadError::Invalid { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use inka_core::read::DiskText;

    const MANIFEST: &str = concat!(
        "[project]\nname = \"t\"\n\n",
        "[phases]\norder = [\"P0\"]\ncurrent = \"P0\"\n\n",
        "[values]\nfiles = []\n",
    );

    fn project_with(files: &[(&str, &str)]) -> tempfile::TempDir {
        let temp = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir_all(temp.path().join("argument")).expect("argument dir");
        std::fs::write(temp.path().join("inka.toml"), MANIFEST).expect("manifest");
        for (relative, text) in files {
            std::fs::write(temp.path().join(relative), text).expect("argument file");
        }
        temp
    }

    #[test]
    fn a_clean_project_yields_no_diagnostics() {
        // Arrange
        let temp = project_with(&[(
            "argument/a.inka",
            "objective root {\n  ```\n  ok\n  ```\n  children: leaf\n}\n\nfiat leaf {\n  ```\n  ok\n  ```\n}\n",
        )]);

        // Act
        let found = analyze(temp.path(), &DiskText);

        // Assert
        assert!(found.is_empty(), "expected no findings, got {found:?}");
    }

    #[test]
    fn an_undefined_reference_is_an_error_on_its_own_range() {
        // Arrange: `children: ghost` points at nothing.
        let text = "objective root {\n  ```\n  ok\n  ```\n  children: ghost\n}\n";
        let temp = project_with(&[("argument/a.inka", text)]);

        // Act
        let found = analyze(temp.path(), &DiskText);

        // Assert: anchored to `ghost` on line 4 (zero-based), with severity
        // and source filled in.
        let path = temp.path().join("argument/a.inka");
        let diagnostics = found.get(&path).expect("findings on a.inka");
        let ghost = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.message.contains("undefined reference `ghost`"))
            .expect("undefined-reference finding");
        assert_eq!(ghost.severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(ghost.source.as_deref(), Some(SOURCE));
        assert_eq!(ghost.range.start.line, 4);
        assert_eq!(ghost.range.start.character, 12);
        assert_eq!(ghost.range.end.character, 17);
    }

    #[test]
    fn an_unreachable_block_is_a_warning() {
        // Arrange
        let temp = project_with(&[(
            "argument/a.inka",
            "objective root {\n  ```\n  ok\n  ```\n  children: leaf\n}\n\nfiat leaf {\n  ```\n  ok\n  ```\n}\n\nfiat orphan {\n  ```\n  floats\n  ```\n}\n",
        )]);

        // Act
        let found = analyze(temp.path(), &DiskText);

        // Assert
        let path = temp.path().join("argument/a.inka");
        let diagnostics = found.get(&path).expect("findings on a.inka");
        assert!(
            diagnostics.iter().any(|diagnostic| {
                diagnostic.severity == Some(DiagnosticSeverity::WARNING)
                    && diagnostic.message.contains("unreachable")
            }),
            "expected an unreachable warning, got {diagnostics:?}"
        );
    }

    #[test]
    fn a_spanless_project_problem_anchors_to_the_manifest() {
        // Arrange: a parseable project with no objective block at all.
        let temp = project_with(&[("argument/a.inka", "fiat leaf {\n  ```\n  ok\n  ```\n}\n")]);

        // Act
        let found = analyze(temp.path(), &DiskText);

        // Assert
        let manifest = temp.path().join("inka.toml");
        let diagnostics = found.get(&manifest).expect("findings on inka.toml");
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("no objective block found")),
            "expected the missing-objective finding, got {diagnostics:?}"
        );
        assert_eq!(diagnostics[0].range, Range::default());
    }

    /// A manifest importing one calc evidence file.
    const MANIFEST_WITH_CALCS: &str = concat!(
        "[project]\nname = \"t\"\n\n",
        "[phases]\norder = [\"P0\"]\ncurrent = \"P0\"\n\n",
        "[values]\nfiles = []\n\n",
        "[evidence]\ncalcs = [\"calc-evidence.json\"]\n",
    );

    /// One target backing `modelled`, whose upstream assumes `assumes`.
    fn calc_evidence(assumes: &str) -> String {
        format!(
            r#"{{"v": 1, "targets": [{{"path": "P::?v",
                "definitions": [{{"path": "P::?v", "kind": "verification", "source": "x"}}],
                "assumes": [{assumes}], "leaves": [{{"id": "P::?v"}}], "valid": true}}]}}"#
        )
    }

    const BIND_UNDER_ROOT: &str = "objective root {\n  ```\n  ok\n  ```\n  children: modelled\n}\n\nbind modelled {\n  ```\n  ok\n  ```\n  calc: \"P::?v\"\n}\n";

    #[test]
    fn an_assumption_cycle_is_an_error_on_the_calc_reference() {
        // Arrange: the bind's evidence assumes the objective above it.
        let temp = project_with(&[
            ("argument/a.inka", BIND_UNDER_ROOT),
            ("calc-evidence.json", &calc_evidence("\"root\"")),
        ]);
        std::fs::write(temp.path().join("inka.toml"), MANIFEST_WITH_CALCS).expect("manifest");

        // Act
        let found = analyze(temp.path(), &DiskText);

        // Assert: anchored to `calc: "P::?v"` on line 11 (zero-based).
        let path = temp.path().join("argument/a.inka");
        let diagnostics = found.get(&path).expect("findings on a.inka");
        let cycle = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.message.contains("cycle through an assumption"))
            .expect("the cycle finding");
        assert_eq!(cycle.severity, Some(DiagnosticSeverity::ERROR));
        assert_eq!(cycle.range.start.line, 11);
    }

    #[test]
    fn a_broken_calc_evidence_file_anchors_its_error_to_that_file() {
        // Arrange: the configured file declares a version this build does not read.
        let temp = project_with(&[
            ("argument/a.inka", BIND_UNDER_ROOT),
            ("calc-evidence.json", r#"{"v": 2, "targets": []}"#),
        ]);
        std::fs::write(temp.path().join("inka.toml"), MANIFEST_WITH_CALCS).expect("manifest");

        // Act
        let found = analyze(temp.path(), &DiskText);

        // Assert
        let evidence = temp.path().join("calc-evidence.json");
        let diagnostics = found
            .get(&evidence)
            .expect("findings on calc-evidence.json");
        assert_eq!(diagnostics.len(), 1);
        assert!(
            diagnostics[0].message.contains("version 2"),
            "got {diagnostics:?}"
        );
    }

    #[test]
    fn a_broken_manifest_anchors_its_error_to_the_manifest_file() {
        // Arrange
        let temp = project_with(&[("argument/a.inka", "fiat leaf {\n  ```\n  ok\n  ```\n}\n")]);
        std::fs::write(temp.path().join("inka.toml"), "not toml [").expect("break manifest");

        // Act
        let found = analyze(temp.path(), &DiskText);

        // Assert
        let manifest = temp.path().join("inka.toml");
        let diagnostics = found.get(&manifest).expect("findings on inka.toml");
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].severity, Some(DiagnosticSeverity::ERROR));
    }
}
