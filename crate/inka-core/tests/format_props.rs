//! Property tests for the formatter.
//!
//! `formatting.md` promises three things: the formatted file parses to the
//! same tree apart from positions and fence indentation, no contract (and so
//! no view hash) changes, and formatting a formatted file changes nothing.
//! These check all three against arguments written in arbitrary layouts —
//! any indentation, line breaks anywhere tokens allow them, properties in any
//! order, comments own-line and trailing on any token, CRLF or CR endings —
//! with values that make some fences unsafe to re-indent.

use std::collections::HashMap;

use inka_core::ast::{Block, Comment};
use inka_core::format::format_source;
use inka_core::interpolate::interpolate;
use inka_core::lexer::Lexer;
use inka_core::normalize;
use inka_core::parser::{Parse, parse};
use inka_core::source::SourceMap;
use proptest::prelude::*;

/// The values of every generated project. `multi` spans lines, so a fence
/// interpolating it may not be safe to re-indent; `padded` and `blank` carry
/// whitespace of their own, and `accent` is not ASCII.
fn values() -> HashMap<String, String> {
    [
        ("v", "5 m"),
        ("multi", "- one\n  - two\n- three"),
        ("padded", "  padded"),
        ("blank", "   "),
        ("accent", "café"),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value.to_owned()))
    .collect()
}

/// Lines a description is built from: plain text, nested text, single- and
/// multi-line interpolation, values with whitespace of their own, an
/// undefined value, blank lines, tabs, non-ASCII and non-NFC text, and text
/// that would be syntax outside a fence.
const DESCRIPTION_LINES: &[&str] = &[
    "the claim holds",
    "  - a nested item",
    "at most {$v}",
    "{$multi}",
    "  then {$multi}",
    "uses {$undefined}",
    "",
    "text // not a comment",
    "{ braces } and `code`",
    "\tafter a tab",
    "café résumé — 日本語",
    "cafe\u{301} written decomposed",
    "{$padded} first",
    "{$blank}",
    "  {$accent}",
];

const COMMENT_TEXTS: &[&str] = &[
    "note",
    "  indented note",
    "",
    "why: {$v} // inner",
    "\u{65e5}\u{672c}\u{8a9e}\u{306e}\u{30b3}\u{30e1}\u{30f3}\u{30c8}",
    "cafe\u{301}",
];

/// Info strings, including one that starts with a backtick and so needs the
/// space that separates it from the fence.
const INFO_STRINGS: &[&str] = &["md", "rust ignore", "`x"];

#[derive(Debug, Clone)]
enum Kind {
    Objective { promote: bool },
    Structure,
    Bind { veriq: bool },
    Fiat { promote: Option<&'static str> },
}

#[derive(Debug, Clone)]
struct GenBlock {
    kind: Kind,
    tags: Vec<&'static str>,
    description: Vec<usize>,
    fence_length: usize,
    info: Option<&'static str>,
    children: Vec<&'static str>,
    until: bool,
}

fn kind() -> impl Strategy<Value = Kind> {
    prop_oneof![
        any::<bool>().prop_map(|promote| Kind::Objective { promote }),
        Just(Kind::Structure),
        any::<bool>().prop_map(|veriq| Kind::Bind { veriq }),
        prop::option::of(prop::sample::select(vec!["structure", "bind"]))
            .prop_map(|promote| Kind::Fiat { promote }),
    ]
}

fn block() -> impl Strategy<Value = GenBlock> {
    (
        kind(),
        prop::sample::subsequence(vec!["mission", "aocs", "power"], 0..=2),
        prop::collection::vec(0..DESCRIPTION_LINES.len(), 1..4),
        3usize..=4,
        prop::option::of(prop::sample::select(INFO_STRINGS)),
        prop::collection::vec(prop::sample::select(vec!["a", "b", "ns::c", "d"]), 1..4),
        any::<bool>(),
    )
        .prop_map(
            |(kind, tags, description, fence_length, info, children, until)| GenBlock {
                // A promotion is only valid with a deadline.
                until: until
                    || matches!(
                        kind,
                        Kind::Objective { promote: true } | Kind::Fiat { promote: Some(_) }
                    ),
                kind,
                tags,
                description,
                fence_length,
                info,
                children,
            },
        )
}

/// One property of a block as the writer emits it.
enum Property {
    Description,
    Tokens(Vec<String>),
}

/// Writes generated blocks as source text, taking every layout decision from
/// `dice`: where lines break, how deep they are indented, and where comments
/// go. Only layout is random here; what the blocks say is fixed by the
/// generated blocks.
struct Writer<'a> {
    out: String,
    newline: &'static str,
    dice: &'a [u8],
    roll: usize,
    at_line_start: bool,
    comments: usize,
}

