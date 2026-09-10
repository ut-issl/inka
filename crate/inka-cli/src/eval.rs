//! The `inka eval` command: evaluate the argument as a whole and report
//! whether it is currently valid.
//!
//! The command is deliberately thin. All judgement lives in the core's
//! evaluation pass; this module loads the inputs — project, records — hands
//! them over, and formats the verdict. The human output is a one-line summary
//! (`established 12/15`) followed by the blame list; `--json` emits the same
//! evaluation machine-readably. An argument that is not fully established
//! maps to a failed outcome, so the command can sit in CI as it is.

use std::io::{self, Write};
use std::path::Path;

use inka_core::approvals::{self, APPROVALS_DIR};
use inka_core::eval::{Blame, Evaluation, NodeStatus, evaluate};
use inka_core::source::{Diagnostics, Error, SourceMap};
use serde::Serialize;

use crate::check::Outcome;
use crate::load::{ProjectView, load_project_view};
use crate::render::render;

/// Runs `inka eval` against `project_dir`, writing the verdict to `out` and
/// any findings to `err`.
///
/// Nothing is written to `out` unless the argument loaded and evaluated: a
/// partial verdict would read as a smaller argument's clean one.
///
/// # Errors
///
/// Propagates any I/O error from writing to `out` or `err`.
pub fn run(
    project_dir: &Path,
    json: bool,
    out: &mut impl Write,
    err: &mut impl Write,
    color: bool,
) -> io::Result<Outcome> {
    let Some(ProjectView { manifest, view }) = load_project_view(project_dir, err, color)? else {
        return Ok(Outcome::Failed);
    };
    let records = match approvals::load_dir(&project_dir.join(APPROVALS_DIR)) {
        Ok(records) => records,
        Err(error) => return fail(&error.to_string(), err, color),
    };
    let evaluation = match evaluate(&view, &manifest.phases, &records) {
        Ok(evaluation) => evaluation,
        Err(error) => return fail(&error.to_string(), err, color),
    };

    if json {
        write_json(&evaluation, out)?;
    } else {
        write_human(&evaluation, out)?;
    }
    Ok(if evaluation.is_clean() {
        Outcome::Ok
    } else {
        Outcome::Failed
    })
}

/// Reports one unanchored error through the shared rendering path.
fn fail(message: &str, err: &mut impl Write, color: bool) -> io::Result<Outcome> {
    let diagnostics = Diagnostics {
        errors: vec![Error::unanchored(message.to_owned())],
        warnings: Vec::new(),
    };
    render(&SourceMap::new(), &diagnostics, err, color)?;
    Ok(Outcome::Failed)
}

/// The summary line and, when anything failed, the blame list — one line per
/// reason, spelled `<kind>:<id>: <reason>`.
fn write_human(evaluation: &Evaluation, out: &mut impl Write) -> io::Result<()> {
    writeln!(
        out,
        "established {}/{}",
        evaluation.established_count(),
        evaluation.total()
    )?;
    if evaluation.blame.is_empty() {
        return Ok(());
    }
    writeln!(out, "blame:")?;
    for blame in &evaluation.blame {
        for reason in &blame.reasons {
            writeln!(out, "  {}:{}: {reason}", blame.mode.name(), blame.node)?;
        }
    }
    Ok(())
}

/// The machine-readable evaluation: the counts plus the core's own
/// serialization of every verdict and blame entry.
#[derive(Debug, Serialize)]
struct JsonReport<'e> {
    established: usize,
    total: usize,
    nodes: &'e [NodeStatus],
    blame: &'e [Blame],
}

fn write_json(evaluation: &Evaluation, out: &mut impl Write) -> io::Result<()> {
    let report = JsonReport {
        established: evaluation.established_count(),
        total: evaluation.total(),
        nodes: &evaluation.nodes,
        blame: &evaluation.blame,
    };
    let mut json = serde_json::to_string_pretty(&report).map_err(io::Error::other)?;
    json.push('\n');
    out.write_all(json.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    use inka_core::eval::BlameReason;
    use inka_core::view::Mode;

    fn evaluation() -> Evaluation {
        Evaluation {
            nodes: vec![
                NodeStatus {
                    id: "leaf".to_owned(),
                    mode: Mode::Fiat,
                    established: true,
                },
                NodeStatus {
                    id: "root".to_owned(),
                    mode: Mode::Objective,
                    established: false,
                },
            ],
            blame: vec![Blame {
                node: "root".to_owned(),
                mode: Mode::Objective,
                reasons: vec![BlameReason::MissingApproval],
            }],
        }
    }

    #[test]
    fn the_human_report_is_a_summary_and_one_line_per_reason() {
        // Arrange
        let mut out = Vec::new();

        // Act
        write_human(&evaluation(), &mut out).expect("writing to a vec succeeds");

        // Assert
        let text = String::from_utf8(out).expect("the report is UTF-8");
        assert_eq!(
            text,
            "established 1/2\nblame:\n  objective:root: no approval covers the current content\n"
        );
    }

    #[test]
    fn a_clean_evaluation_prints_the_summary_alone() {
        // Arrange
        let clean = Evaluation {
            nodes: vec![NodeStatus {
                id: "leaf".to_owned(),
                mode: Mode::Fiat,
                established: true,
            }],
            blame: Vec::new(),
        };
        let mut out = Vec::new();

        // Act
        write_human(&clean, &mut out).expect("writing to a vec succeeds");

        // Assert
        assert_eq!(out, b"established 1/1\n");
    }

    #[test]
    fn the_json_report_carries_counts_verdicts_and_blame() {
        // Arrange
        let mut out = Vec::new();

        // Act
        write_json(&evaluation(), &mut out).expect("writing to a vec succeeds");

        // Assert
        let json: serde_json::Value =
            serde_json::from_slice(&out).expect("the report is valid JSON");
        assert_eq!(json["established"], 1);
        assert_eq!(json["total"], 2);
        assert_eq!(json["nodes"][1]["id"], "root");
        assert_eq!(json["nodes"][1]["established"], false);
        assert_eq!(json["blame"][0]["node"], "root");
        assert_eq!(json["blame"][0]["reasons"][0]["cause"], "missing_approval");
    }
}
