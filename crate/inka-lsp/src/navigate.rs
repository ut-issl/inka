//! Resolving the identifier under a cursor: definition, references, hover
//! text, and rename validation.
//!
//! Everything here walks the loaded blocks directly — at project scale a
//! linear scan per request is well inside the full-recomputation budget, and
//! it keeps this module free of any index that could go stale.

use std::fmt::Write as _;

use inka_core::ast::{Block, Deadline, FiatPromoteTarget, ObjectivePromoteTarget, SourceRef};
use inka_core::interpolate::RenderedDescription;
use inka_core::normalize;
use inka_core::project::Project;
use inka_core::source::{FileId, Span, Spanned};

/// What an offset in a file points at.
#[derive(Debug, Clone, Copy)]
pub enum Symbol<'p> {
    /// The id in a block header.
    Definition(&'p Block<RenderedDescription>),
    /// One name in a `children:` list.
    Reference(&'p Spanned<String>),
}

impl Symbol<'_> {
    /// The identifier's text.
    pub fn name(&self) -> &str {
        match self {
            Symbol::Definition(block) => &block.common().id.value,
            Symbol::Reference(reference) => &reference.value,
        }
    }

    /// The identifier's own span.
    pub fn span(&self) -> Span {
        match self {
            Symbol::Definition(block) => block.common().id.span,
            Symbol::Reference(reference) => reference.span,
        }
    }
}

/// The identifier at `offset` in `file`, if any.
///
/// The end position is inclusive, so a cursor sitting right after the last
/// character still hits the identifier — which is where an editor leaves the
/// cursor after a double-click.
pub fn symbol_at(project: &Project, file: FileId, offset: usize) -> Option<Symbol<'_>> {
    for block in &project.blocks {
        if covers(block.common().id.span, file, offset) {
            return Some(Symbol::Definition(block));
        }
        for child in block.children().iter().map(|entry| &entry.id) {
            if covers(child.span, file, offset) {
                return Some(Symbol::Reference(child));
            }
        }
    }
    None
}

fn covers(span: Span, file: FileId, offset: usize) -> bool {
    span.file_id == file && offset >= span.start && offset <= span.end
}

/// The block defining `name`, if any. Duplicate ids are a check error, so
/// taking the first match is as deterministic as the input allows; anything
/// that *acts* on the definition must use [`definitions`] and handle the
/// ambiguity instead.
pub fn definition<'p>(project: &'p Project, name: &str) -> Option<&'p Block<RenderedDescription>> {
    definitions(project, name).into_iter().next()
}

/// Every block defining `name`. More than one means the project is broken
/// (`inka check` reports the duplicate), but navigation still runs on broken
/// projects, so callers see the ambiguity instead of a silent first pick.
pub fn definitions<'p>(project: &'p Project, name: &str) -> Vec<&'p Block<RenderedDescription>> {
    project
        .blocks
        .iter()
        .filter(|block| block.common().id.value == name)
        .collect()
}

/// The spans of every `children:` entry naming `name`, across all files.
pub fn references(project: &Project, name: &str) -> Vec<Span> {
    project
        .blocks
        .iter()
        .flat_map(Block::children)
        .filter(|child| child.id.value == name)
        .map(|child| child.id.span)
        .collect()
}

/// The DSL keyword of a block's mode, as written in source and in approval
/// record targets.
pub fn mode_keyword(block: &Block<RenderedDescription>) -> &'static str {
    match block {
        Block::Objective(_) => "objective",
        Block::Structure(_) => "structure",
        Block::Bind(_) => "bind",
        Block::Fiat(_) => "fiat",
    }
}

/// Markdown hover text for one block: mode and id as the header, the
/// contract text exactly as hashing sees it, and the annotations that are
/// part of the contract (deadline, promotion, evidence reference). Taking
/// the block — not a name — keeps hover exact when a duplicated id makes
/// name lookup ambiguous.
pub fn hover(block: &Block<RenderedDescription>) -> String {
    let common = block.common();
    let mut text = format!(
        "**{}** `{}`\n\n{}",
        mode_keyword(block),
        common.id.value,
        normalize::description(common.description.rendered()),
    );

    let mut annotations: Vec<String> = Vec::new();
    if let Some(deadline) = deadline_of(block) {
        annotations.push(format!("until: {}", deadline.until.value));
    }
    if let Some(promote) = promote_of(block) {
        annotations.push(format!("promote: {promote}"));
    }
    if let Block::Bind(bind) = block {
        annotations.push(match &bind.source.value {
            SourceRef::Test(id) => format!("test: \"{id}\""),
            SourceRef::Veriq(target) => format!("veriq: \"{target}\""),
        });
    }

    if !annotations.is_empty() {
        text.push_str("\n\n---\n");
        for annotation in annotations {
            write!(text, "\n{annotation}  ").expect("writing to a String");
        }
    }
    text
}

