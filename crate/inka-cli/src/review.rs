//! The `inka review` command: the queue of targets needing a decision, an
//! interactive loop over it, and a non-interactive single-target form.
//!
//! The queue is derived in the core by the same validity function `inka eval`
//! reads, so the two never disagree; this module only loads the inputs,
//! renders the queue, and writes records. Writing is append-only by
//! construction: every decision goes through [`approvals::append`], and no
//! code path here rewrites or deletes an existing line. Timestamps are taken
//! in this layer and passed into the pure record constructors.
//!
//! An interactive session judges the project as it was loaded: every decision
//! binds to the hash that was on screen, so an edit made elsewhere while the
//! session runs cannot be approved unseen — a record against the older
//! content simply reads as stale, exactly as if it had been written before
//! the edit.

use std::io::{self, BufRead, Write};
use std::path::Path;

use inka_core::approvals::{self, APPROVALS_DIR};
use inka_core::bind;
use inka_core::calc_evidence::CalcEvidenceSet;
use inka_core::evidence::EvidenceSet;
use inka_core::hash::{LocalView, ViewHash};
use inka_core::record::{Record, RecordFields, Target, Timestamp};
use inka_core::review::{BlockedReason, QueueItem, QueueState, ReviewQueue, derive_queue};
use inka_core::source::{Diagnostics, Error, SourceMap};
use inka_core::view::{ArgumentView, EvidenceKind, Mode, NodeComments, Property, ViewNode};

use crate::check::Outcome;
use crate::load::{ProjectView, load_project_view};
use crate::render::render;

/// What one `inka review` invocation was asked to do.
#[derive(Debug)]
pub struct Request {
    /// Who is reviewing; records append to `approvals/<approver>.jsonl`.
    pub approver: String,
    /// The key id records name; the approver name when not given.
    pub key_id: Option<String>,
    /// The selected form of the command.
    pub action: Action,
}

/// The three forms of the command.
#[derive(Debug)]
pub enum Action {
    /// Show the queue and exit.
    List,
    /// Record one decision on one named target.
    Single {
        /// The block id the decision targets.
        target: String,
        /// The decision to record.
        decision: Decision,
    },
    /// Walk the queue one target at a time.
    Interactive,
}

/// A decision the non-interactive form records.
#[derive(Debug)]
pub enum Decision {
    /// `--approve`, with an optional comment.
    Approve {
        /// An optional note the record carries.
        comment: Option<String>,
    },
    /// `--decline`; the comment is mandatory and clap enforces it.
    Decline {
        /// Why the content is vetoed.
        comment: String,
    },
}

/// The command's connection to the terminal, injected so tests can drive the
/// interactive loop with a scripted reader and a fixed clock.
pub struct Session<'a> {
    /// Where interactive answers are read from.
    pub input: &'a mut dyn BufRead,
    /// Whether that input is a terminal; the interactive form refuses a pipe.
    pub input_is_tty: bool,
    /// The clock, read once per recorded decision.
    pub now: &'a mut dyn FnMut() -> Timestamp,
}

/// Runs `inka review` against `project_dir`, writing the queue or recorded
/// decisions to `out` and any findings to `err`.
///
/// # Errors
///
/// Propagates any I/O error from writing to `out` or `err`.
pub fn run(
    project_dir: &Path,
    request: &Request,
    mut session: Session<'_>,
    out: &mut impl Write,
    err: &mut impl Write,
    color: bool,
) -> io::Result<Outcome> {
    let Some(ProjectView {
        manifest,
        evidence,
        calc_evidence,
        view,
    }) = load_project_view(project_dir, err, color)?
    else {
        return Ok(Outcome::Failed);
    };
    let records = match approvals::load_dir(&project_dir.join(APPROVALS_DIR)) {
        Ok(records) => records,
        Err(error) => return fail(&error.to_string(), err, color),
    };
    let context = RecordContext {
        project_dir,
        approver: &request.approver,
        key_id: request.key_id.as_deref(),
        phase: &manifest.phases.current,
    };

    match &request.action {
        Action::Single { target, decision } => {
            let imported = Imported {
                tests: &evidence,
                calcs: &calc_evidence,
            };
            let answer = single(&context, &view, imported, target, decision, &mut session);
            match answer {
                Ok(line) => {
                    writeln!(out, "{line}")?;
                    Ok(Outcome::Ok)
                }
                Err(message) => fail(&message, err, color),
            }
        }
        Action::List => {
            let queue =
                match derive_queue(&view, &manifest.phases, &records, &evidence, &calc_evidence) {
                    Ok(queue) => queue,
                    Err(error) => return fail(&error.to_string(), err, color),
                };
            write_queue(&queue, out)?;
            Ok(Outcome::Ok)
        }
        Action::Interactive => {
            let queue =
                match derive_queue(&view, &manifest.phases, &records, &evidence, &calc_evidence) {
                    Ok(queue) => queue,
                    Err(error) => return fail(&error.to_string(), err, color),
                };
            interactive(&context, &view, &queue, &mut session, out, err, color)
        }
    }
}

