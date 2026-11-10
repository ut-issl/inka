//! `inka.toml` project manifest: rule-side state (current phase) and loading
//! configuration, deserialized from TOML.
//!
//! Unknown keys are rejected (`deny_unknown_fields`) so a typo fails loudly
//! instead of being silently ignored. Which files form the argument is not
//! configuration — `argument/` is read by convention — so the manifest stays
//! small: the phase order (needed for phase comparison), the current phase,
//! and where to find values files.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use serde::Deserialize;

/// The parsed contents of an `inka.toml` file.
///
/// All three sections are required, matching the spec's project-layout
/// definition; a manifest missing one is rejected at deserialization time.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub project: ProjectSection,
    pub phases: PhasesSection,
    pub values: ValuesSection,
    /// Absent when the project imports no evidence.
    #[serde(default)]
    pub evidence: Option<EvidenceSection>,
}

/// The `[project]` section. `name` is only used for display today.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectSection {
    pub name: String,
}

/// The `[phases]` section: the total order of phases and the current one.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PhasesSection {
    pub order: Vec<String>,
    pub current: String,
}

impl PhasesSection {
    /// Maps each phase name to its ordinal in `order`, so later passes compare
    /// phases by index without re-scanning the list. Keys borrow from `self`.
    #[must_use]
    pub fn phase_index(&self) -> HashMap<&str, usize> {
        self.order
            .iter()
            .enumerate()
            .map(|(index, name)| (name.as_str(), index))
            .collect()
    }
}

/// The `[values]` section: TOML files resolving `{$var}` in descriptions.
///
/// Paths are relative to the manifest's directory; joining them onto a base
/// directory is the loader's job, not the parser's.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ValuesSection {
    pub files: Vec<PathBuf>,
}

/// The optional `[evidence]` section: the evidence JSON files to import, test
/// evidence under `tests` and calc evidence under `calcs`.
///
/// Paths are relative to the manifest's directory, like `values.files`, and
/// joining them is the loader's job. The delivery machinery (capsula) is
/// responsible for the files being present and current; the manifest only
/// names where they are expected. Either list may be left out.
#[derive(Debug, Clone, PartialEq, Eq, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceSection {
    /// Test evidence files (`schemas/test-evidence-v1.schema.json`).
    #[serde(default)]
    pub tests: Vec<PathBuf>,
    /// Calc evidence files (`schemas/calc-evidence-v1.schema.json`).
    #[serde(default)]
    pub calcs: Vec<PathBuf>,
}

/// A reason an `inka.toml` file was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ManifestError {
    #[error("invalid inka.toml: {0}")]
    Toml(#[from] toml::de::Error),
    #[error("project.name must not be empty")]
    EmptyProjectName,
    #[error("phases.order must not be empty")]
    EmptyPhaseOrder,
    #[error("phases.order contains a blank phase name")]
    BlankPhaseName,
    #[error("phases.order lists `{name}` more than once")]
    DuplicatePhase { name: String },
    #[error("phases.current `{name}` is not listed in phases.order")]
    CurrentPhaseNotInOrder { name: String },
    #[error(
        "`[evidence] files` is now `[evidence] tests`; rename the key \
         (calc evidence files go under `[evidence] calcs`)"
    )]
    RetiredEvidenceFiles,
}

/// Parses and validates the text of an `inka.toml` file.
///
/// # Errors
///
/// Returns a [`ManifestError`] when the TOML is malformed, contains unknown
/// keys, has an empty or blank `project.name`, has an empty `phases.order` or
/// a blank phase name in it, repeats a phase in `phases.order`, or names a
/// `phases.current` that `phases.order` does not contain. A manifest still
/// using `[evidence] files` is told the key's new name instead of "unknown".
pub fn parse_manifest(text: &str) -> Result<Manifest, ManifestError> {
    let manifest: Manifest = toml::from_str(text).map_err(|error| {
        if names_retired_evidence_files(text) {
            ManifestError::RetiredEvidenceFiles
        } else {
            ManifestError::Toml(error)
        }
    })?;
    validate(&manifest)?;
    Ok(manifest)
}

