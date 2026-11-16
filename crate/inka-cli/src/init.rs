//! The `inka init` command: lay out a new project — a manifest, a values file,
//! and a starter argument — that `inka check` and `inka fmt --check` accept as
//! it stands.
//!
//! Initialisation only ever adds files. Every file it would write is checked
//! up front, and each is then created with `create_new`, so an existing
//! project, or a file that appears between the check and the write, is never
//! overwritten.

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use inka_core::manifest::parse_manifest;
use inka_core::source::{Diagnostics, Error, SourceMap};

use crate::check::Outcome;
use crate::render::render;

/// The manifest's file name, read by every other command.
const MANIFEST: &str = "inka.toml";
/// The values file the manifest lists.
const VALUES: &str = "values.toml";
/// The directory the argument is read from by convention.
const ARGUMENT_DIR: &str = "argument";
/// The starter argument file, inside [`ARGUMENT_DIR`].
const STARTER: &str = "objective.inka";

/// What `inka init` was asked to create.
#[derive(Debug, Clone)]
pub struct Options {
    /// `project.name`; defaults to the project directory's name.
    pub name: Option<String>,
    /// `phases.order`; the first phase becomes `phases.current`.
    pub phases: Vec<String>,
}

/// Runs `inka init` in `project_dir`, creating the directory if it is missing.
///
/// Every created path is listed on `out`. The run fails, writing nothing,
/// when the directory already holds a manifest or any other file init would
/// write, when no name is given and none can be derived from the directory,
/// or when the name or a phase is not one a manifest accepts.
///
/// # Errors
///
/// Propagates any I/O error from writing to `out` or `err`. A failure to
/// create a file is reported on `err` instead.
pub fn run(
    project_dir: &Path,
    options: &Options,
    out: &mut impl Write,
    err: &mut impl Write,
    color: bool,
) -> io::Result<Outcome> {
    match scaffold(project_dir, options) {
        Ok(created) => {
            for path in created {
                writeln!(out, "{}", path.display())?;
            }
            Ok(Outcome::Ok)
        }
        Err(message) => {
            let diagnostics = Diagnostics {
                errors: vec![Error::unanchored(message)],
                warnings: Vec::new(),
            };
            render(&SourceMap::new(), &diagnostics, err, color)?;
            Ok(Outcome::Failed)
        }
    }
}

/// Validates the request, then writes the project. Returns the created paths
/// in creation order, or the one message explaining why nothing was written.
fn scaffold(project_dir: &Path, options: &Options) -> Result<Vec<PathBuf>, String> {
    if let Some(phase) = options.phases.iter().find(|phase| !is_identifier(phase)) {
        return Err(format!(
            "phase `{phase}` is not an identifier ([A-Za-z_][A-Za-z0-9_-]*), so `until:` could not name it"
        ));
    }
    let name = match &options.name {
        Some(name) => name.clone(),
        None => derive_name(project_dir)?,
    };
    let manifest = manifest_text(&name, &options.phases);
    // The manifest is generated, so a rejection here is a bad name or phase
    // list, reported in the words `inka check` would use.
    parse_manifest(&manifest).map_err(|error| error.to_string())?;

    let argument_dir = project_dir.join(ARGUMENT_DIR);
    let files = [
        (project_dir.join(MANIFEST), manifest),
        (project_dir.join(VALUES), VALUES_TEXT.to_owned()),
        (argument_dir.join(STARTER), STARTER_TEXT.to_owned()),
    ];
    if let Some((existing, _)) = files.iter().find(|(path, _)| path.exists()) {
        return Err(format!(
            "{} already exists; `inka init` never overwrites a file",
            existing.display()
        ));
    }

    let mut created = Vec::new();
    for dir in [project_dir, argument_dir.as_path()] {
        if !dir.exists() {
            fs::create_dir_all(dir)
                .map_err(|error| format!("failed to create {}: {error}", dir.display()))?;
            created.push(dir.to_path_buf());
        }
    }
    for (path, text) in files {
        create_new(&path, &text)
            .map_err(|error| format!("failed to create {}: {error}", path.display()))?;
        created.push(path);
    }
    Ok(created)
}

