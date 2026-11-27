//! The `inka review` command: the queue of targets needing a decision, an
//! interactive loop over it, and a non-interactive single-target form.
//!
//! The queue is derived in the core by the same validity function `inka eval`
//! reads, so the two never disagree, and so are the content of a target and
//! the recording of a decision; this module only loads the inputs, prompts,
//! and renders. Writing is append-only by construction: every decision goes
//! through the core's [`record_decision`](core_review::record_decision), and
//! no code path here rewrites or deletes an existing line. Timestamps are
//! taken in this layer and passed in.
//!
//! An interactive session judges the project as it was loaded: every decision
//! binds to the hash that was on screen, so an edit made elsewhere while the
//! session runs cannot be approved unseen — a record against the older
//! content simply reads as stale, exactly as if it had been written before
//! the edit.
//!
//! Standard output is styled through the `console` crate. A hue means one of
//! three things: the review state (unapproved, stale, declined, blocked), the
//! status `inka eval` gives a child (established, blamed, not established)
//! or, in blue, a block id. The state is always spelled out too, so the
//! uncoloured rendering loses nothing. Styling wraps text that has already
//! been through [`sanitize`]; it is never read from a source file or a
//! record. Descriptions are wrapped to the terminal by `textwrap`, and the
//! guides that hang the children under `children:` are drawn by `termtree`.
//!
//! A target is the local view its approval binds to, and one thing more: the
//! frame of each direct child says where that child stands — its status, and
//! when it is blamed, why. The queue offers a parent whatever state its
//! children are in, and a child that is blocked never comes up in the walk at
//! all; without this a reviewer could not tell why a parent turned up before
//! the claims under it. The status is not part of the local view and no
//! approval binds to it.

use std::collections::HashMap;
use std::io::{self, BufRead, Write};
use std::path::Path;

use console::{Alignment, Style, measure_text_width, pad_str};
use inka_core::approvals::{self, APPROVALS_DIR};
use inka_core::argument::Argument;
use inka_core::evidence::EvidenceSet;
use inka_core::local::{ChildrenContent, LocalContent};
use inka_core::record::Timestamp;
use inka_core::review::{
    self as core_review, BlockedReason, QueueItem, QueueState, ReviewQueue, Reviewer, derive_queue,
};
use inka_core::source::{Diagnostics, Error, SourceMap};
use inka_core::view::{ArgumentView, Mode, Property, ViewNode};
use inka_core::wire::{self, NodeData, Snapshot, Status};
use termtree::{GlyphPalette, Tree};

use crate::check::Outcome;
use crate::load::load_project_view;
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
    /// How standard output is presented.
    pub screen: Screen,
}

/// How the command's standard output is presented, injected so tests render
/// deterministically whatever terminal they run in.
#[derive(Debug, Clone, Copy)]
pub struct Screen {
    /// Whether output carries ANSI styling.
    pub color: bool,
    /// The terminal width in columns, which the rule of a target's header
    /// fills.
    pub width: usize,
}

impl Screen {
    /// The width assumed when standard output is not a terminal.
    pub const DEFAULT_WIDTH: usize = 80;

    /// `style`, switched on or off by this screen alone: the crate's own
    /// terminal detection never decides.
    fn styled(self, style: Style) -> Style {
        style.force_styling(self.color)
    }

    /// A block id, wherever one is shown.
    fn id(self) -> Style {
        self.styled(Style::new().blue().bold())
    }

    /// Emphasis that names nothing: the keys of the prompt.
    fn bold(self) -> Style {
        self.styled(Style::new().bold())
    }

    fn dim(self) -> Style {
        self.styled(Style::new().dim())
    }

    fn key(self) -> Style {
        self.styled(Style::new().cyan())
    }

    /// The hue of a review state.
    fn group(self, group: Group) -> Style {
        self.styled(match group {
            Group::Unapproved => Style::new().yellow(),
            Group::Stale => Style::new().magenta(),
            Group::Declined => Style::new().red(),
            Group::Blocked => Style::new().dim(),
        })
    }

    /// The hue of a child's status, the same the web UI gives it.
    fn status(self, status: &Status) -> Style {
        self.styled(match status {
            Status::Established => Style::new().green(),
            Status::Blamed { .. } => Style::new().red(),
            Status::Unestablished => Style::new().dim(),
        })
    }

    fn label(self, label: ReviewLabel) -> Style {
        self.styled(match label {
            ReviewLabel::Approve => Style::new().green(),
            ReviewLabel::Decline => Style::new().red(),
        })
    }

    /// A target as `mode:id`, the mode receding behind the id.
    fn target(self, mode: Mode, id: &str) -> String {
        format!(
            "{}{}",
            self.dim().apply_to(format!("{}:", mode.name())),
            self.id().apply_to(id)
        )
    }
}

/// The review states the output groups and colours by.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Group {
    Unapproved,
    Stale,
    Declined,
    Blocked,
}

impl Group {
    const fn of(state: &QueueState) -> Self {
        match state {
            QueueState::Missing => Group::Unapproved,
            QueueState::Stale { .. } => Group::Stale,
            QueueState::Declined { .. } => Group::Declined,
        }
    }