/// Whether `text` has an `[evidence] files` key: what `[evidence] tests` was
/// called while test evidence was the only kind. Asked only after the strict
/// read has failed, to say what to rename rather than "unknown field".
fn names_retired_evidence_files(text: &str) -> bool {
    toml::from_str::<toml::Table>(text).is_ok_and(|manifest| {
        manifest
            .get("evidence")
            .and_then(toml::Value::as_table)
            .is_some_and(|evidence| evidence.contains_key("files"))
    })
}

fn validate(manifest: &Manifest) -> Result<(), ManifestError> {
    if manifest.project.name.trim().is_empty() {
        return Err(ManifestError::EmptyProjectName);
    }

    let phases = &manifest.phases;
    if phases.order.is_empty() {
        return Err(ManifestError::EmptyPhaseOrder);
    }

    let mut seen = HashSet::new();
    for name in &phases.order {
        if name.trim().is_empty() {
            return Err(ManifestError::BlankPhaseName);
        }
        if !seen.insert(name.as_str()) {
            return Err(ManifestError::DuplicatePhase { name: name.clone() });
        }
    }

    if !phases.order.contains(&phases.current) {
        return Err(ManifestError::CurrentPhaseNotInOrder {
            name: phases.current.clone(),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds a manifest from per-section body lines; `None` omits the section.
    fn manifest_text(project: Option<&str>, phases: Option<&str>, values: Option<&str>) -> String {
        let sections = [
            project.map(|body| format!("[project]\n{body}\n")),
            phases.map(|body| format!("[phases]\n{body}\n")),
            values.map(|body| format!("[values]\n{body}\n")),
        ];
        sections.into_iter().flatten().collect()
    }

    const PROJECT: &str = "name = \"demo\"";
    const PHASES: &str = "order = [\"M0\", \"M1\"]\ncurrent = \"M0\"";
    const VALUES: &str = "files = [\"values.toml\", \"extra.toml\"]";

    #[test]
    fn parses_manifest_with_all_sections() {
        // Arrange
        let text = manifest_text(Some(PROJECT), Some(PHASES), Some(VALUES));

        // Act
        let manifest = parse_manifest(&text).expect("full manifest should parse");

        // Assert
        assert_eq!(manifest.project.name, "demo");
        assert_eq!(manifest.phases.order, ["M0", "M1"]);
        assert_eq!(manifest.phases.current, "M0");
        assert_eq!(
            manifest.values.files,
            [PathBuf::from("values.toml"), PathBuf::from("extra.toml")]
        );
    }

    #[test]
    fn rejects_missing_project_section() {
        // Arrange
        let text = manifest_text(None, Some(PHASES), Some(VALUES));

        // Act
        let error = parse_manifest(&text).expect_err("missing [project] should be rejected");

        // Assert
        assert!(matches!(error, ManifestError::Toml(_)));
        assert!(error.to_string().contains("project"));
    }

    #[test]
    fn rejects_missing_phases_section() {
        // Arrange
        let text = manifest_text(Some(PROJECT), None, Some(VALUES));

        // Act
        let error = parse_manifest(&text).expect_err("missing [phases] should be rejected");

        // Assert
        assert!(matches!(error, ManifestError::Toml(_)));
        assert!(error.to_string().contains("phases"));
    }

    #[test]
    fn rejects_missing_values_section() {
        // Arrange
        let text = manifest_text(Some(PROJECT), Some(PHASES), None);

        // Act
        let error = parse_manifest(&text).expect_err("missing [values] should be rejected");

        // Assert
        assert!(matches!(error, ManifestError::Toml(_)));
        assert!(error.to_string().contains("values"));
    }

    #[test]
    fn rejects_unknown_top_level_key() {
        // Arrange
        let text = format!(
            "{}\n[extras]\nlocation = \"x\"\n",
            manifest_text(Some(PROJECT), Some(PHASES), Some(VALUES))
        );

        // Act
        let error = parse_manifest(&text).expect_err("unknown section should be rejected");

        // Assert
        assert!(matches!(error, ManifestError::Toml(_)));
        assert!(error.to_string().contains("extras"));
    }

    #[test]
    fn rejects_unknown_key_inside_project_section() {
        // Arrange
        let text = manifest_text(
            Some("name = \"demo\"\nowner = \"x\""),
            Some(PHASES),
            Some(VALUES),
        );

        // Act
        let error = parse_manifest(&text).expect_err("typoed key should be rejected");

        // Assert
        assert!(matches!(error, ManifestError::Toml(_)));
        assert!(error.to_string().contains("owner"));
    }

    #[test]
    fn rejects_unknown_key_inside_phases_section() {
        // Arrange
        let text = manifest_text(
            Some(PROJECT),
            Some("order = [\"M0\"]\ncurrent = \"M0\"\nnext = \"M1\""),
            Some(VALUES),
        );

        // Act
        let error = parse_manifest(&text).expect_err("typoed key should be rejected");

        // Assert
        assert!(matches!(error, ManifestError::Toml(_)));
        assert!(error.to_string().contains("next"));
    }

    #[test]
    fn rejects_unknown_key_inside_values_section() {
        // Arrange
        let text = manifest_text(
            Some(PROJECT),
            Some(PHASES),
            Some("files = [\"values.toml\"]\nformat = \"json\""),
        );

        // Act
        let error = parse_manifest(&text).expect_err("typoed key should be rejected");

        // Assert
        assert!(matches!(error, ManifestError::Toml(_)));
        assert!(error.to_string().contains("format"));
    }

    #[test]
    fn rejects_blank_project_name() {
        // Arrange
        let text = manifest_text(Some("name = \"  \""), Some(PHASES), Some(VALUES));

        // Act
        let error = parse_manifest(&text).expect_err("blank project.name should be rejected");

        // Assert
        assert_eq!(error, ManifestError::EmptyProjectName);
    }

    #[test]
    fn rejects_empty_phase_order() {
        // Arrange
        let text = manifest_text(
            Some(PROJECT),
            Some("order = []\ncurrent = \"M0\""),
            Some(VALUES),
        );

        // Act
        let error = parse_manifest(&text).expect_err("empty phases.order should be rejected");

        // Assert
        assert_eq!(error, ManifestError::EmptyPhaseOrder);
    }

    #[test]
    fn rejects_blank_phase_name_in_order() {
        // Arrange
        let text = manifest_text(
            Some(PROJECT),
            Some("order = [\"M0\", \" \"]\ncurrent = \"M0\""),
            Some(VALUES),
        );

        // Act
        let error = parse_manifest(&text).expect_err("blank phase name should be rejected");

        // Assert
        assert_eq!(error, ManifestError::BlankPhaseName);
    }

    #[test]
    fn rejects_current_phase_that_is_not_in_order() {
        // Arrange
        let text = manifest_text(
            Some(PROJECT),
            Some("order = [\"M0\", \"M1\"]\ncurrent = \"M2\""),
            Some(VALUES),
        );

        // Act
        let error = parse_manifest(&text).expect_err("current outside order should be rejected");

        // Assert
        assert_eq!(
            error,
            ManifestError::CurrentPhaseNotInOrder {
                name: "M2".to_string()
            }
        );
    }

    #[test]
    fn rejects_duplicate_phase_in_order() {
        // Arrange
        let text = manifest_text(
            Some(PROJECT),
            Some("order = [\"M0\", \"M1\", \"M0\"]\ncurrent = \"M1\""),
            Some(VALUES),
        );

        // Act
        let error = parse_manifest(&text).expect_err("duplicate phase should be rejected");

        // Assert
        assert_eq!(
            error,
            ManifestError::DuplicatePhase {
                name: "M0".to_string()
            }
        );
    }

    #[test]
    fn phase_index_maps_names_to_ordinals() {
        // Arrange
        let text = manifest_text(Some(PROJECT), Some(PHASES), Some(VALUES));
        let manifest = parse_manifest(&text).expect("manifest should parse");

        // Act
        let index = manifest.phases.phase_index();

        // Assert
        assert_eq!(index.get("M0"), Some(&0));
        assert_eq!(index.get("M1"), Some(&1));
        assert_eq!(index.get("M2"), None);
    }
}