fn deadline_of(block: &Block<RenderedDescription>) -> Option<&Deadline> {
    match block {
        Block::Objective(objective) => objective.deadline.as_ref().map(|d| &d.deadline),
        Block::Structure(structure) => structure.deadline.as_ref(),
        Block::Bind(bind) => bind.deadline.as_ref(),
        Block::Fiat(fiat) => fiat.deadline.as_ref().map(|d| &d.deadline),
    }
}

fn promote_of(block: &Block<RenderedDescription>) -> Option<&'static str> {
    match block {
        Block::Objective(objective) => objective
            .deadline
            .as_ref()
            .and_then(|d| d.promote.as_ref())
            .map(|promote| match promote.target.value {
                ObjectivePromoteTarget::Structure => "structure",
            }),
        Block::Fiat(fiat) => {
            fiat.deadline
                .as_ref()
                .and_then(|d| d.promote.as_ref())
                .map(|promote| match promote.target.value {
                    FiatPromoteTarget::Structure => "structure",
                    FiatPromoteTarget::Bind => "bind",
                })
        }
        Block::Structure(_) | Block::Bind(_) => None,
    }
}

/// Checks that renaming `old` to `new` is possible in this project.
///
/// # Errors
///
/// Returns the user-facing reason the rename cannot proceed: `old` has no
/// definition (a dangling reference is fixed by editing it, not renaming),
/// `new` is not a valid identifier, or `new` is already taken.
pub fn validate_rename(project: &Project, old: &str, new: &str) -> Result<(), String> {
    if old.contains("::") {
        return Err(format!(
            "`{old}` is a namespaced reference; it is renamed at its definition, outside this project"
        ));
    }
    match definitions(project, old).len() {
        0 => {
            return Err(format!(
                "`{old}` has no definition; fix the reference instead of renaming"
            ));
        }
        1 => {}
        // With two definitions the references are ambiguous, so any edit
        // would move approvals and references between two different nodes.
        _ => {
            return Err(format!(
                "`{old}` is defined more than once; fix the duplicate (`inka check`) before renaming"
            ));
        }
    }
    if !is_valid_id(new) {
        return Err(format!(
            "`{new}` is not a valid id: ids match [A-Za-z_][A-Za-z0-9_-]*"
        ));
    }
    if new != old && definition(project, new).is_some() {
        return Err(format!("`{new}` is already defined"));
    }
    Ok(())
}

/// Whether `name` is a block id the lexer would read back as one token.
///
/// This mirrors the lexer's identifier rules (`is_ident_start` /
/// `is_ident_continue`) plus the keyword exclusion; the record-side validator
/// in `inka-core` is looser (it admits `::` for namespaced targets), so it
/// cannot be reused for a definition id.
fn is_valid_id(name: &str) -> bool {
    let mut bytes = name.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == b'_')
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        && !matches!(name, "objective" | "structure" | "bind" | "fiat")
}