    /// The state as a target's header spells it.
    const fn name(self) -> &'static str {
        match self {
            Group::Unapproved => "unapproved",
            Group::Stale => "stale",
            Group::Declined => "declined",
            Group::Blocked => "blocked",
        }
    }

    /// The state as a section heading spells it.
    const fn heading(self) -> &'static str {
        match self {
            Group::Unapproved => "Unapproved",
            Group::Stale => "Stale",
            Group::Declined => "Declined",
            Group::Blocked => "Blocked",
        }
    }
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
    let screen = session.screen;
    let Some(argument) = load_project_view(project_dir, err, color)? else {
        return Ok(Outcome::Failed);
    };
    let Argument {
        manifest,
        evidence,
        view,
        ..
    } = &argument;
    let records = match approvals::load_dir(&project_dir.join(APPROVALS_DIR)) {
        Ok(records) => records,
        Err(error) => return fail(&error.to_string(), err, color),
    };
    let approvals_dir = project_dir.join(APPROVALS_DIR);
    let context = Reviewer {
        approvals_dir: &approvals_dir,
        approver: &request.approver,
        key_id: request.key_id.as_deref(),
        phase: &manifest.phases.current,
    };

    match &request.action {
        Action::Single { target, decision } => {
            let answer = single(&context, view, evidence, target, decision, &mut session);
            match answer {
                Ok(line) => {
                    writeln!(out, "{line}")?;
                    Ok(Outcome::Ok)
                }
                Err(message) => fail(&message, err, color),
            }
        }
        Action::List => {
            let queue = match derive_queue(view, &manifest.phases, &records, evidence) {
                Ok(queue) => queue,
                Err(error) => return fail(&error.to_string(), err, color),
            };
            write_queue(&queue, screen, out)?;
            Ok(Outcome::Ok)
        }
        Action::Interactive => {
            let queue = match derive_queue(view, &manifest.phases, &records, evidence) {
                Ok(queue) => queue,
                Err(error) => return fail(&error.to_string(), err, color),
            };
            interactive(&context, &argument, &queue, &mut session, out, err, color)
        }
    }
}

/// Records one decision on one named target, whatever its queue state: an
/// extra approval or a veto is a judgement too, and counting is eval's job.
/// Returns the confirmation line, or the failure to report.
fn single(
    context: &Reviewer<'_>,
    view: &ArgumentView,
    evidence: &EvidenceSet,
    target: &str,
    decision: &Decision,
    session: &mut Session<'_>,
) -> Result<String, String> {
    let (label, decision) = match decision {
        Decision::Approve { comment } => (
            ReviewLabel::Approve,
            core_review::Decision::Approve {
                comment: comment.clone(),
            },
        ),
        Decision::Decline { comment } => (
            ReviewLabel::Decline,
            core_review::Decision::Decline {
                comment: comment.clone(),
            },
        ),
    };
    let timestamp = (session.now)();
    let record = core_review::decide(context, view, evidence, target, None, decision, timestamp)
        .map_err(|error| error.to_string())?;
    Ok(format!(
        "{} in {APPROVALS_DIR}/{}.jsonl",
        recorded_line(
            label,
            record.target().kind(),
            record.target().id(),
            session.screen
        ),
        context.approver
    ))
}

/// Where every node stands, by id: what a child's frame reads its status
/// from. A node missing from it is drawn without one.
type Statuses<'a> = HashMap<&'a str, &'a NodeData>;

fn statuses_of(nodes: &[NodeData]) -> Statuses<'_> {
    nodes.iter().map(|node| (node.id.as_str(), node)).collect()
}

/// Where every node stands under the records in the store right now: the
/// snapshot the web UI is served, so the two surfaces say the same of a
/// child.
fn standing(argument: &Argument, context: &Reviewer<'_>) -> Result<Snapshot, String> {
    let records = approvals::load_dir(context.approvals_dir).map_err(|error| error.to_string())?;
    wire::snapshot(argument, &records).map_err(|error| error.to_string())
}

