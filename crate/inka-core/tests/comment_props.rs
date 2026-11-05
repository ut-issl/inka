//! Property tests for attached comments.
//!
//! The spec promises that a comment is never a syntax error: every `//`
//! outside a fence attaches somewhere. These check that promise against
//! comments placed on arbitrary lines of a real argument, own-line and
//! trailing alike, and that a comment never changes anything but comments.

use std::fmt::Write as _;

use inka_core::ast::Block;
use inka_core::lexer::Lexer;
use inka_core::parser::{Parse, parse};
use inka_core::source::SourceMap;
use proptest::prelude::*;

/// An argument that exercises every anchor: block headers with tags, a
/// description, a multi-line `children:` list, an evidence reference, and the
/// full deadline group.
const ARGUMENT: &str = "\
objective root #mission {
  ```
  the satellite survives
  ```
  children: branch,
    leaf
  until: M3
  promote: structure
}

structure branch {
  ```
  power holds // not a comment: this is description text
  ```
  children: leaf, checked
}

bind checked {
  ```
  checked in CI
  ```
  test: \"tests/x.rs::y\"
}

fiat leaf {
  ```
  taken as given
  ```
  until: FM
  promote: bind
}
";

fn parse_source(source: &str) -> Parse {
    let mut map = SourceMap::new();
    let file_id = map.add_file("test.inka".into(), source.to_owned());
    let tokens = Lexer::new(file_id, source)
        .lex()
        .expect("a comment never breaks lexing");
    parse(file_id, &tokens)
}

/// Indices of the lines of `source` that lie outside every fence, fence
/// markers excluded: the lines where `//` starts a comment.
fn lines_outside_fences(source: &str) -> Vec<usize> {
    let mut inside = false;
    let mut outside = Vec::new();
    for (index, line) in source.lines().enumerate() {
        if line.trim_start().starts_with("```") {
            inside = !inside;
        } else if !inside {
            outside.push(index);
        }
    }
    outside
}

/// Where a generated comment goes: before a line, or at the end of it.
#[derive(Debug, Clone, Copy)]
enum Placement {
    OwnLineBefore(usize),
    TrailingAfter(usize),
}

fn placements() -> impl Strategy<Value = Vec<Placement>> {
    let lines = lines_outside_fences(ARGUMENT);
    let placement = (prop::sample::select(lines), any::<bool>()).prop_map(|(line, own_line)| {
        if own_line {
            Placement::OwnLineBefore(line)
        } else {
            Placement::TrailingAfter(line)
        }
    });
    prop::collection::vec(placement, 1..8)
}

/// `ARGUMENT` with a numbered comment at each placement, and the number of
/// comment lines that makes.
///
/// A line takes at most one trailing comment: everything after the first `//`
/// is that comment's text, so a second one would not be a second comment.
fn with_comments(placements: &[Placement]) -> (String, usize) {
    let mut out = String::new();
    let mut count = 0;
    for (index, line) in ARGUMENT.lines().enumerate() {
        for (number, placement) in placements.iter().enumerate() {
            if matches!(placement, Placement::OwnLineBefore(at) if *at == index) {
                writeln!(out, "// note {number}").expect("writing to a String");
                count += 1;
            }
        }
        out.push_str(line);
        let trailing = placements.iter().position(
            |placement| matches!(placement, Placement::TrailingAfter(at) if *at == index),
        );
        if let Some(number) = trailing {
            write!(out, " // trailing {number}").expect("writing to a String");
            count += 1;
        }
        out.push('\n');
    }
    (out, count)
}

/// Everything about a block except where it is and what is commented on it.
type Shape = (String, String, Vec<String>, Vec<String>, Option<String>);

fn shape(block: &Block) -> Shape {
    let common = block.common();
    (
        common.id.value.clone(),
        common.description.raw.clone(),
        common.tags.iter().map(|tag| tag.value.clone()).collect(),
        block
            .children()
            .iter()
            .map(|child| child.id.value.clone())
            .collect(),
        block
            .deadline()
            .map(|deadline| deadline.until.value.clone()),
    )
}

/// How many comment lines the parse attached, file comment included.
fn attached_lines(parse: &Parse) -> usize {
    let mut comments: Vec<_> = parse.comment.iter().collect();
    for block in &parse.blocks {
        comments.extend(block.common().comment.iter());
        comments.extend(block.children().iter().filter_map(|c| c.comment.as_ref()));
        comments.extend(block.deadline().and_then(|d| d.comment.as_ref()));
        match block {
            Block::Objective(b) => {
                comments.extend(b.children_comment.iter());
                let promote = b.deadline.as_ref().and_then(|d| d.promote.as_ref());
                comments.extend(promote.and_then(|p| p.comment.as_ref()));
            }
            Block::Structure(b) => comments.extend(b.children_comment.iter()),
            Block::Bind(b) => comments.extend(b.source_comment.iter()),
            Block::Fiat(b) => {
                let promote = b.deadline.as_ref().and_then(|d| d.promote.as_ref());
                comments.extend(promote.and_then(|p| p.comment.as_ref()));
            }
        }
    }
    comments.iter().map(|comment| comment.lines().len()).sum()
}

proptest! {
    /// Comments anywhere outside a fence parse cleanly, change no block, and
    /// every one of them is attached somewhere — none is lost.
    #[test]
    fn a_comment_is_never_an_error_and_never_lost(placements in placements()) {
        let (commented, count) = with_comments(&placements);

        let plain = parse_source(ARGUMENT);
        let parsed = parse_source(&commented);

        prop_assert!(parsed.errors.is_empty(), "{:?}\n{commented}", parsed.errors);
        let shapes = |parse: &Parse| parse.blocks.iter().map(shape).collect::<Vec<_>>();
        prop_assert_eq!(shapes(&parsed), shapes(&plain));
        prop_assert_eq!(attached_lines(&parsed), count);
    }
}