/// Every span a rename of `old` must rewrite in the argument files: the
/// definition id and all references, in no particular order.
pub fn rename_spans(project: &Project, old: &str) -> Vec<Span> {
    let mut spans = references(project, old);
    if let Some(block) = definition(project, old) {
        spans.push(block.common().id.span);
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use inka_core::project::load_project;

    const MANIFEST: &str = concat!(
        "[project]\nname = \"t\"\n\n",
        "[phases]\norder = [\"P0\", \"P1\"]\ncurrent = \"P0\"\n\n",
        "[values]\nfiles = []\n",
    );

    fn scenario() -> (tempfile::TempDir, Project) {
        let temp = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir_all(temp.path().join("argument")).expect("argument dir");
        std::fs::write(temp.path().join("inka.toml"), MANIFEST).expect("manifest");
        std::fs::write(
            temp.path().join("argument/a.inka"),
            "objective root {\n  ```\n  the goal\n  ```\n  children: leaf, helper\n}\n",
        )
        .expect("a.inka");
        std::fs::write(
            temp.path().join("argument/b.inka"),
            concat!(
                "fiat leaf {\n  ```\n  accepted as is\n  ```\n  until: P1\n",
                "  promote: bind\n}\n\n",
                "structure helper {\n  ```\n  helps\n  ```\n  children: leaf\n}\n",
            ),
        )
        .expect("b.inka");
        let project = load_project(temp.path())
            .result
            .expect("scenario project loads");
        (temp, project)
    }

    fn file_of(project: &Project, name: &str) -> FileId {
        project
            .source_map
            .files()
            .find(|(_, file)| file.path.ends_with(name))
            .map(|(id, _)| id)
            .expect("file is loaded")
    }

    #[test]
    fn symbol_at_finds_definitions_and_references_and_nothing_in_prose() {
        // Arrange
        let (_temp, project) = scenario();
        let a = file_of(&project, "a.inka");
        let root_offset = "objective ".len() + 1;

        // Act & Assert: inside the header id.
        let definition = symbol_at(&project, a, root_offset).expect("a symbol");
        assert!(matches!(definition, Symbol::Definition(_)));
        assert_eq!(definition.name(), "root");

        // Inside a children entry.
        let leaf_offset = "objective root {\n  ```\n  the goal\n  ```\n  children: ".len();
        let reference = symbol_at(&project, a, leaf_offset).expect("a symbol");
        assert!(matches!(reference, Symbol::Reference(_)));
        assert_eq!(reference.name(), "leaf");

        // Offset 0 is the `objective` keyword: no identifier.
        assert!(symbol_at(&project, a, 0).is_none());
    }

    #[test]
    fn references_span_files_and_definition_resolves_by_name() {
        // Arrange
        let (_temp, project) = scenario();

        // Act
        let leaf_references = references(&project, "leaf");
        let defined = definition(&project, "leaf").expect("leaf is defined");

        // Assert: referenced from a.inka (root) and b.inka (helper).
        assert_eq!(leaf_references.len(), 2);
        let files: std::collections::HashSet<_> =
            leaf_references.iter().map(|span| span.file_id).collect();
        assert_eq!(files.len(), 2, "references come from two files");
        assert_eq!(mode_keyword(defined), "fiat");
    }

    #[test]
    fn hover_carries_the_contract_and_its_annotations() {
        // Arrange
        let (_temp, project) = scenario();

        // Act
        let text = hover(definition(&project, "leaf").expect("leaf is defined"));

        // Assert
        assert!(text.starts_with("**fiat** `leaf`"), "got: {text}");
        assert!(text.contains("accepted as is"));
        assert!(text.contains("until: P1"));
        assert!(text.contains("promote: bind"));
    }

    #[test]
    fn rename_validation_guards_identity_grammar_and_collisions() {
        // Arrange
        let (_temp, project) = scenario();

        // Act & Assert
        assert!(validate_rename(&project, "leaf", "sheet").is_ok());
        assert!(validate_rename(&project, "leaf", "leaf").is_ok());
        assert!(
            validate_rename(&project, "leaf", "root")
                .expect_err("collision")
                .contains("already defined")
        );
        assert!(
            validate_rename(&project, "leaf", "1bad")
                .expect_err("bad start")
                .contains("not a valid id")
        );
        assert!(
            validate_rename(&project, "leaf", "fiat")
                .expect_err("keyword")
                .contains("not a valid id")
        );
        assert!(
            validate_rename(&project, "ghost", "sheet")
                .expect_err("dangling")
                .contains("no definition")
        );
        assert!(
            validate_rename(&project, "ns::leaf", "sheet")
                .expect_err("namespaced")
                .contains("namespaced reference")
        );
    }

    #[test]
    fn a_duplicated_id_blocks_rename_and_lists_both_definitions() {
        // Arrange: `leaf` defined twice — a broken project that still loads.
        let temp = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir_all(temp.path().join("argument")).expect("argument dir");
        std::fs::write(temp.path().join("inka.toml"), MANIFEST).expect("manifest");
        std::fs::write(
            temp.path().join("argument/a.inka"),
            "fiat leaf {\n  ```\n  first\n  ```\n}\n",
        )
        .expect("a.inka");
        std::fs::write(
            temp.path().join("argument/b.inka"),
            "fiat leaf {\n  ```\n  second\n  ```\n}\n",
        )
        .expect("b.inka");
        let project = load_project(temp.path())
            .result
            .expect("duplicates still load");

        // Act & Assert
        assert_eq!(definitions(&project, "leaf").len(), 2);
        assert!(
            validate_rename(&project, "leaf", "sheet")
                .expect_err("ambiguous")
                .contains("defined more than once")
        );
    }

    #[test]
    fn rename_spans_cover_the_definition_and_every_reference() {
        // Arrange
        let (_temp, project) = scenario();

        // Act
        let spans = rename_spans(&project, "leaf");

        // Assert: two references plus the definition.
        assert_eq!(spans.len(), 3);
    }
}
