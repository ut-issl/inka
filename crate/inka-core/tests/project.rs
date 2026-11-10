//! Loads the checked-in fixture projects end to end, and exercises every
//! loader error path against synthetic projects in a temp directory.

use std::fs;
use std::path::PathBuf;

use inka_core::ast::{Block, SourceRef};
use inka_core::project::{LoadError, LoadOutcome, Project, load_project, load_project_with};
use inka_core::read::{DiskText, ReadText};
use inka_core::values::ValuesError;

mod common;
use common::fixture_dir;

fn rendered_descriptions(project: &Project) -> Vec<&str> {
    project
        .blocks
        .iter()
        .map(|block| block.common().description.rendered())
        .collect()
}

#[test]
fn loads_dogfood_project_and_interpolates_values() {
    // Arrange & Act
    let project = load_project(&fixture_dir("dogfood"))
        .result
        .expect("dogfood project should load");

    // Assert
    assert_eq!(project.manifest.project.name, "inka-dogfood");
    assert!(!project.blocks.is_empty(), "expected parsed blocks");
    let rendered = rendered_descriptions(&project);
    assert!(
        rendered
            .iter()
            .any(|text| text.contains("SSH signature (sshsig / ed25519)")),
        "sig_scheme should be interpolated into a description"
    );
    assert!(
        rendered.iter().all(|text| !text.contains("{$")),
        "no {{$var}} marker should survive interpolation"
    );
}

#[test]
fn loads_aocs_project_and_interpolates_all_eight_values() {
    // Arrange
    let expected_values = [
        "0.05 deg (3σ)",
        "0.01 deg (3σ)",
        "10 deg/s",
        "2 orbits",
        "30 %",
        "30 deg",
        "10 min",
        "500-600 km altitude",
    ];

    // Act
    let project = load_project(&fixture_dir("aocs"))
        .result
        .expect("aocs project should load");

    // Assert
    let rendered = rendered_descriptions(&project);
    for value in expected_values {
        assert!(
            rendered.iter().any(|text| text.contains(value)),
            "value `{value}` should appear in a rendered description"
        );
    }
    assert!(
        rendered.iter().all(|text| !text.contains("{$")),
        "no {{$var}} marker should survive interpolation"
    );
}