/// The imported evidence a bind's current hash is resolved against.
#[derive(Clone, Copy)]
struct Imported<'a> {
    tests: &'a EvidenceSet,
    calcs: &'a CalcEvidenceSet,
}

/// Everything a recorded decision needs besides the decision itself.
struct RecordContext<'a> {
    project_dir: &'a Path,
    approver: &'a str,
    key_id: Option<&'a str>,
    phase: &'a str,
}

/// Records one decision on one named target, whatever its queue state: an
/// extra approval or a veto is a judgement too, and counting is eval's job.
/// Returns the confirmation line, or the failure to report.
fn single(
    context: &RecordContext<'_>,
    view: &ArgumentView,
    imported: Imported<'_>,
    target: &str,
    decision: &Decision,
    session: &mut Session<'_>,
) -> Result<String, String> {
    let Some(node) = view.node(target) else {
        return Err(format!("no block named `{target}` in the argument"));
    };
    let current_hash = current_hash(view, node, imported)?;
    let (label, comment) = match decision {
        Decision::Approve { comment } => (ReviewLabel::Approve, comment.clone()),
        Decision::Decline { comment } => (ReviewLabel::Decline, Some(comment.clone())),
    };
    write_record(context, node, current_hash, label, comment, session)?;
    Ok(format!(
        "recorded {} of {}:{} in {APPROVALS_DIR}/{}.jsonl",
        label.name(),
        node.mode.name(),
        node.id,
        context.approver
    ))
}

/// Walks the actionable queue one target at a time, reading `a`/`d`/`s`/`q`.
fn interactive(
    context: &RecordContext<'_>,
    view: &ArgumentView,
    queue: &ReviewQueue,
    session: &mut Session<'_>,
    out: &mut impl Write,
    err: &mut impl Write,
    color: bool,
) -> io::Result<Outcome> {
    if !session.input_is_tty {
        return fail(
            "standard input is not a terminal; use --list to see the queue, or \
             `<id> --approve` / `<id> --decline --comment \"...\"` to record one decision",
            err,
            color,
        );
    }

    write_information(queue, out)?;
    let actionable: Vec<&QueueItem> = queue
        .items
        .iter()
        .filter(|item| !matches!(item.state, QueueState::Declined { .. }))
        .collect();
    if actionable.is_empty() {
        writeln!(out, "nothing needs a decision")?;
        return Ok(Outcome::Ok);
    }

    let total = actionable.len();
    let mut recorded = 0usize;
    let mut failed = 0usize;
    for (index, item) in actionable.iter().enumerate() {
        let node = view
            .node(&item.id)
            .expect("the queue names nodes of the same view");
        write_target(node, item, index + 1, total, view, out)?;
        let decision = match prompt_decision(session, out)? {
            Answer::Approve => Some((ReviewLabel::Approve, None)),
            Answer::Decline => {
                let comment = prompt_comment(session, out)?;
                if comment.is_none() {
                    writeln!(out, "no comment given; skipped")?;
                }
                comment.map(|comment| (ReviewLabel::Decline, Some(comment)))
            }
            Answer::Skip => None,
            Answer::Quit => break,
        };
        if let Some((label, comment)) = decision {
            match write_record(context, node, item.current_hash, label, comment, session) {
                Ok(()) => {
                    writeln!(
                        out,
                        "recorded {} of {}:{}",
                        label.name(),
                        node.mode.name(),
                        node.id
                    )?;
                    recorded += 1;
                }
                Err(message) => {
                    // Surface the failure next to the prompt it answers, not
                    // after the whole session, and fail the command: a
                    // decision the reviewer took but the store did not keep
                    // must not exit zero.
                    fail(&message, err, color)?;
                    err.flush()?;
                    failed += 1;
                }
            }
        }
    }
    writeln!(
        out,
        "decisions recorded: {recorded}; targets remaining: {}",
        total - recorded
    )?;
    Ok(if failed == 0 {
        Outcome::Ok
    } else {
        Outcome::Failed
    })
}