impl Writer<'_> {
    fn die(&mut self) -> u8 {
        let value = self.dice[self.roll % self.dice.len()];
        self.roll += 1;
        value
    }

    fn newline(&mut self) {
        self.out.push_str(self.newline);
        if self.die().is_multiple_of(5) {
            self.out.push_str(self.newline);
        }
        self.at_line_start = true;
    }

    fn indent(&mut self) {
        let width = usize::from(self.die() % 7);
        self.out.push_str(&" ".repeat(width));
    }

    /// Writes one token, maybe on a new line, maybe with a comment on its
    /// own line before it or trailing after it.
    fn token(&mut self, text: &str) {
        let die = self.die();
        if die.is_multiple_of(7) {
            self.own_line_comment();
        } else if !self.at_line_start && die.is_multiple_of(3) {
            self.newline();
        }
        if self.at_line_start {
            self.indent();
        } else {
            self.out.push(' ');
        }
        self.out.push_str(text);
        self.at_line_start = false;
        if self.die().is_multiple_of(6) {
            let text = self.comment_text();
            self.out.push_str(" //");
            self.out.push_str(&text);
            self.newline();
        }
    }

    fn own_line_comment(&mut self) {
        if !self.at_line_start {
            self.newline();
        }
        self.indent();
        let text = self.comment_text();
        self.out.push_str("//");
        self.out.push_str(&text);
        self.newline();
    }

    fn comment_text(&mut self) -> String {
        let text = COMMENT_TEXTS[usize::from(self.die()) % COMMENT_TEXTS.len()];
        self.comments += 1;
        if text.is_empty() {
            String::new()
        } else {
            format!(" {text} {}", self.comments)
        }
    }

    fn fence(&mut self, block: &GenBlock) {
        if !self.at_line_start {
            self.newline();
        }
        let fence = "`".repeat(block.fence_length);
        let base = " ".repeat(usize::from(self.die() % 7));
        self.indent();
        self.out.push_str(&fence);
        if let Some(info) = block.info {
            self.out.push(' ');
            self.out.push_str(info);
        }
        self.out.push_str(self.newline);
        for &line in &block.description {
            let text = DESCRIPTION_LINES[line];
            if !text.is_empty() {
                self.out.push_str(&base);
                self.out.push_str(text);
            }
            if self.die().is_multiple_of(4) {
                self.out.push_str("  ");
            }
            self.out.push_str(self.newline);
        }
        self.indent();
        self.out.push_str(&fence);
        self.newline();
    }

    fn block(&mut self, block: &GenBlock) {
        let keyword = match block.kind {
            Kind::Objective { .. } => "objective",
            Kind::Structure => "structure",
            Kind::Bind { .. } => "bind",
            Kind::Fiat { .. } => "fiat",
        };
        self.token(keyword);
        let id = format!("b{}", self.roll);
        self.token(&id);
        for tag in &block.tags {
            self.token(&format!("#{tag}"));
        }
        self.token("{");

        // Properties in an order the dice pick; the description is one of
        // them, since the layout puts it first whatever the source order.
        let mut properties = properties(block);
        let rotation = usize::from(self.die()) % properties.len();
        properties.rotate_left(rotation);
        for property in properties {
            match property {
                Property::Description => self.fence(block),
                Property::Tokens(tokens) => {
                    for token in tokens {
                        self.token(&token);
                    }
                }
            }
        }
        self.token("}");
        self.newline();
    }
}

fn tokens(parts: &[&str]) -> Property {
    Property::Tokens(parts.iter().map(|part| (*part).to_owned()).collect())
}

fn properties(block: &GenBlock) -> Vec<Property> {
    let mut properties = vec![Property::Description];
    match block.kind {
        Kind::Objective { .. } | Kind::Structure => {
            let mut parts = vec!["children", ":"];
            for (index, child) in block.children.iter().enumerate() {
                if index > 0 {
                    parts.push(",");
                }
                parts.push(child);
            }
            properties.push(tokens(&parts));
        }
        Kind::Bind { veriq } => {
            let key = if veriq { "veriq" } else { "test" };
            properties.push(tokens(&[key, ":", "\"tests/x.rs::y\""]));
        }
        Kind::Fiat { .. } => {}
    }
    if block.until {
        properties.push(tokens(&["until", ":", "M3"]));
    }
    let promote = match block.kind {
        Kind::Objective { promote: true } => Some("structure"),
        Kind::Fiat { promote } => promote,
        _ => None,
    };
    if let Some(target) = promote {
        properties.push(tokens(&["promote", ":", target]));
    }
    properties
}

/// The comments a generated file has outside its blocks.
#[derive(Debug, Clone, Copy)]
struct Edges {
    /// A file comment at the top, followed by a blank line.
    header: bool,
    /// A comment after the last block, which joins the file comment.
    footer: bool,
}

