//! Read-only code lenses: every block definition shows whether its node is
//! currently established, straight from evaluation's judgement.
//!
//! A lens is informational — its command is empty, so clicking does nothing.
//! Approval records are read from disk, not through the overlay store: an
//! editor session edits the argument, while records are appended by `inka
//! approve` outside it. The server asks clients that support it to re-request
//! lenses when a watched file changes (see the server's refresh request).

use std::collections::HashMap;
use std::path::Path;

use lsp_types::{CodeLens, Command, Range};

use inka_core::approvals::{self, APPROVALS_DIR};
use inka_core::eval::{Blame, evaluate};
use inka_core::graph::Graph;
use inka_core::project::Project;
use inka_core::source::FileId;
use inka_core::view::ArgumentView;

use crate::position;

/// The lenses for every block defined in `file`, or `None` when the argument
/// cannot be evaluated at all (a graph that does not build, a phase the order
/// does not list) — those are diagnostics' findings, not lens content. The
/// graph gate comes first, so a broken argument answers `None` whatever the
/// record store looks like.
pub fn lenses(root: &Path, project: &Project, file: FileId) -> Option<Vec<CodeLens>> {
    let source = project.source_map.file(file)?;
    let graph = Graph::of_project(project).ok()?;
    let view = ArgumentView::build(&graph, &project.source_map);
    let records = match approvals::load_dir(&root.join(APPROVALS_DIR)) {
        Ok(records) => records,
        // A record store that cannot be read is one loud lens, not a guess:
        // judging without the records would misreport every node.
        Err(error) => return Some(vec![note(format!("approvals unreadable: {error}"))]),
    };
    let evaluation = evaluate(&view, &project.manifest.phases, &records, &project.evidence).ok()?;
    let established: HashMap<&str, bool> = evaluation
        .nodes
        .iter()
        .map(|node| (node.id.as_str(), node.established))
        .collect();
    let blamed: HashMap<&str, &Blame> = evaluation
        .blame
        .iter()
        .map(|blame| (blame.node.as_str(), blame))
        .collect();

    let mut found = Vec::new();
    for block in &project.blocks {
        let id = &block.common().id;
        if id.span.file_id != file {
            continue;
        }
        let Some(&is_established) = established.get(id.value.as_str()) else {
            continue;
        };
        found.push(CodeLens {
            range: position::range(source, id.span),
            command: Some(Command {
                title: title(is_established, blamed.get(id.value.as_str()).copied()),
                command: String::new(),
                arguments: None,
            }),
            data: None,
        });
    }
    Some(found)
}

/// One line of verdict: established, the node's own blame reasons, or — with
/// no reason of its own — the only remaining cause, an unestablished child.
fn title(established: bool, blame: Option<&Blame>) -> String {
    if established {
        return "✓ established".to_owned();
    }
    match blame {
        Some(blame) => {
            let reasons: Vec<String> = blame.reasons.iter().map(ToString::to_string).collect();
            format!("✗ {}", reasons.join("; "))
        }
        None => "✗ a child is not established".to_owned(),
    }
}

/// A file-level notice pinned to the first line.
fn note(title: String) -> CodeLens {
    CodeLens {
        range: Range::default(),
        command: Some(Command {
            title,
            command: String::new(),
            arguments: None,
        }),
        data: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use inka_core::eval::BlameReason;
    use inka_core::view::Mode;

    #[test]
    fn title_reads_the_verdict_and_its_own_reasons() {
        // Arrange
        let blame = Blame {
            node: "leaf".to_owned(),
            mode: Mode::Fiat,
            reasons: vec![BlameReason::MissingApproval],
        };

        // Act & Assert
        assert_eq!(title(true, None), "✓ established");
        assert_eq!(
            title(false, Some(&blame)),
            "✗ no approval covers the current content"
        );
        assert_eq!(title(false, None), "✗ a child is not established");
    }
}