/// The two record labels, spelled for construction and confirmation lines.
#[derive(Clone, Copy)]
enum ReviewLabel {
    Approve,
    Decline,
}

impl ReviewLabel {
    const fn name(self) -> &'static str {
        match self {
            ReviewLabel::Approve => "approve",
            ReviewLabel::Decline => "decline",
        }
    }
}

/// One interactive answer.
enum Answer {
    Approve,
    Decline,
    Skip,
    Quit,
}

/// Reads one input line, tolerating invalid UTF-8: a stray byte becomes the
/// replacement character instead of aborting the session mid-queue. `None`
/// means end of input.
fn read_answer(session: &mut Session<'_>) -> io::Result<Option<String>> {
    let mut bytes = Vec::new();
    if session.input.read_until(b'\n', &mut bytes)? == 0 {
        return Ok(None);
    }
    Ok(Some(String::from_utf8_lossy(&bytes).trim().to_owned()))
}

/// Prompts until one of `a`/`d`/`s`/`q` is read; end of input quits.
fn prompt_decision(session: &mut Session<'_>, out: &mut impl Write) -> io::Result<Answer> {
    loop {
        write!(out, "(a)pprove / (d)ecline / (s)kip / (q)uit > ")?;
        out.flush()?;
        let Some(line) = read_answer(session)? else {
            writeln!(out)?;
            return Ok(Answer::Quit);
        };
        match line.as_str() {
            "a" => return Ok(Answer::Approve),
            "d" => return Ok(Answer::Decline),
            "s" => return Ok(Answer::Skip),
            "q" => return Ok(Answer::Quit),
            _ => writeln!(out, "please answer a, d, s or q")?,
        }
    }
}

/// Prompts for the mandatory decline comment; `None` when input ended.
fn prompt_comment(session: &mut Session<'_>, out: &mut impl Write) -> io::Result<Option<String>> {
    loop {
        write!(out, "comment (required) > ")?;
        out.flush()?;
        let Some(comment) = read_answer(session)? else {
            writeln!(out)?;
            return Ok(None);
        };
        if !comment.is_empty() {
            return Ok(Some(comment));
        }
        writeln!(out, "a decline must say why")?;
    }
}

/// Builds and appends one record; the only write path the command has.
fn write_record(
    context: &RecordContext<'_>,
    node: &ViewNode,
    current_hash: ViewHash,
    label: ReviewLabel,
    comment: Option<String>,
    session: &mut Session<'_>,
) -> Result<(), String> {
    let target = Target::new(node.mode, &node.id).map_err(|error| error.to_string())?;
    let fields = RecordFields {
        target,
        view_hash: current_hash,
        approver: context.approver.to_owned(),
        key_id: context.key_id.unwrap_or(context.approver).to_owned(),
        timestamp: (session.now)(),
        phase: context.phase.to_owned(),
        until: None,
        policy_hash: None,
    };
    let record = match label {
        ReviewLabel::Approve => Record::approve(fields, comment),
        ReviewLabel::Decline => {
            Record::decline(fields, comment.expect("a decline carries a comment"))
        }
    }
    .map_err(|error| error.to_string())?;
    approvals::append(&context.project_dir.join(APPROVALS_DIR), &record)
        .map_err(|error| error.to_string())
}

/// The view hash a decision recorded now would name, or why none exists.
fn current_hash(
    view: &ArgumentView,
    node: &ViewNode,
    imported: Imported<'_>,
) -> Result<ViewHash, String> {
    let evidence = match node.mode {
        Mode::Bind => {
            let resolved = bind::resolve(view, node, imported.tests, imported.calcs)
                .map_err(|error| error.to_string())?;
            let Some(evidence) = resolved else {
                let reference = node
                    .evidence
                    .as_ref()
                    .expect("resolve reports a bind without a reference as an error");
                return Err(format!(
                    "bind `{}` has no imported {} evidence for `{}`; deliver the evidence first",
                    node.id,
                    reference.kind.name(),
                    reference.target
                ));
            };
            Some(evidence.contract())
        }
        Mode::Objective | Mode::Structure | Mode::Fiat => None,
    };
    LocalView::of_node(view, node, evidence)
        .map(|local| local.view_hash())
        .map_err(|error| error.to_string())
}