fn source(blocks: &[GenBlock], dice: &[u8], newline: &'static str, edges: Edges) -> String {
    let mut writer = Writer {
        out: String::new(),
        newline,
        dice,
        roll: 0,
        at_line_start: true,
        comments: 0,
    };
    if edges.header {
        writer.out.push_str("// file header");
        writer.out.push_str(newline);
        writer.out.push_str(newline);
    }
    for block in blocks {
        writer.block(block);
    }
    if edges.footer {
        writer.out.push_str("// file footer");
        writer.out.push_str(newline);
    }
    writer.out
}

fn parse_text(text: &str) -> Parse {
    let mut map = SourceMap::new();
    let file_id = map.add_file("test.inka".into(), text.to_owned());
    let tokens = Lexer::new(file_id, text)
        .lex()
        .expect("generated text lexes");
    parse(file_id, &tokens)
}

fn format_text(text: &str) -> String {
    let mut map = SourceMap::new();
    let file_id = map.add_file("test.inka".into(), text.to_owned());
    format_source(file_id, text, &values()).expect("generated text parses")
}

/// A comment's lines, without positions.
fn lines(comment: Option<&Comment>) -> Vec<String> {
    comment
        .map(|comment| comment.lines().iter().map(|l| l.value.clone()).collect())
        .unwrap_or_default()
}

/// Fence content without its indentation: each line with its leading and
/// trailing whitespace removed, at any of the line breaks the lexer knows.
fn content(raw: &str) -> Vec<String> {
    raw.replace("\r\n", "\n")
        .replace('\r', "\n")
        .lines()
        .map(|line| line.trim().to_owned())
        .collect()
}

fn promotion<T: std::fmt::Debug>(
    promote: Option<&inka_core::ast::Promotion<T>>,
) -> Option<(String, Vec<String>)> {
    promote.map(|p| (format!("{:?}", p.target.value), lines(p.comment.as_ref())))
}

/// Everything a block says, without positions and fence indentation.
fn tree(block: &Block) -> String {
    let common = block.common();
    let description = &common.description;
    let children: Vec<(String, Vec<String>)> = block
        .children()
        .iter()
        .map(|entry| (entry.id.value.clone(), lines(entry.comment.as_ref())))
        .collect();
    let deadline = block
        .deadline()
        .map(|d| (d.until.value.clone(), lines(d.comment.as_ref())));
    let specific = match block {
        Block::Objective(b) => format!(
            "objective {:?} {:?}",
            lines(b.children_comment.as_ref()),
            promotion(b.deadline.as_ref().and_then(|d| d.promote.as_ref())),
        ),
        Block::Structure(b) => format!("structure {:?}", lines(b.children_comment.as_ref())),
        Block::Bind(b) => format!(
            "bind {:?} {:?}",
            b.source.value,
            lines(b.source_comment.as_ref())
        ),
        Block::Fiat(b) => format!(
            "fiat {:?}",
            promotion(b.deadline.as_ref().and_then(|d| d.promote.as_ref())),
        ),
    };
    format!(
        "{specific} id={} tags={:?} comment={:?} info={:?} fence={} content={:?} \
         children={children:?} deadline={deadline:?}",
        common.id.value,
        common.tags.iter().map(|t| &t.value).collect::<Vec<_>>(),
        lines(common.comment.as_ref()),
        description.info,
        description.fence_length,
        content(&description.raw),
    )
}

fn trees(parsed: &Parse) -> (Vec<String>, Vec<String>) {
    (
        lines(parsed.comment.as_ref()),
        parsed.blocks.iter().map(tree).collect(),
    )
}

/// Each block's contract as hashed: rendered and normalized, or `None` when
/// it names an undefined value.
fn contracts(parsed: &Parse) -> Vec<Option<String>> {
    let values = values();
    parsed
        .blocks
        .iter()
        .map(|block| {
            interpolate(&block.common().description, &values)
                .ok()
                .map(|rendered| normalize::description(rendered.rendered()))
        })
        .collect()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    #[test]
    fn formatting_keeps_the_tree_and_every_contract_and_is_idempotent(
        blocks in prop::collection::vec(block(), 1..4),
        dice in prop::collection::vec(any::<u8>(), 1..64),
        newline in prop::sample::select(vec!["\n", "\r\n", "\r"]),
        header in any::<bool>(),
        footer in any::<bool>(),
    ) {
        let original = source(&blocks, &dice, newline, Edges { header, footer });
        let before = parse_text(&original);
        prop_assert!(before.errors.is_empty(), "{:?}\n{original}", before.errors);

        let formatted = format_text(&original);
        let after = parse_text(&formatted);

        prop_assert!(after.errors.is_empty(), "{:?}\n{formatted}", after.errors);
        prop_assert_eq!(trees(&after), trees(&before), "\n{}\n---\n{}", original, formatted);
        prop_assert_eq!(contracts(&after), contracts(&before), "\n{}\n---\n{}", original, formatted);
        prop_assert_eq!(format_text(&formatted), formatted.clone(), "\n{}", formatted);
    }
}