/// The project name implied by the directory: its final component, after
/// resolving `.` and `..` against the current directory.
fn derive_name(project_dir: &Path) -> Result<String, String> {
    let absolute = std::path::absolute(project_dir)
        .map_err(|error| format!("failed to resolve {}: {error}", project_dir.display()))?;
    // `absolute` keeps `..` as written; walk the components so `inka init ..`
    // names the parent, not a directory called `..`.
    let mut resolved = PathBuf::new();
    for component in absolute.components() {
        match component {
            std::path::Component::ParentDir => {
                resolved.pop();
            }
            std::path::Component::CurDir => {}
            other => resolved.push(other),
        }
    }
    resolved
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_owned)
        .ok_or_else(|| {
            format!(
                "cannot derive a project name from {}; pass --name",
                project_dir.display()
            )
        })
}

/// Whether `text` matches the identifier grammar `[A-Za-z_][A-Za-z0-9_-]*`.
fn is_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// Creates `path` holding `text`, failing if anything is already there.
fn create_new(path: &Path, text: &str) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()
}

/// The manifest for a project called `name` with phases `phases`.
fn manifest_text(name: &str, phases: &[String]) -> String {
    let quote = |text: &str| toml::Value::String(text.to_owned()).to_string();
    let order: Vec<String> = phases.iter().map(|phase| quote(phase)).collect();
    // clap guarantees at least one phase; an empty list still yields a
    // manifest, which `parse_manifest` then rejects.
    let current = phases
        .first()
        .map_or_else(|| quote(""), |phase| quote(phase));
    format!(
        "\
# inka project definition.
# The argument is read by convention: every *.inka under {ARGUMENT_DIR}/.

[project]
name = {name}

[phases]
# The total order of the project's phases, and the phase it is in now.
order = [{order}]
current = {current}

[values]
# Values files resolving {{$var}} in descriptions.
files = [{values}]
",
        name = quote(name),
        order = order.join(", "),
        values = quote(VALUES),
    )
}

/// The starter values file: valid TOML with nothing defined yet.
const VALUES_TEXT: &str = "\
# Values resolving {$var} in argument descriptions.
# Every key is referenceable as {$key}, for example:
#
# pointing_accuracy = \"0.05 deg (3σ)\"
";

/// The starter argument: an objective resting on one fiat, the smallest
/// argument `inka check` accepts.
const STARTER_TEXT: &str = "\
// The root of the argument. Replace these claims with your own.

objective goal {
    ```
    The system meets the objective this project sets out to argue
    ```
    children: premise
}

fiat premise {
    ```
    A claim taken as given until structure or evidence supports it
    ```
}
";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_follow_the_grammar() {
        assert!(is_identifier("M0"));
        assert!(is_identifier("_draft-2"));
        assert!(!is_identifier(""));
        assert!(!is_identifier("0M"));
        assert!(!is_identifier("phase one"));
    }

    #[test]
    fn the_manifest_quotes_names_that_need_it() {
        // Arrange
        let phases = ["BBM".to_owned(), "EM".to_owned()];

        // Act
        let manifest = parse_manifest(&manifest_text("say \"hi\"", &phases))
            .expect("a generated manifest should parse");

        // Assert
        assert_eq!(manifest.project.name, "say \"hi\"");
        assert_eq!(manifest.phases.order, phases);
        assert_eq!(manifest.phases.current, "BBM");
        assert_eq!(manifest.values.files, [PathBuf::from(VALUES)]);
    }

    #[test]
    fn a_name_is_derived_through_dot_and_dot_dot() {
        let base = std::env::temp_dir().join("inka-init-name").join("child");
        assert_eq!(derive_name(&base), Ok("child".to_owned()));
        assert_eq!(
            derive_name(&base.join("..")),
            Ok("inka-init-name".to_owned())
        );
        assert_eq!(derive_name(&base.join(".")), Ok("child".to_owned()));
    }
}