/// The `--list` rendering: one line per queued target, then blocked targets.
fn write_queue(queue: &ReviewQueue, out: &mut impl Write) -> io::Result<()> {
    if queue.items.is_empty() {
        writeln!(out, "nothing needs review")?;
    } else {
        writeln!(out, "targets needing review: {}", queue.items.len())?;
        for item in &queue.items {
            writeln!(
                out,
                "  {}:{} — {}",
                item.mode.name(),
                item.id,
                state_line(&item.state)
            )?;
        }
    }
    write_blocked(queue, out)
}

/// The informational prelude of the interactive loop: declines and blocked
/// targets, which the loop will not prompt on.
fn write_information(queue: &ReviewQueue, out: &mut impl Write) -> io::Result<()> {
    for item in &queue.items {
        if let QueueState::Declined { .. } = &item.state {
            writeln!(
                out,
                "{}:{} — {} (edit or retract to resolve)",
                item.mode.name(),
                item.id,
                state_line(&item.state)
            )?;
        }
    }
    write_blocked(queue, out)
}

fn write_blocked(queue: &ReviewQueue, out: &mut impl Write) -> io::Result<()> {
    if queue.blocked.is_empty() {
        return Ok(());
    }
    writeln!(out, "blocked:")?;
    for item in &queue.blocked {
        writeln!(
            out,
            "  {}:{} — {}",
            item.mode.name(),
            item.id,
            blocked_line(&item.reason)
        )?;
    }
    Ok(())
}

/// One queued target, rendered as the local view the decision would judge:
/// the expanded description, the evidence reference, where the local view
/// includes them the direct children's descriptions, and the deadline, each
/// with the comment attached to it written above it as `//` lines, the way
/// the formatter places comments in a source file. Nothing deeper is shown —
/// the local view is all an approval binds to.
fn write_target(
    node: &ViewNode,
    item: &QueueItem,
    position: usize,
    total: usize,
    view: &ArgumentView,
    out: &mut impl Write,
) -> io::Result<()> {
    writeln!(
        out,
        "\n[{position}/{total}] {}:{} — {}",
        node.mode.name(),
        node.id,
        state_line(&item.state)
    )?;
    let comments = &node.comments;
    write_comment(comments.block.as_deref(), 2, out)?;
    write_indented(&node.description, 2, out)?;
    if let Some(reference) = &node.evidence {
        let property = match reference.kind {
            EvidenceKind::Test => Property::Test,
            EvidenceKind::Calc => Property::Calc,
        };
        let quoted = format!("\"{}\"", reference.target);
        write_property(comments, property, &quoted, out)?;
    }
    write_children(node, view, out)?;
    if let Some(deadline) = &node.deadline {
        write_property(comments, Property::Until, &deadline.until, out)?;
        if let Some(promote) = deadline.promote {
            write_property(comments, Property::Promote, promote.name(), out)?;
        }
    }
    Ok(())
}

/// The `children:` property: each direct child's id and description, under
/// the comment on its entry.
fn write_children(node: &ViewNode, view: &ArgumentView, out: &mut impl Write) -> io::Result<()> {
    if node.children.is_empty() {
        return Ok(());
    }
    let comments = &node.comments;
    write_comment(property_comment(comments, Property::Children), 2, out)?;
    writeln!(out, "  children:")?;
    for child in &node.children {
        let description = view.node(child).map_or("(not defined)", |child_node| {
            child_node.description.as_str()
        });
        write_comment(comments.children.get(child).map(String::as_str), 4, out)?;
        write_indented(&format!("{child}:"), 4, out)?;
        write_indented(description, 6, out)?;
    }
    Ok(())
}

/// One `key: value` property line under the comment attached to it.
fn write_property(
    comments: &NodeComments,
    property: Property,
    value: &str,
    out: &mut impl Write,
) -> io::Result<()> {
    write_comment(property_comment(comments, property), 2, out)?;
    write_indented(&format!("{}: {value}", property.name()), 2, out)
}

fn property_comment(comments: &NodeComments, property: Property) -> Option<&str> {
    comments.properties.get(&property).map(String::as_str)
}