#[test]
fn loads_the_same_block_sequence_on_every_run() {
    // Arrange & Act: files are read in sorted path order, so two loads of
    // the same project must produce the same block sequence.
    let first = load_project(&fixture_dir("dogfood"))
        .result
        .expect("dogfood project should load");
    let second = load_project(&fixture_dir("dogfood"))
        .result
        .expect("dogfood project should load");

    // Assert
    let ids = |project: &Project| {
        project
            .blocks
            .iter()
            .map(|block| block.common().id.value.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(&first), ids(&second));
}

#[test]
fn load_reports_warnings_alongside_the_result() {
    // Arrange & Act: no load-stage pass emits warnings today, so the lane is
    // empty — what matters is that it rides outside the Result, present on
    // success and failure alike.
    let LoadOutcome { warnings, result } = load_project(&fixture_dir("dogfood"));

    // Assert
    assert_eq!(warnings, []);
    assert!(result.is_ok());
}

/// A throwaway project directory, removed on drop.
struct TempProject {
    root: PathBuf,
}

const MANIFEST_WITHOUT_VALUES: &str = concat!(
    "[project]\nname = \"t\"\n\n",
    "[phases]\norder = [\"P0\"]\ncurrent = \"P0\"\n\n",
    "[values]\nfiles = []\n",
);

const PLAIN_FIAT_BLOCK: &str = "fiat a {\n  ```\n  plain text\n  ```\n}\n";

impl TempProject {
    fn new(name: &str) -> Self {
        let root =
            std::env::temp_dir().join(format!("inka-project-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("temp project dir should be creatable");
        Self { root }
    }

    fn write(&self, relative: &str, text: &str) {
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().expect("relative path should have a parent"))
            .expect("parent dir should be creatable");
        fs::write(path, text).expect("file should be writable");
    }

    fn subdir(&self, relative: &str) {
        fs::create_dir_all(self.root.join(relative)).expect("subdir should be creatable");
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn loads_bind_block_preserving_source_and_rendering_description() {
    // Arrange
    let project = TempProject::new("bind-block");
    project.write(
        "inka.toml",
        &MANIFEST_WITHOUT_VALUES.replace("files = []", "files = [\"values.toml\"]"),
    );
    project.write("values.toml", "target = \"the hash property\"\n");
    project.write(
        "argument/a.inka",
        "bind b {\n  ```\n  verifies {$target}\n  ```\n  calc: \"M::?f\"\n}\n",
    );

    // Act
    let loaded = load_project(&project.root)
        .result
        .expect("bind project should load");

    // Assert: the Bind arm of `with_description` must keep `source` while
    // swapping in the rendered description.
    assert_eq!(loaded.blocks.len(), 1);
    let Block::Bind(bind) = &loaded.blocks[0] else {
        panic!("expected a bind block, got {:?}", loaded.blocks[0]);
    };
    assert!(
        bind.common
            .description
            .rendered()
            .contains("verifies the hash property"),
        "description should be rendered, got: {}",
        bind.common.description.rendered()
    );
    assert!(matches!(&bind.source.value, SourceRef::Calc(name) if name == "M::?f"));
}

#[test]
fn loads_empty_argument_directory_as_project_without_blocks() {
    // Arrange
    let project = TempProject::new("empty-argument");
    project.write("inka.toml", MANIFEST_WITHOUT_VALUES);
    project.subdir("argument");

    // Act
    let loaded = load_project(&project.root)
        .result
        .expect("empty argument/ should load");

    // Assert
    assert_eq!(loaded.blocks, []);
}

#[test]
fn ignores_files_without_the_inka_extension() {
    // Arrange
    let project = TempProject::new("non-inka-files");
    project.write("inka.toml", MANIFEST_WITHOUT_VALUES);
    project.write("argument/a.inka", PLAIN_FIAT_BLOCK);
    project.write("argument/README.md", "notes, not part of the argument\n");

    // Act
    let loaded = load_project(&project.root)
        .result
        .expect("non-.inka files are ignored");

    // Assert
    assert_eq!(loaded.blocks.len(), 1);
}

#[test]
fn loads_the_evidence_the_manifest_imports() {
    // Arrange
    let project = TempProject::new("evidence");
    project.write(
        "inka.toml",
        &format!("{MANIFEST_WITHOUT_VALUES}\n[evidence]\ntests = [\"evidence.json\"]\n"),
    );
    project.write(
        "evidence.json",
        r#"{"v": 1, "tests": [{"id": "t", "body": "fn t() {}", "executions": [{"case": "default", "outcome": "pass"}]}]}"#,
    );
    project.write("argument/a.inka", PLAIN_FIAT_BLOCK);

    // Act
    let loaded = load_project(&project.root)
        .result
        .expect("evidence project should load");

    // Assert
    assert!(loaded.evidence.get("t").is_some());
}

#[test]
fn reports_an_unloadable_evidence_file_as_an_input_error() {
    // Arrange: the file the manifest names is not there.
    let project = TempProject::new("missing-evidence-file");
    project.write(
        "inka.toml",
        &format!("{MANIFEST_WITHOUT_VALUES}\n[evidence]\ntests = [\"evidence.json\"]\n"),
    );
    project.write("argument/a.inka", PLAIN_FIAT_BLOCK);

    // Act
    let error = load_project(&project.root)
        .result
        .expect_err("a missing evidence file should fail the load");

    // Assert
    assert!(matches!(error, LoadError::Evidence(_)));
    assert!(error.to_string().contains("evidence.json"));
}

/// One calc evidence target assuming `assumed`, as a producer delivers it.
const CALC_EVIDENCE: &str = r#"{"v": 1, "targets": [{"path": "AOCS::?verify_detumble",
    "definitions": [{"path": "AOCS::?verify_detumble", "kind": "verification", "source": "def verify_detumble(): ..."}],
    "assumes": ["assumed"], "leaves": [{"id": "AOCS::?verify_detumble", "result": true}], "valid": true}]}"#;

#[test]
fn loads_the_calc_evidence_the_manifest_imports() {
    // Arrange: calc evidence alone; the tests list may be left out.
    let project = TempProject::new("calc-evidence");
    project.write(
        "inka.toml",
        &format!("{MANIFEST_WITHOUT_VALUES}\n[evidence]\ncalcs = [\"calc-evidence.json\"]\n"),
    );
    project.write("calc-evidence.json", CALC_EVIDENCE);
    project.write("argument/a.inka", PLAIN_FIAT_BLOCK);

    // Act
    let loaded = load_project(&project.root)
        .result
        .expect("calc evidence project should load");

    // Assert
    let target = loaded
        .calc_evidence
        .get("AOCS::?verify_detumble")
        .expect("the target is loaded");
    assert_eq!(target.assumes, ["assumed"]);
    assert!(loaded.evidence.iter().next().is_none());
}

#[test]
fn reports_an_unloadable_calc_evidence_file_as_an_input_error() {
    // Arrange: the file the manifest names is not there.
    let project = TempProject::new("missing-calc-evidence-file");
    project.write(
        "inka.toml",
        &format!("{MANIFEST_WITHOUT_VALUES}\n[evidence]\ncalcs = [\"calc-evidence.json\"]\n"),
    );
    project.write("argument/a.inka", PLAIN_FIAT_BLOCK);

    // Act
    let error = load_project(&project.root)
        .result
        .expect_err("a missing calc evidence file should fail the load");

    // Assert
    assert!(matches!(error, LoadError::CalcEvidence(_)));
    assert!(error.to_string().contains("calc-evidence.json"));
}

#[test]
fn reports_missing_manifest() {
    // Arrange
    let project = TempProject::new("missing-manifest");

    // Act
    let error = load_project(&project.root)
        .result
        .expect_err("missing inka.toml should fail");

    // Assert
    assert!(matches!(error, LoadError::Read { .. }));
    assert!(error.to_string().contains("inka.toml"));
}

#[test]
fn reports_invalid_manifest_with_its_path() {
    // Arrange
    let project = TempProject::new("invalid-manifest");
    project.write("inka.toml", "[project]\nname = \"t\"\n");

    // Act
    let error = load_project(&project.root)
        .result
        .expect_err("incomplete inka.toml should fail");

    // Assert
    assert!(matches!(error, LoadError::Manifest { .. }));
    assert!(error.to_string().contains("inka.toml"));
}

#[test]
fn reports_value_key_collision_across_files() {
    // Arrange
    let project = TempProject::new("value-collision");
    project.write(
        "inka.toml",
        &MANIFEST_WITHOUT_VALUES.replace("files = []", "files = [\"a.toml\", \"b.toml\"]"),
    );
    project.write("a.toml", "x = \"1\"\n");
    project.write("b.toml", "x = \"2\"\n");
    project.write("argument/a.inka", PLAIN_FIAT_BLOCK);

    // Act
    let error = load_project(&project.root)
        .result
        .expect_err("colliding value key should fail");

    // Assert
    assert!(matches!(
        error,
        LoadError::Values(ValuesError::DuplicateKey { .. })
    ));
}

#[test]
fn reports_missing_argument_directory() {
    // Arrange
    let project = TempProject::new("missing-argument");
    project.write("inka.toml", MANIFEST_WITHOUT_VALUES);

    // Act
    let error = load_project(&project.root)
        .result
        .expect_err("missing argument/ should fail");

    // Assert
    assert!(matches!(error, LoadError::MissingArgumentDir { .. }));
    assert!(error.to_string().contains("argument"));
}

#[test]
fn rejects_subdirectories_under_argument() {
    // Arrange
    let project = TempProject::new("argument-subdir");
    project.write("inka.toml", MANIFEST_WITHOUT_VALUES);
    project.write("argument/a.inka", PLAIN_FIAT_BLOCK);
    project.subdir("argument/nested");

    // Act
    let error = load_project(&project.root)
        .result
        .expect_err("argument/ subdirectory should fail");

    // Assert
    assert!(matches!(error, LoadError::UnsupportedSubdirectory { .. }));
    assert!(error.to_string().contains("nested"));
}

#[test]
fn reports_the_first_subdirectory_in_path_order() {
    // Arrange: two offenders — the reported one must not depend on OS
    // directory-iteration order.
    let project = TempProject::new("argument-subdirs-sorted");
    project.write("inka.toml", MANIFEST_WITHOUT_VALUES);
    project.subdir("argument/z-sub");
    project.subdir("argument/a-sub");

    // Act
    let error = load_project(&project.root)
        .result
        .expect_err("argument/ subdirectory should fail");

    // Assert
    assert!(
        error.to_string().contains("a-sub"),
        "expected the first subdirectory in sorted order, got: {error}"
    );
}

#[test]
fn reports_undefined_variable_with_file_position() {
    // Arrange
    let project = TempProject::new("undefined-variable");
    project.write("inka.toml", MANIFEST_WITHOUT_VALUES);
    project.write(
        "argument/a.inka",
        "fiat a {\n  ```\n  needs {$missing}\n  ```\n}\n",
    );

    // Act
    let error = load_project(&project.root)
        .result
        .expect_err("undefined variable should fail");

    // Assert
    let LoadError::Invalid { errors, .. } = &error else {
        panic!("expected Invalid, got {error:?}");
    };
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0].message, "undefined variable `missing`");
    let message = error.to_string();
    assert!(
        message.contains("a.inka:3:"),
        "message should carry path:line:column, got: {message}"
    );
}

