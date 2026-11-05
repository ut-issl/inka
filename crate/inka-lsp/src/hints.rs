//! Inlay hints: each `children:` entry carries the first line of the child's
//! contract, so an argument reads in place instead of one jump per name. When
//! the label does not show the whole contract, a Markdown tooltip does.
//!
//! VS Code caps the *combined* label length of every hint on one line
//! (`editor.inlayHints.maximumLength`, 43 by default) and drops the hints past
//! the cap, so a `children:` line naming several blocks would show only the
//! first. The bundled extension lifts that cap for inka files; other clients
//! get the same per-hint labels and their own line policy.

use lsp_types::{InlayHint, InlayHintLabel, InlayHintTooltip, MarkupContent, MarkupKind};

use inka_core::normalize;
use inka_core::project::Project;
use inka_core::source::FileId;

use crate::{navigate, position};

/// Longest hint label, in characters; a longer first line ends in `…`.
const LABEL_BUDGET: usize = 48;

/// The hints for `file` whose anchors lie inside the `[start, end]` byte
/// range — the visible range the client asked about.
///
/// A reference without exactly one definition gets no hint: a dangling name
/// has nothing to say, and a duplicated one would attribute one block's words
/// to the other.
pub fn hints(project: &Project, file: FileId, start: usize, end: usize) -> Vec<InlayHint> {
    let Some(source) = project.source_map.file(file) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for block in &project.blocks {
        for child in block.children().iter().map(|entry| &entry.id) {
            if child.span.file_id != file || child.span.end < start || child.span.start > end {
                continue;
            }
            let definitions = navigate::definitions(project, &child.value);
            let [definition] = definitions.as_slice() else {
                continue;
            };
            let description = normalize::description(definition.common().description.rendered());
            let Some(label) = description
                .lines()
                .next()
                .filter(|line| !line.is_empty())
                .map(truncate)
            else {
                continue;
            };
            let tooltip = tooltip(&label, description);
            found.push(InlayHint {
                position: position::position(source, child.span.end),
                label: InlayHintLabel::String(label),
                kind: None,
                text_edits: None,
                tooltip,
                padding_left: Some(true),
                padding_right: None,
                data: None,
            });
        }
    }
    found
}

/// The whole contract as Markdown, or nothing when the label already shows
/// all of it — a one-line contract within the budget.
fn tooltip(label: &str, description: String) -> Option<InlayHintTooltip> {
    (label != description).then_some(InlayHintTooltip::MarkupContent(MarkupContent {
        kind: MarkupKind::Markdown,
        value: description,
    }))
}

/// The first `LABEL_BUDGET` characters, with `…` marking a cut.
fn truncate(line: &str) -> String {
    let mut characters = line.chars();
    let short: String = characters.by_ref().take(LABEL_BUDGET).collect();
    if characters.next().is_some() {
        format!("{short}…")
    } else {
        short
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_cuts_at_the_budget_on_a_character_boundary() {
        // Arrange: multibyte characters around the boundary.
        let long = "あ".repeat(LABEL_BUDGET + 1);
        let exact = "x".repeat(LABEL_BUDGET);

        // Act & Assert
        assert_eq!(truncate(&long).chars().count(), LABEL_BUDGET + 1);
        assert!(truncate(&long).ends_with('…'));
        assert_eq!(truncate(&exact), exact);
        assert_eq!(truncate("short"), "short");
    }
}