/// A comment as `//` lines, `//` alone for an empty line.
fn write_comment(text: Option<&str>, indent: usize, out: &mut impl Write) -> io::Result<()> {
    let Some(text) = text else {
        return Ok(());
    };
    for line in text.lines() {
        let line = sanitize(line);
        if line.is_empty() {
            writeln!(out, "{:indent$}//", "")?;
        } else {
            writeln!(out, "{:indent$}// {line}", "")?;
        }
    }
    Ok(())
}

fn write_indented(text: &str, indent: usize, out: &mut impl Write) -> io::Result<()> {
    for line in text.lines() {
        writeln!(out, "{:indent$}{}", "", sanitize(line))?;
    }
    Ok(())
}

/// Text out of a source file or a record, made safe for the terminal.
///
/// Control characters must not reach it, where an escape sequence could spoof
/// the prompt the reviewer answers; tabs are ordinary content. Bidirectional
/// embedding, override, and isolate controls go too, so text cannot be shown
/// in an order other than the one that was hashed. Other format characters,
/// such as the joiners emoji are built from, stay.
fn sanitize(line: &str) -> String {
    line.chars()
        .filter(|&c| (!c.is_control() || c == '\t') && !is_bidi_control(c))
        .collect()
}

fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}

fn state_line(state: &QueueState) -> String {
    match state {
        QueueState::Missing => "no approval covers the current content".to_owned(),
        QueueState::Stale { approvers } => format!(
            "approvals are stale: {} approved an earlier version",
            sanitize(&approvers.join(", "))
        ),
        QueueState::Declined { by } => format!("declined by {}", sanitize(&by.join(", "))),
    }
}