#[test]
fn aggregates_undefined_variables_across_files() {
    // Arrange
    let project = TempProject::new("undefined-variables-two-files");
    project.write("inka.toml", MANIFEST_WITHOUT_VALUES);
    project.write(
        "argument/a.inka",
        "fiat a {\n  ```\n  needs {$first}\n  ```\n}\n",
    );
    project.write(
        "argument/b.inka",
        "fiat b {\n  ```\n  needs {$second}\n  ```\n}\n",
    );

    // Act
    let error = load_project(&project.root)
        .result
        .expect_err("undefined variables should fail");

    // Assert: both files are reported in one pass, each with its own position.
    let LoadError::Invalid { errors, .. } = &error else {
        panic!("expected Invalid, got {error:?}");
    };
    assert_eq!(errors.len(), 2);
    let message = error.to_string();
    assert!(message.contains("a.inka:3:"), "got: {message}");
    assert!(message.contains("b.inka:3:"), "got: {message}");
}

#[test]
fn reports_missing_values_file() {
    // Arrange
    let project = TempProject::new("missing-values-file");
    project.write(
        "inka.toml",
        &MANIFEST_WITHOUT_VALUES.replace("files = []", "files = [\"values.toml\"]"),
    );
    project.write("argument/a.inka", PLAIN_FIAT_BLOCK);

    // Act
    let error = load_project(&project.root)
        .result
        .expect_err("missing values file should fail");

    // Assert
    assert!(matches!(error, LoadError::Values(ValuesError::Read { .. })));
    assert!(error.to_string().contains("values.toml"));
}