/// Walks the actionable queue one target at a time, reading `a`/`d`/`s`/`q`.
///
/// The queue and the hash each decision binds to are the ones the session
/// was loaded with. The statuses shown on a target's children are not: they
/// are read again after every recorded decision, so a child approved a
/// moment ago is not still shown as lacking an approval.
fn interactive(
    context: &Reviewer<'_>,
    argument: &Argument,
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

    let view = &argument.view;
    let screen = session.screen;
    write_sections(queue, &[Group::Declined, Group::Blocked], true, screen, out)?;
    let actionable: Vec<&QueueItem> = queue
        .items
        .iter()
        .filter(|item| !matches!(item.state, QueueState::Declined { .. }))
        .collect();
    if actionable.is_empty() {
        writeln!(out, "nothing needs a decision")?;
        return Ok(Outcome::Ok);
    }

    let mut snapshot = match standing(argument, context) {
        Ok(snapshot) => snapshot,
        Err(message) => return fail(&message, err, color),
    };
    let total = actionable.len();
    let mut recorded = 0usize;
    let mut failed = 0usize;
    for (index, item) in actionable.iter().enumerate() {
        let node = view
            .node(&item.id)
            .expect("the queue names nodes of the same view");
        let statuses = statuses_of(&snapshot.nodes);
        write_target(node, item, index + 1, total, view, &statuses, screen, out)?;
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
            match write_record(context, node, item, label, comment, session) {
                Ok(()) => {
                    writeln!(out, "{}", recorded_line(label, node.mode, &node.id, screen))?;
                    recorded += 1;
                    match standing(argument, context) {
                        Ok(current) => snapshot = current,
                        // The decision is kept; only the statuses shown
                        // from here on are the ones read before it.
                        Err(message) => {
                            fail(&message, err, color)?;
                            err.flush()?;
                            failed += 1;
                        }
                    }
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

/// The confirmation of one recorded decision, the decision in its colour.
fn recorded_line(label: ReviewLabel, mode: Mode, id: &str, screen: Screen) -> String {
    format!(
        "{} of {}",
        screen
            .label(label)
            .apply_to(format!("recorded {}", label.name())),
        screen.target(mode, id)
    )
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
        let screen = session.screen;
        let approve = screen.label(ReviewLabel::Approve).bold();
        let decline = screen.label(ReviewLabel::Decline).bold();
        let key = screen.bold();
        write!(
            out,
            "[{}]pprove  [{}]ecline  [{}]kip  [{}]uit > ",
            approve.apply_to('a'),
            decline.apply_to('d'),
            key.apply_to('s'),
            key.apply_to('q')
        )?;
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

/// Records the reviewer's answer on one queued target, at the hash the
/// queue showed it with; the only write path the interactive loop has.
fn write_record(
    context: &Reviewer<'_>,
    node: &ViewNode,
    item: &QueueItem,
    label: ReviewLabel,
    comment: Option<String>,
    session: &mut Session<'_>,
) -> Result<(), String> {
    let decision = match label {
        ReviewLabel::Approve => core_review::Decision::Approve { comment },
        ReviewLabel::Decline => core_review::Decision::Decline {
            comment: comment.expect("a decline carries a comment"),
        },
    };
    let timestamp = (session.now)();
    core_review::record_decision(context, node, item.current_hash, decision, timestamp)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// The `--list` rendering: a summary line, then one section per state.
///
/// The state comes first: a reader of the list wants to know what is
/// unapproved, what went stale and what is vetoed. The queue's leaves-first
/// order therefore holds within each section, not across them — a stale leaf
/// is listed under `Stale` even when its unapproved parent is listed above it
/// under `Unapproved`. The interactive walk, which does not group, follows the
/// queue's order throughout.
fn write_queue(queue: &ReviewQueue, screen: Screen, out: &mut impl Write) -> io::Result<()> {
    let summary = match queue.items.len() {
        0 => "nothing needs review".to_owned(),
        1 => "1 target needs review".to_owned(),
        count => format!("{count} targets need review"),
    };
    if queue.blocked.is_empty() {
        writeln!(out, "{summary}")?;
    } else {
        writeln!(out, "{summary}, {} blocked", queue.blocked.len())?;
    }
    if queue.items.is_empty() && queue.blocked.is_empty() {
        return Ok(());
    }
    writeln!(out)?;
    write_sections(
        queue,
        &[
            Group::Unapproved,
            Group::Stale,
            Group::Declined,
            Group::Blocked,
        ],
        false,
        screen,
        out,
    )
}

/// One line of a section: the target, and what its state adds to the heading.
struct Row {
    target: String,
    detail: Option<String>,
}

/// The targets of each of `groups` under a heading that names the state and
/// counts them, sections separated by a blank line and empty ones omitted.
/// The details share one column across sections.
///
/// `--list` shows every group. The interactive loop shows, as its `prelude`,
/// the declined and blocked targets it will not prompt on.
fn write_sections(
    queue: &ReviewQueue,
    groups: &[Group],
    prelude: bool,
    screen: Screen,
    out: &mut impl Write,
) -> io::Result<()> {
    let sections: Vec<(Group, Vec<Row>)> = groups
        .iter()
        .map(|&group| (group, section_rows(queue, group, prelude, screen)))
        .filter(|(_, rows)| !rows.is_empty())
        .collect();
    let column = sections
        .iter()
        .flat_map(|(_, rows)| rows)
        .filter(|row| row.detail.is_some())
        .map(|row| measure_text_width(&row.target))
        .max()
        .unwrap_or(0);

    for (index, (group, rows)) in sections.iter().enumerate() {
        if index > 0 {
            writeln!(out)?;
        }
        let heading = format!("{} ({})", group.heading(), rows.len());
        writeln!(out, "{}", screen.group(*group).bold().apply_to(heading))?;
        for row in rows {
            match &row.detail {
                Some(detail) => writeln!(
                    out,
                    "  {}  {}",
                    pad_str(&row.target, column, Alignment::Left, None),
                    screen.dim().apply_to(detail)
                )?,
                None => writeln!(out, "  {}", row.target)?,
            }
        }
    }
    Ok(())
}

fn section_rows(queue: &ReviewQueue, group: Group, prelude: bool, screen: Screen) -> Vec<Row> {
    if group == Group::Blocked {
        return queue
            .blocked
            .iter()
            .map(|item| Row {
                target: screen.target(item.mode, &item.id),
                detail: Some(blocked_line(&item.reason)),
            })
            .collect();
    }
    queue
        .items
        .iter()
        .filter(|item| Group::of(&item.state) == group)
        .map(|item| Row {
            target: screen.target(item.mode, &item.id),
            detail: state_detail(&item.state).map(|detail| {
                if prelude && group == Group::Declined {
                    format!("{detail} (edit or retract to resolve)")
                } else {
                    detail
                }
            }),
        })
        .collect()
}

/// One queued target, rendered as the local view the decision would judge:
/// the expanded description, the evidence reference, where the local view
/// includes them the direct children's descriptions, and the deadline, each
/// with the comment attached to it written above it as `//` lines, the way
/// the formatter places comments in a source file. Nothing deeper is shown —
/// the local view is all an approval binds to. The one addition is each
/// child's status from `statuses`, which no approval binds to.
///
/// A ruled header as wide as the terminal opens the target, so one target
/// does not run into the next; a blank line closes it, ahead of the prompt.
#[allow(clippy::too_many_arguments)]
fn write_target(
    node: &ViewNode,
    item: &QueueItem,
    position: usize,
    total: usize,
    view: &ArgumentView,
    statuses: &Statuses<'_>,
    screen: Screen,
    out: &mut impl Write,
) -> io::Result<()> {
    let group = Group::of(&item.state);
    let rule = screen.dim();
    let header = format!(
        "{} [{position}/{total}] {} {} {} ",
        rule.apply_to("──"),
        screen.target(node.mode, &node.id),
        rule.apply_to("──"),
        screen.group(group).apply_to(group.name())
    );
    let fill = screen.width.saturating_sub(measure_text_width(&header));
    writeln!(out, "\n{header}{}", rule.apply_to("─".repeat(fill)))?;
    if let Some(detail) = state_detail(&item.state) {
        writeln!(out, "   {}", screen.group(group).apply_to(detail))?;
    }
    writeln!(out)?;

    let content = LocalContent::of(view, node);
    write_comment(content.comment.as_deref(), screen, out)?;
    write_description(&content.description, group, screen, out)?;
    if let Some(evidence) = &content.evidence {
        let quoted = format!("\"{}\"", evidence.target);
        write_property(
            evidence.comment.as_deref(),
            evidence.property(),
            &quoted,
            screen,
            out,
        )?;
    }
    if let Some(children) = &content.children {
        write_children(children, statuses, screen, out)?;
    }
    if let Some(deadline) = &content.deadline {
        write_property(
            deadline.until_comment.as_deref(),
            Property::Until,
            &deadline.until,
            screen,
            out,
        )?;
        if let Some(promote) = &deadline.promote {
            write_property(
                promote.comment.as_deref(),
                Property::Promote,
                promote.mode.name(),
                screen,
                out,
            )?;
        }
    }
    writeln!(out)
}

/// The narrowest a description is wrapped to, however small the terminal.
const MIN_TEXT_WIDTH: usize = 20;

/// The guides that hang each child under `children:`, the way a file
/// explorer draws a directory's entries.
const GUIDES: GlyphPalette = GlyphPalette {
    middle_item: "├",
    last_item: "└",
    item_indent: "─ ",
    middle_skip: "│",
    last_skip: " ",
    skip_indent: "  ",
};

/// The description the decision judges, wrapped to the terminal in the
/// frame that marks it as the subject.
fn write_description(
    text: &str,
    group: Group,
    screen: Screen,
    out: &mut impl Write,
) -> io::Result<()> {
    // Two columns of indent, then a border and a space on either side.
    let lines = wrap(text, screen.width.saturating_sub(2 + 4));
    for line in frame(&lines, None, &[], Frame::Subject(group), screen) {
        writeln!(out, "  {line}")?;
    }
    Ok(())
}

/// The two frames a target is drawn with. The description under judgement
/// gets a double one in the colour of its review state so it stands out from
/// the children around it, which get a light, dim one; the double line
/// carries the difference when colour is off.
#[derive(Clone, Copy)]
enum Frame {
    Subject(Group),
    Child,
}

impl Frame {
    /// The corners clockwise from the top left, the horizontal and the
    /// vertical edge, then the two ends of the rule above a footer.
    const fn glyphs(self) -> [char; 8] {
        match self {
            Frame::Subject(_) => ['╔', '╗', '╝', '╚', '═', '║', '╟', '╢'],
            Frame::Child => ['╭', '╮', '╯', '╰', '─', '│', '├', '┤'],
        }
    }

    /// The line of the rule above a footer: always a light one, so a double
    /// frame is not cut in two.
    const RULE: char = '─';

    fn style(self, screen: Screen) -> Style {
        match self {
            Frame::Subject(group) => screen.group(group).bold(),
            Frame::Child => screen.dim(),
        }
    }
}

/// `lines` in a frame as wide as the longest of them, with `title` — already
/// styled — set into the top edge when there is one. A `footer` that is not
/// empty follows the lines under a rule across the frame.
fn frame(
    lines: &[String],
    title: Option<&str>,
    footer: &[String],
    kind: Frame,
    screen: Screen,
) -> Vec<String> {
    let [
        top_left,
        top_right,
        bottom_right,
        bottom_left,
        horizontal,
        vertical,
        rule_left,
        rule_right,
    ] = kind.glyphs();
    let edge = |length: usize| horizontal.to_string().repeat(length);
    let title_width = title.map_or(0, measure_text_width);
    let inner = lines
        .iter()
        .chain(footer)
        .map(|line| measure_text_width(line))
        // A titled edge needs a dash on either side of ` title `.
        .chain(title.map(|_| title_width + 2))
        .max()
        .unwrap_or(0);
    let border = kind.style(screen);
    let top = match title {
        Some(title) => format!(
            "{} {title} {}",
            border.apply_to(format!("{top_left}{horizontal}")),
            border.apply_to(format!("{}{top_right}", edge(inner - title_width - 1)))
        ),
        None => border
            .apply_to(format!("{top_left}{}{top_right}", edge(inner + 2)))
            .to_string(),
    };
    let row = |line: &String| {
        format!(
            "{} {} {}",
            border.apply_to(vertical),
            pad_str(line, inner, Alignment::Left, None),
            border.apply_to(vertical)
        )
    };
    let mut framed = vec![top];
    framed.extend(lines.iter().map(row));
    if !footer.is_empty() {
        let rule = Frame::RULE.to_string().repeat(inner + 2);
        framed.push(
            border
                .apply_to(format!("{rule_left}{rule}{rule_right}"))
                .to_string(),
        );
        framed.extend(footer.iter().map(row));
    }
    framed.push(
        border
            .apply_to(format!("{bottom_left}{}{bottom_right}", edge(inner + 2)))
            .to_string(),
    );
    framed
}

/// The `children:` property as a tree: each direct child hangs from a guide
/// with the comment on its entry and then its description, wrapped to the
/// terminal in a frame that carries its id in the top edge.
///
/// The top edge also carries the child's status, after the id, and what the
/// status needs said beyond its name stands under a rule at the foot of the
/// frame: every reason a blamed child fails on, or how many blamed nodes lie
/// below one that is not established. An established child gets no foot.
fn write_children(
    children: &ChildrenContent,
    statuses: &Statuses<'_>,
    screen: Screen,
    out: &mut impl Write,
) -> io::Result<()> {
    write_comment(children.comment.as_deref(), screen, out)?;

    // Two columns of indent, three of guide, then the frame's own four.
    let width = screen.width.saturating_sub(2 + 3 + 4);
    let entries = children.entries.iter().map(|child| {
        let description = child.description.as_deref().unwrap_or("(not defined)");
        let mut lines = comment_lines(child.comment.as_deref(), screen);
        let id = screen.id().apply_to(sanitize(&child.id)).to_string();
        let standing = statuses.get(child.id.as_str());
        let title = match standing {
            Some(node) => format!(
                "{id} {} {}",
                screen.dim().apply_to("──"),
                screen
                    .status(&node.status)
                    .apply_to(status_name(&node.status))
            ),
            None => id,
        };
        let footer = standing.map_or_else(Vec::new, |node| status_footer(node, width));
        lines.extend(frame(
            &wrap(description, width),
            Some(&title),
            &footer,
            Frame::Child,
            screen,
        ));
        Tree::new(lines.join("\n")).with_multiline(true)
    });
    let tree = Tree::new(screen.key().apply_to("children:").to_string())
        .with_glyphs(GUIDES)
        .with_leaves(entries);
    for line in tree.to_string().lines() {
        writeln!(out, "  {line}")?;
    }
    Ok(())
}

/// One `key: value` property line under the comment attached to it.
fn write_property(
    comment: Option<&str>,
    property: Property,
    value: &str,
    screen: Screen,
    out: &mut impl Write,
) -> io::Result<()> {
    write_comment(comment, screen, out)?;
    writeln!(
        out,
        "  {} {}",
        screen.key().apply_to(format!("{}:", property.name())),
        sanitize(value)
    )
}

/// A comment written above the property it is attached to.
fn write_comment(text: Option<&str>, screen: Screen, out: &mut impl Write) -> io::Result<()> {
    for line in comment_lines(text, screen) {
        writeln!(out, "  {line}")?;
    }
    Ok(())
}

/// A comment as `//` lines, `//` alone for an empty line.
fn comment_lines(text: Option<&str>, screen: Screen) -> Vec<String> {
    let style = screen.dim();
    text.into_iter()
        .flat_map(str::lines)
        .map(|line| {
            let line = sanitize(line);
            let comment = if line.is_empty() {
                "//".to_owned()
            } else {
                format!("// {line}")
            };
            style.apply_to(comment).to_string()
        })
        .collect()
}

/// `text` made safe for the terminal and wrapped to `width` columns, never
/// narrower than [`MIN_TEXT_WIDTH`]. The line breaks of the source are kept.
/// A tab has no width a frame could be aligned to, so it becomes spaces.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let clean: Vec<String> = text
        .lines()
        .map(|line| sanitize(line).replace('\t', "    "))
        .collect();
    textwrap::wrap(&clean.join("\n"), width.max(MIN_TEXT_WIDTH))
        .into_iter()
        .map(std::borrow::Cow::into_owned)
        .collect()
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

/// A status as `inka eval` and the web UI spell it.
const fn status_name(status: &Status) -> &'static str {
    match status {
        Status::Established => "established",
        Status::Blamed { .. } => "blamed",
        Status::Unestablished => "not established",
    }
}

/// What a child's status says beyond its name: each reason it is blamed, in
/// the words of `inka eval`, or how many of the nodes below it are. Which
/// nodes those are is read in the child's own local view.
fn status_detail(node: &NodeData) -> Vec<String> {
    match &node.status {
        Status::Established => Vec::new(),
        Status::Blamed { reasons } => reasons.iter().map(ToString::to_string).collect(),
        Status::Unestablished => vec![match node.blamed_below {
            1 => "1 node below it is blamed".to_owned(),
            count => format!("{count} nodes below it are blamed"),
        }],
    }
}

/// The foot of a child's frame: its [`status_detail`] wrapped to `width`.
/// Several reasons are set apart as a list, each wrapped under its own dash,
/// so where one ends and the next begins does not depend on the wrapping.
fn status_footer(node: &NodeData, width: usize) -> Vec<String> {
    let details = status_detail(node);
    if let [detail] = details.as_slice() {
        return wrap(detail, width);
    }
    details
        .iter()
        .flat_map(|detail| {
            wrap(detail, width.saturating_sub(2))
                .into_iter()
                .enumerate()
                .map(|(index, line)| {
                    let lead = if index == 0 { "- " } else { "  " };
                    format!("{lead}{line}")
                })
        })
        .collect()
}

/// What a state says beyond its name: who approved an earlier version, or
/// who declined. An unapproved target has nothing to add.
fn state_detail(state: &QueueState) -> Option<String> {
    match state {
        QueueState::Missing => None,
        QueueState::Stale { approvers } => Some(format!(
            "{} approved an earlier version",
            sanitize(&approvers.join(", "))
        )),
        QueueState::Declined { by } => Some(format!("by {}", sanitize(&by.join(", ")))),
    }
}

fn blocked_line(reason: &BlockedReason) -> String {
    match reason {
        BlockedReason::MissingEvidence { target } => {
            format!("no imported test evidence provides `{}`", sanitize(target))
        }
        BlockedReason::VeriqNotEvaluated => "veriq evidence is not evaluated yet".to_owned(),
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

    use inka_core::hash::LocalView;
    use inka_core::view::{Evidence, EvidenceKind, NodeComments};

    /// An uncoloured screen of a fixed width.
    const PLAIN: Screen = Screen {
        color: false,
        width: 60,
    };

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
            screen: PLAIN,
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

        // Assert: the queue runs leaves first — leaf, branch, root.
        assert!(transcript.contains("[1/3] fiat:leaf"));
        assert!(transcript.contains("[2/3] structure:branch"));
        assert!(transcript.contains("[3/3] objective:root"));
        assert!(transcript.contains("the branch claim"));
        assert!(transcript.contains("children:"));
        assert!(transcript.contains("decisions recorded: 3; targets remaining: 0"));
        assert_eq!(record_lines(dir.path()).len(), 3);
    }

    #[test]
    fn a_decline_demands_a_comment_until_one_is_given() {
        // Arrange & Act: decline the first target, first with a blank comment.
        let dir = scenario_project();
        let transcript = run_interactive(dir.path(), "d\n\nthe leaf claim is wrong\nq\n");

        // Assert: the blank was refused, the decline recorded, and quit left
        // the remaining targets untouched.
        assert!(transcript.contains("a decline must say why"));
        assert!(transcript.contains("recorded decline of fiat:leaf"));
        assert!(transcript.contains("decisions recorded: 1; targets remaining: 2"));
        let lines = record_lines(dir.path());
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("\"label\":\"decline\""));
        assert!(lines[0].contains("the leaf claim is wrong"));
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
  ╔══════════════════╗
  ║ the branch claim ║
  ╚══════════════════╝
  // one entry per hazard
  children:
  └─ // the only hazard so far
     ╭─ leaf ── blamed ───────────────────────╮
     │ checked in CI                          │
     ├────────────────────────────────────────┤
     │ no approval covers the current content │
     ╰────────────────────────────────────────╯
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
        // Arrange: a veriq bind is never queued, so the target is rendered
        // directly.
        let node = ViewNode {
            id: "checked".to_owned(),
            mode: Mode::Bind,
            namespace: "arg".to_owned(),
            description: "checked by a solver".to_owned(),
            tags: Vec::new(),
            evidence: Some(Evidence {
                kind: EvidenceKind::Veriq,
                target: "aocs.check".to_owned(),
            }),
            deadline: None,
            children: Vec::new(),
            comments: NodeComments {
                properties: BTreeMap::from([(Property::Veriq, "the shipped check".to_owned())]),
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
        write_target(&node, &item, 1, 1, &view, &Statuses::new(), PLAIN, &mut out)
            .expect("writing to a vec succeeds");

        // Assert
        let text = String::from_utf8(out).expect("UTF-8");
        assert!(
            text.ends_with(
                "  ║ checked by a solver ║\n  ╚═════════════════════╝\n  \
                 // the shipped check\n  veriq: \"aocs.check\"\n\n"
            ),
            "{text}"
        );
    }

    #[test]
    fn a_target_header_names_the_state_and_fills_the_terminal_width() {
        // Arrange: approve everything, then edit the leaf so it is stale.
        let dir = scenario_project();
        run_interactive(dir.path(), "a\na\na\n");
        let argument = dir.path().join("argument/arg.inka");
        let text = fs::read_to_string(&argument).expect("the argument reads");
        fs::write(&argument, text.replace("taken as given", "taken, edited"))
            .expect("the argument writes");

        // Act
        let transcript = run_interactive(dir.path(), "s\ns\n");

        // Assert: every header is exactly as wide as the screen, and a stale
        // target says who approved the earlier version under it.
        let headers: Vec<&str> = transcript
            .lines()
            .filter(|line| line.starts_with("── ["))
            .collect();
        assert_eq!(headers.len(), 2, "{transcript}");
        for header in &headers {
            assert_eq!(measure_text_width(header), PLAIN.width, "{header}");
        }
        assert!(
            transcript.contains(
                "── [1/2] fiat:leaf ── stale ────────────────────────────────\n   \
                 alice approved an earlier version\n\n  \
                 ╔═══════════════╗\n  ║ taken, edited ║\n  ╚═══════════════╝\n\n\
                 [a]pprove  [d]ecline  [s]kip  [q]uit > "
            ),
            "{transcript}"
        );
    }

    #[test]
    fn a_header_wider_than_the_terminal_gets_no_rule_to_wrap() {
        // Arrange: a screen narrower than the header itself.
        let node = ViewNode {
            id: "leaf".to_owned(),
            mode: Mode::Fiat,
            namespace: "arg".to_owned(),
            description: "taken as given".to_owned(),
            tags: Vec::new(),
            evidence: None,
            deadline: None,
            children: Vec::new(),
            comments: NodeComments::default(),
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
        let narrow = Screen {
            color: false,
            width: 10,
        };
        let mut out = Vec::new();

        // Act
        write_target(
            &node,
            &item,
            1,
            1,
            &view,
            &Statuses::new(),
            narrow,
            &mut out,
        )
        .expect("writing to a vec succeeds");
        let text = String::from_utf8(out).expect("UTF-8");

        // Assert
        assert!(
            text.starts_with("\n── [1/1] fiat:leaf ── unapproved \n"),
            "{text:?}"
        );
    }

    #[test]
    fn colour_marks_the_state_and_leaves_the_text_unchanged() {
        // Arrange: a declined leaf, so the prelude and a target both render.
        let dir = scenario_project();
        run_interactive(dir.path(), "d\nnot so\nq\n");
        let render = |color: bool| {
            let request = Request {
                approver: "alice".to_owned(),
                key_id: None,
                action: Action::Interactive,
            };
            let mut input = Cursor::new(b"a\nq\n".to_vec());
            let mut now = || -> Timestamp {
                "2026-11-01T00:00:00Z"
                    .parse()
                    .expect("the fixed instant parses")
            };
            let session = Session {
                input: &mut input,
                input_is_tty: true,
                now: &mut now,
                screen: Screen { color, ..PLAIN },
            };
            let mut out = Vec::new();
            run(
                dir.path(),
                &request,
                session,
                &mut out,
                &mut Vec::new(),
                false,
            )
            .expect("writing to vecs succeeds");
            String::from_utf8(out).expect("the transcript is UTF-8")
        };

        // Act: the same session twice, on a throwaway copy of the records.
        let records = dir.path().join("approvals/alice.jsonl");
        let before = fs::read_to_string(&records).expect("the records read");
        let coloured = render(true);
        fs::write(&records, before).expect("the records restore");
        let plain = render(false);

        // Assert: red for the decline, yellow for the unapproved target,
        // green for the approval, cyan for a property key — and nothing but
        // styling differs from the plain rendering.
        assert!(
            coloured.contains("\u{1b}[31m\u{1b}[1mDeclined (1)\u{1b}[0m"),
            "{coloured:?}"
        );
        assert!(
            coloured.contains("\u{1b}[33munapproved\u{1b}[0m"),
            "{coloured:?}"
        );
        assert!(
            coloured.contains("\u{1b}[32mrecorded approve\u{1b}[0m"),
            "{coloured:?}"
        );
        assert!(
            coloured.contains("\u{1b}[36mchildren:\u{1b}[0m"),
            "{coloured:?}"
        );
        assert!(
            coloured.contains("\u{1b}[34m\u{1b}[1mleaf\u{1b}[0m"),
            "{coloured:?}"
        );
        assert!(
            coloured.contains("\u{1b}[33m\u{1b}[1m╔════════════════╗\u{1b}[0m"),
            "{coloured:?}"
        );
        assert!(!plain.contains('\u{1b}'), "{plain:?}");
        assert_eq!(console::strip_ansi_codes(&coloured), plain);
        assert!(
            plain.starts_with("Declined (1)\n  fiat:leaf  by alice (edit or retract to resolve)\n"),
            "{plain}"
        );
    }

    #[test]
    fn descriptions_wrap_to_the_terminal_and_children_hang_from_guides() {
        // Arrange: descriptions longer than the 60-column screen, one of
        // them Japanese, which has no spaces to break at.
        let dir = project_with(
            "objective root {\n  ```\n  the root claim is long enough that it cannot fit on \
             one line of a sixty column terminal\n  ```\n  children: first, second\n}\n\n\
             fiat first {\n  ```\n  the first child also says rather more than a single \
             line of the terminal can hold\n  ```\n}\n\n\
             fiat second {\n  ```\n  評価はコミット済みの入力だけから再現でき、\
             途中で環境の状態を一切読まないことを前提とする。\n  ```\n}\n",
        );

        // Act
        let transcript = run_interactive(dir.path(), "s\ns\ns\n");

        // Assert: the frame closes on every line, the guides run down to the
        // last child, and nothing is wider than the screen.
        assert!(
            transcript.contains(
                "\
  ╔═════════════════════════════════════════════════════╗
  ║ the root claim is long enough that it cannot fit on ║
  ║ one line of a sixty column terminal                 ║
  ╚═════════════════════════════════════════════════════╝
  children:
  ├─ ╭─ first ── blamed ───────────────────────────────────╮
  │  │ the first child also says rather more than a single │
  │  │ line of the terminal can hold                       │
  │  ├─────────────────────────────────────────────────────┤
  │  │ no approval covers the current content              │
  │  ╰─────────────────────────────────────────────────────╯
  └─ ╭─ second ── blamed ─────────────────────────────────╮
     │ 評価はコミット済みの入力だけから再現でき、途中で環 │
     │ 境の状態を一切読まないことを前提とする。           │
     ├────────────────────────────────────────────────────┤
     │ no approval covers the current content             │
     ╰────────────────────────────────────────────────────╯
"
            ),
            "{transcript}"
        );
        for line in transcript.lines().filter(|line| line.starts_with("  ")) {
            assert!(measure_text_width(line) <= PLAIN.width, "{line}");
            assert_eq!(line.trim_end(), line, "no trailing blanks: {line:?}");
        }
    }

    #[test]
    fn a_child_frame_names_its_status_and_every_reason_it_is_blamed() {
        // Arrange: a root over children that fail in every way there is,
        // three of them blocked and so never offered as targets themselves.
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/unestablished");

        // Act: skip all five targets; nothing is written to the fixture.
        let transcript = run_interactive(&dir, "s\ns\ns\ns\ns\n");

        // Assert: the status follows the id in the top edge and the reasons
        // stand under a rule, in the words of `inka eval`.
        let root = transcript
            .split("objective:root")
            .nth(1)
            .expect("the root is the last target");
        assert!(
            root.contains(
                "\
  ├─ ╭─ tested ── blamed ─────────────────────────╮
  │  │ checked in CI                              │
  │  ├────────────────────────────────────────────┤
  │  │ no imported test evidence provides `tests/ │
  │  │ hash_props.rs`                             │
  │  ╰────────────────────────────────────────────╯
"
            ),
            "{transcript}"
        );
        assert!(
            root.contains(
                "\
  ├─ ╭─ partial ── blamed ──────────────────────────────╮
  │  │ every platform is exercised                      │
  │  ├──────────────────────────────────────────────────┤
  │  │ - no approval covers the current content         │
  │  │ - required executions were skipped, cancelled or │
  │  │   missing: macos, windows                        │
  │  ╰──────────────────────────────────────────────────╯
"
            ),
            "{transcript}"
        );
        assert!(
            root.contains("declined by sato: the margin calculation omits"),
            "{transcript}"
        );
        for line in root.lines().filter(|line| line.starts_with("  ")) {
            assert!(measure_text_width(line) <= PLAIN.width, "{line}");
        }
    }

    #[test]
    fn a_child_approved_in_the_session_reads_as_established_under_its_parent() {
        // Arrange
        let dir = scenario_project();

        // Act: approve the leaf, then look at its parent.
        let transcript = run_interactive(dir.path(), "a\ns\ns\n");

        // Assert: an established child gets its status and no foot.
        assert!(
            transcript.contains(
                "\
  children:
  └─ ╭─ leaf ── established ─╮
     │ taken as given        │
     ╰───────────────────────╯
"
            ),
            "{transcript}"
        );
    }

    #[test]
    fn a_child_failing_only_through_what_is_below_it_says_how_many_are_blamed() {
        // Arrange
        let dir = scenario_project();

        // Act: leave the leaf, approve the branch, then look at the root.
        let transcript = run_interactive(dir.path(), "s\na\ns\n");

        // Assert: the branch has no cause of its own, so it is not blamed;
        // which node below it is, its own local view says.
        assert!(
            transcript.contains(
                "\
  children:
  └─ ╭─ branch ── not established ─╮
     │ the branch claim            │
     ├─────────────────────────────┤
     │ 1 node below it is blamed   │
     ╰─────────────────────────────╯
"
            ),
            "{transcript}"
        );
    }

    #[test]
    fn colour_repeats_a_child_status_and_never_carries_it_alone() {
        // Arrange
        let dir = scenario_project();
        let request = Request {
            approver: "alice".to_owned(),
            key_id: None,
            action: Action::Interactive,
        };
        let mut input = Cursor::new(b"a\ns\ns\n".to_vec());
        let mut now = || -> Timestamp {
            "2026-11-01T00:00:00Z"
                .parse()
                .expect("the fixed instant parses")
        };
        let session = Session {
            input: &mut input,
            input_is_tty: true,
            now: &mut now,
            screen: Screen {
                color: true,
                width: 60,
            },
        };
        let mut out = Vec::new();

        // Act
        run(
            dir.path(),
            &request,
            session,
            &mut out,
            &mut Vec::new(),
            false,
        )
        .expect("writing to vecs succeeds");
        let coloured = String::from_utf8(out).expect("UTF-8");

        // Assert: green for established, red for blamed, the word in both.
        assert!(
            coloured.contains("\u{1b}[32mestablished\u{1b}[0m"),
            "{coloured:?}"
        );
        assert!(
            coloured.contains("\u{1b}[31mblamed\u{1b}[0m"),
            "{coloured:?}"
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
        assert!(
            transcript.contains("  ║ taken    as given ║\n"),
            "{transcript:?}"
        );
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