fn blocked_line(reason: &BlockedReason) -> String {
    match reason {
        BlockedReason::MissingEvidence { kind, target } => {
            format!(
                "no imported {} evidence provides `{}`",
                kind.name(),
                sanitize(target)
            )
        }
        BlockedReason::UntilExpired { until } => {
            format!(
                "approvals no longer count: the current phase is past until:{}",
                sanitize(until)
            )
        }
    }
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

/// The current instant in the record spelling, truncated to whole seconds.
///
/// # Panics
///
/// Panics if the system clock reports an instant outside jiff's range, which
/// no real clock does.
#[must_use]
pub fn now() -> Timestamp {
    let second = jiff::Timestamp::now().as_second();
    let instant = jiff::Timestamp::from_second(second).expect("the clock is in jiff's range");
    instant
        .to_string()
        .parse()
        .expect("jiff prints the record spelling")
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::BTreeMap;
    use std::fs;
    use std::io::Cursor;

    use inka_core::view::Evidence;

    /// Writes the three-node scenario project and returns its directory.
    fn scenario_project() -> tempfile::TempDir {
        project_with(
            "objective root {\n  ```\n  the root claim\n  ```\n  children: branch\n}\n\n\
             structure branch {\n  ```\n  the branch claim\n  ```\n  children: leaf\n}\n\n\
             fiat leaf {\n  ```\n  taken as given\n  ```\n}\n",
        )
    }

    /// Writes a one-file project holding `argument` and returns its directory.
    fn project_with(argument: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("a temp dir creates");
        fs::write(
            dir.path().join("inka.toml"),
            "[project]\nname = \"review-scenario\"\n\n[phases]\norder = [\"M0\"]\ncurrent = \"M0\"\n\n[values]\nfiles = []\n",
        )
        .expect("the manifest writes");
        fs::create_dir(dir.path().join("argument")).expect("the argument directory creates");
        fs::write(dir.path().join("argument/arg.inka"), argument).expect("the argument writes");
        dir
    }

    /// Runs the interactive form against `dir` with `answers` as scripted
    /// terminal input, returning the outcome and the captured stdout.
    fn run_interactive_bytes(dir: &Path, answers: &[u8]) -> (Outcome, String) {
        let request = Request {
            approver: "alice".to_owned(),
            key_id: None,
            action: Action::Interactive,
        };
        let mut input = Cursor::new(answers.to_owned());
        let mut now = || -> Timestamp {
            "2026-11-01T00:00:00Z"
                .parse()
                .expect("the fixed instant parses")
        };
        let session = Session {
            input: &mut input,
            input_is_tty: true,
            now: &mut now,
        };
        let mut out = Vec::new();
        let mut err = Vec::new();
        let outcome = run(dir, &request, session, &mut out, &mut err, false)
            .expect("writing to vecs succeeds");
        (
            outcome,
            String::from_utf8(out).expect("the transcript is UTF-8"),
        )
    }

    /// The happy-path wrapper: the loop itself must succeed.
    fn run_interactive(dir: &Path, answers: &str) -> String {
        let (outcome, transcript) = run_interactive_bytes(dir, answers.as_bytes());
        assert!(
            matches!(outcome, Outcome::Ok),
            "the loop itself succeeds:\n{transcript}"
        );
        transcript
    }

    fn record_lines(dir: &Path) -> Vec<String> {
        fs::read_to_string(dir.join("approvals/alice.jsonl"))
            .map(|text| text.lines().map(str::to_owned).collect())
            .unwrap_or_default()
    }

    #[test]
    fn approving_every_target_interactively_records_three_decisions() {
        // Arrange
        let dir = scenario_project();

        // Act
        let transcript = run_interactive(dir.path(), "a\na\na\n");

        // Assert: queue order is sorted block IDs — branch, leaf, root.
        assert!(transcript.contains("[1/3] structure:branch"));
        assert!(transcript.contains("the branch claim"));
        assert!(transcript.contains("children:"));
        assert!(transcript.contains("decisions recorded: 3; targets remaining: 0"));
        assert_eq!(record_lines(dir.path()).len(), 3);
    }

    #[test]
    fn a_decline_demands_a_comment_until_one_is_given() {
        // Arrange & Act: decline the first target, first with a blank comment.
        let dir = scenario_project();
        let transcript = run_interactive(dir.path(), "d\n\nthe branch claim is wrong\nq\n");

        // Assert: the blank was refused, the decline recorded, and quit left
        // the remaining targets untouched.
        assert!(transcript.contains("a decline must say why"));
        assert!(transcript.contains("recorded decline of structure:branch"));
        assert!(transcript.contains("decisions recorded: 1; targets remaining: 2"));
        let lines = record_lines(dir.path());
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("\"label\":\"decline\""));
        assert!(lines[0].contains("the branch claim is wrong"));
    }

    #[test]
    fn a_target_shows_each_comment_above_its_anchor_and_the_deadline() {
        // Arrange: a comment on every anchor a structure and a bind carry.
        let dir = project_with(
            "\
// a file comment, shown nowhere

objective root {
  ```
  the root claim
  ```
  children: branch
}

// shared with the thermal argument
structure branch {
  ```
  the branch claim
  ```
  // one entry per hazard
  children:
    // the only hazard so far
    leaf
  // revisit at CDR
  //
  // once the model lands
  until: M0
}

fiat leaf {
  ```
  checked in CI
  ```
  until: M0
  promote: bind // once the rig exists
}
",
        );

        // Act: walk past every target.
        let transcript = run_interactive(dir.path(), "s\ns\ns\nq\n");

        // Assert
        assert!(
            transcript.contains(
                "\
  // shared with the thermal argument
  the branch claim
  // one entry per hazard
  children:
    // the only hazard so far
    leaf:
      checked in CI
  // revisit at CDR
  //
  // once the model lands
  until: M0
"
            ),
            "{transcript}"
        );
        assert!(
            transcript.contains("  until: M0\n  // once the rig exists\n  promote: bind\n"),
            "{transcript}"
        );
        assert!(!transcript.contains("file comment"), "{transcript}");
    }

    #[test]
    fn an_evidence_reference_is_spelled_with_its_own_scheme_under_its_comment() {
        // Arrange: a calc bind is never queued, so the target is rendered
        // directly.
        let node = ViewNode {
            id: "checked".to_owned(),
            mode: Mode::Bind,
            namespace: "arg".to_owned(),
            description: "checked by a solver".to_owned(),
            tags: Vec::new(),
            evidence: Some(Evidence {
                kind: EvidenceKind::Calc,
                target: "AOCS::?check".to_owned(),
            }),
            deadline: None,
            children: Vec::new(),
            comments: NodeComments {
                properties: BTreeMap::from([(Property::Calc, "the shipped check".to_owned())]),
                ..NodeComments::default()
            },
        };
        let item = QueueItem {
            id: node.id.clone(),
            mode: node.mode,
            current_hash: LocalView::fiat("unused", None).view_hash(),
            state: QueueState::Missing,
        };
        let view = ArgumentView {
            nodes: vec![node.clone()],
            edges: Vec::new(),
            roots: Vec::new(),
        };
        let mut out = Vec::new();

        // Act
        write_target(&node, &item, 1, 1, &view, &mut out).expect("writing to a vec succeeds");

        // Assert
        let text = String::from_utf8(out).expect("UTF-8");
        assert!(
            text.ends_with(
                "  checked by a solver\n  // the shipped check\n  calc: \"AOCS::?check\"\n"
            ),
            "{text}"
        );
    }

    #[test]
    fn source_text_reaches_the_terminal_without_escapes_or_bidi_controls() {
        // Arrange: an escape sequence and a right-to-left override in a
        // comment, a line of nothing but control characters, and a tab.
        let dir = project_with(
            "objective root {\n  ```\n  the root claim\n  ```\n  children: leaf\n}\n\n\
             fiat leaf {\n  ```\n  taken\tas given\n  ```\n  \
             // red \u{1b}[31malert\u{202E} reversed\n  // \u{1b}\u{7}\n  until: M0\n}\n",
        );

        // Act
        let transcript = run_interactive(dir.path(), "s\ns\nq\n");

        // Assert
        assert!(!transcript.contains('\u{1b}'), "{transcript:?}");
        assert!(!transcript.contains('\u{202E}'), "{transcript:?}");
        assert!(
            transcript.contains("  // red [31malert reversed\n  //\n  until: M0\n"),
            "{transcript:?}"
        );
        assert!(transcript.contains("  taken\tas given\n"), "{transcript:?}");
    }

    #[test]
    fn sanitize_drops_controls_and_bidi_controls_but_keeps_tabs_and_joiners() {
        assert_eq!(sanitize("a\u{1b}b\tc"), "ab\tc");
        assert_eq!(sanitize("\u{2066}x\u{2069}\u{202A}y\u{202E}"), "xy");
        // A zero-width joiner builds emoji; it is not a bidi control.
        assert_eq!(
            sanitize("\u{1F469}\u{200D}\u{1F4BB}"),
            "\u{1F469}\u{200D}\u{1F4BB}"
        );
    }

    #[test]
    fn an_unrecognized_answer_reprompts_and_skip_moves_on() {
        // Arrange & Act
        let dir = scenario_project();
        let transcript = run_interactive(dir.path(), "x\ns\nq\n");

        // Assert
        assert!(transcript.contains("please answer a, d, s or q"));
        assert!(transcript.contains("decisions recorded: 0; targets remaining: 3"));
        assert_eq!(record_lines(dir.path()).len(), 0);
    }

    #[test]
    fn invalid_utf8_input_reprompts_instead_of_aborting() {
        // Arrange & Act: a stray byte, then a skip and a quit.
        let dir = scenario_project();
        let (outcome, transcript) = run_interactive_bytes(dir.path(), b"\xff\xfe\ns\nq\n");

        // Assert: the session survives the byte and records nothing.
        assert!(matches!(outcome, Outcome::Ok));
        assert!(transcript.contains("please answer a, d, s or q"));
        assert_eq!(record_lines(dir.path()).len(), 0);
    }

    #[cfg(unix)]
    #[test]
    fn a_store_failure_fails_the_command_instead_of_exiting_zero() {
        use std::os::unix::fs::PermissionsExt;

        // Arrange: an approvals directory nothing can write into.
        let dir = scenario_project();
        let approvals = dir.path().join("approvals");
        fs::create_dir(&approvals).expect("the approvals directory creates");
        fs::set_permissions(&approvals, fs::Permissions::from_mode(0o555))
            .expect("the permissions set");

        // Act: the reviewer approves, the store refuses.
        let (outcome, transcript) = run_interactive_bytes(dir.path(), b"a\nq\n");

        // Assert: the decision that was not kept must not exit zero.
        fs::set_permissions(&approvals, fs::Permissions::from_mode(0o755))
            .expect("the permissions restore");
        assert!(
            matches!(outcome, Outcome::Failed),
            "transcript:\n{transcript}"
        );
        assert!(transcript.contains("decisions recorded: 0"));
    }

    #[test]
    fn the_end_of_input_quits_the_loop() {
        // Arrange & Act: the terminal closed mid-queue.
        let dir = scenario_project();
        let transcript = run_interactive(dir.path(), "a\n");

        // Assert: one decision stands; nothing was invented for the rest.
        assert!(transcript.contains("decisions recorded: 1; targets remaining: 2"));
        assert_eq!(record_lines(dir.path()).len(), 1);
    }
}