#[test]
fn reports_parser_diagnostics_before_interpolation() {
    // Arrange: one block missing its description, one referencing an
    // undefined variable. The syntax stage must report without the
    // interpolation stage masking or duplicating it.
    let project = TempProject::new("parse-diagnostics");
    project.write("inka.toml", MANIFEST_WITHOUT_VALUES);
    project.write("argument/a.inka", "fiat a {\n}\n");
    project.write(
        "argument/b.inka",
        "fiat b {\n  ```\n  needs {$missing}\n  ```\n}\n",
    );

    // Act
    let error = load_project(&project.root)
        .result
        .expect_err("broken block should fail");

    // Assert
    let LoadError::Invalid { errors, .. } = &error else {
        panic!("expected Invalid, got {error:?}");
    };
    assert!(
        errors
            .iter()
            .all(|error| !error.message.contains("undefined variable")),
        "interpolation must not run while syntax errors exist: {errors:?}"
    );
}

/// A reader serving in-memory texts for some paths and the disk for the rest,
/// the shape a language server uses for its open buffers.
struct OverlayReader {
    overlays: Vec<(PathBuf, String)>,
}

impl ReadText for OverlayReader {
    fn read_text(&self, path: &std::path::Path) -> std::io::Result<String> {
        match self.overlays.iter().find(|(overlaid, _)| overlaid == path) {
            Some((_, text)) => Ok(text.clone()),
            None => DiskText.read_text(path),
        }
    }
}

#[test]
fn load_project_with_prefers_overlay_text_over_the_disk() {
    // Arrange: the disk holds one fiat; the overlay holds an edited version
    // of the same file that the editor has not saved yet.
    let project = TempProject::new("overlay-wins");
    project.write("inka.toml", MANIFEST_WITHOUT_VALUES);
    project.write("argument/a.inka", PLAIN_FIAT_BLOCK);
    let reader = OverlayReader {
        overlays: vec![(
            project.root.join("argument/a.inka"),
            "fiat a {\n  ```\n  edited text\n  ```\n}\n".to_owned(),
        )],
    };

    // Act
    let loaded = load_project_with(&project.root, &reader)
        .result
        .expect("overlaid project should load");

    // Assert: analysis saw the buffer, not the file.
    assert_eq!(loaded.blocks.len(), 1);
    assert_eq!(
        loaded.blocks[0].common().description.rendered().trim(),
        "edited text"
    );
}

#[test]
fn load_project_with_reads_unoverlaid_files_from_the_disk() {
    // Arrange: an overlay for one argument file only; the manifest and the
    // second argument file must still come from the disk.
    let project = TempProject::new("overlay-fallback");
    project.write("inka.toml", MANIFEST_WITHOUT_VALUES);
    project.write("argument/a.inka", PLAIN_FIAT_BLOCK);
    project.write(
        "argument/b.inka",
        "fiat b {\n  ```\n  from the disk\n  ```\n}\n",
    );
    let reader = OverlayReader {
        overlays: vec![(
            project.root.join("argument/a.inka"),
            "fiat a {\n  ```\n  from the buffer\n  ```\n}\n".to_owned(),
        )],
    };

    // Act
    let loaded = load_project_with(&project.root, &reader)
        .result
        .expect("partially overlaid project should load");

    // Assert
    let rendered = rendered_descriptions(&loaded);
    assert_eq!(rendered.len(), 2);
    assert!(rendered.iter().any(|text| text.contains("from the buffer")));
    assert!(rendered.iter().any(|text| text.contains("from the disk")));
}
