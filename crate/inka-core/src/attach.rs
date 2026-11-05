//! Comment attachment: which element a `//` comment belongs to.
//!
//! The parser records, for each block it reads, the token range every anchor
//! covers — the block itself, each property, each child entry — and asks
//! [`anchor_of`] where each comment of that block goes. Ranges are token
//! indices into the comment-free token stream, so blank lines and the exact
//! columns of a comment never matter, only which tokens surround it.
//!
//! The rules are the specification's (`spec.md` → Comments):
//!
//! - a *trailing* comment belongs to the innermost anchor covering the token
//!   before it on its line;
//! - an *own-line* comment belongs to the anchor its next token begins, or,
//!   when that token begins none, to the innermost anchor around it.
//!
//! An own-line comment around which no anchor lies is the caller's to place:
//! at the top level it goes to the next block or the file.
//!
//! [`CommentQueue`] hands the parser the comments of each region in source
//! order, and [`distribute`] sorts one block's comments into [`CommentSlots`].

use std::collections::VecDeque;

use crate::ast::Comment;
use crate::source::Spanned;

/// One element a comment can attach to, inside a single block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Anchor {
    Block,
    Children,
    /// The child entry at this position of the `children:` list.
    Child(usize),
    Source,
    Until,
    Promote,
}

/// The tokens one anchor covers, as inclusive token indices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct AnchorRange {
    pub anchor: Anchor,
    pub first: usize,
    pub last: usize,
}

impl AnchorRange {
    fn covers(&self, index: usize) -> bool {
        self.first <= index && index <= self.last
    }

    fn width(&self) -> usize {
        self.last - self.first
    }
}

/// Where a comment sits in the token stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Position {
    /// Index of the first token after the comment.
    pub next: usize,
    /// Whether a token precedes the comment on its line.
    pub trailing: bool,
}

/// The anchor a comment attaches to, or `None` when no anchor lies around an
/// own-line comment.
pub(crate) fn anchor_of(position: Position, anchors: &[AnchorRange]) -> Option<Anchor> {
    if position.trailing {
        // A trailing comment always has a token before it on its line.
        let previous = position.next.checked_sub(1)?;
        return innermost(anchors.iter().filter(|range| range.covers(previous)));
    }
    let begun = anchors.iter().filter(|range| range.first == position.next);
    innermost(begun).or_else(|| {
        innermost(
            anchors
                .iter()
                .filter(|range| range.first < position.next && position.next <= range.last),
        )
    })
}

fn innermost<'a>(ranges: impl Iterator<Item = &'a AnchorRange>) -> Option<Anchor> {
    ranges
        .min_by_key(|range| range.width())
        .map(|range| range.anchor)
}

/// A comment not yet attached, with what attachment needs to know.
pub(crate) struct PendingComment {
    pub line: Spanned<String>,
    pub position: Position,
    pub blank_line_after: bool,
}

/// The comments of a file not yet attached, in source order.
pub(crate) struct CommentQueue {
    pending: VecDeque<PendingComment>,
}

impl CommentQueue {
    pub fn new(comments: Vec<PendingComment>) -> Self {
        Self {
            pending: comments.into(),
        }
    }

    /// Takes the run of comment lines at the very top of the file when a
    /// blank line follows it: that run is the file's own comment.
    pub fn take_file_header(&mut self) -> Vec<Spanned<String>> {
        let run_end = self
            .pending
            .iter()
            .take_while(|comment| comment.position.next == 0)
            .position(|comment| comment.blank_line_after);
        let Some(run_end) = run_end else {
            return Vec::new();
        };
        self.pending
            .drain(..=run_end)
            .map(|comment| comment.line)
            .collect()
    }

    /// Takes every comment that lies before token `last + 1`: an own-line
    /// comment whose next token is at most `last`, or a trailing comment
    /// whose preceding token is.
    pub fn take_through(&mut self, last: usize) -> Vec<PendingComment> {
        let count = self
            .pending
            .iter()
            .take_while(|comment| {
                let Position { next, trailing } = comment.position;
                next <= last + usize::from(trailing)
            })
            .count();
        self.pending.drain(..count).collect()
    }

    /// Takes whatever is left.
    pub fn take_rest(&mut self) -> Vec<Spanned<String>> {
        self.pending.drain(..).map(|comment| comment.line).collect()
    }
}

/// The comment lines gathered for each anchor of one block.
#[derive(Default)]
pub(crate) struct CommentSlots {
    pub block: Vec<Spanned<String>>,
    pub children: Vec<Spanned<String>>,
    /// Indexed by child entry.
    child: Vec<Vec<Spanned<String>>>,
    pub source: Vec<Spanned<String>>,
    pub until: Vec<Spanned<String>>,
    pub promote: Vec<Spanned<String>>,
}

impl CommentSlots {
    fn slot(&mut self, anchor: Anchor) -> &mut Vec<Spanned<String>> {
        match anchor {
            Anchor::Block => &mut self.block,
            Anchor::Children => &mut self.children,
            Anchor::Child(index) => {
                if self.child.len() <= index {
                    self.child.resize_with(index + 1, Vec::new);
                }
                &mut self.child[index]
            }
            Anchor::Source => &mut self.source,
            Anchor::Until => &mut self.until,
            Anchor::Promote => &mut self.promote,
        }
    }

    /// The comment on the child entry at `index`, taken out of the slots.
    pub fn take_child(&mut self, index: usize) -> Option<Comment> {
        self.child
            .get_mut(index)
            .and_then(|lines| Comment::from_lines(std::mem::take(lines)))
    }
}

/// Sorts one block's comments into the slots of its anchors.
///
/// A comment no anchor surrounds lies before the block at the top level, so
/// it goes to the block: a top-level comment belongs to the next block.
pub(crate) fn distribute(comments: Vec<PendingComment>, anchors: &[AnchorRange]) -> CommentSlots {
    let mut slots = CommentSlots::default();
    for comment in comments {
        let anchor = anchor_of(comment.position, anchors).unwrap_or(Anchor::Block);
        slots.slot(anchor).push(comment.line);
    }
    slots
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `fiat` `x` `{` `children` `:` `a` `,` `b` `until` `:` `M3` `}` at
    /// token indices 0 through 11.
    fn anchors() -> Vec<AnchorRange> {
        let range = |anchor, first, last| AnchorRange {
            anchor,
            first,
            last,
        };
        vec![
            range(Anchor::Block, 0, 11),
            range(Anchor::Children, 3, 7),
            range(Anchor::Child(0), 5, 6),
            range(Anchor::Child(1), 7, 7),
            range(Anchor::Until, 8, 10),
        ]
    }

    fn own_line(next: usize) -> Position {
        Position {
            next,
            trailing: false,
        }
    }

    fn trailing(next: usize) -> Position {
        Position {
            next,
            trailing: true,
        }
    }

    #[test]
    fn an_own_line_comment_attaches_to_the_anchor_its_next_token_begins() {
        let anchors = anchors();

        assert_eq!(anchor_of(own_line(0), &anchors), Some(Anchor::Block));
        assert_eq!(anchor_of(own_line(3), &anchors), Some(Anchor::Children));
        assert_eq!(anchor_of(own_line(7), &anchors), Some(Anchor::Child(1)));
        assert_eq!(anchor_of(own_line(8), &anchors), Some(Anchor::Until));
    }

    #[test]
    fn an_own_line_comment_inside_an_anchor_attaches_to_the_innermost_one() {
        let anchors = anchors();

        // Between `until:` and `M3`: inside the property.
        assert_eq!(anchor_of(own_line(10), &anchors), Some(Anchor::Until));
        // Between `children:` and `a`: `a` begins the first entry.
        assert_eq!(anchor_of(own_line(5), &anchors), Some(Anchor::Child(0)));
        // Before the closing brace: dangling, so the block.
        assert_eq!(anchor_of(own_line(11), &anchors), Some(Anchor::Block));
        // Inside the header, between `x` and `{`.
        assert_eq!(anchor_of(own_line(2), &anchors), Some(Anchor::Block));
    }

    #[test]
    fn a_trailing_comment_attaches_to_the_innermost_anchor_of_the_token_before_it() {
        let anchors = anchors();

        // After `{`.
        assert_eq!(anchor_of(trailing(3), &anchors), Some(Anchor::Block));
        // After `children:`.
        assert_eq!(anchor_of(trailing(5), &anchors), Some(Anchor::Children));
        // After `a,`: the comma belongs to the entry.
        assert_eq!(anchor_of(trailing(7), &anchors), Some(Anchor::Child(0)));
        // After `M3`.
        assert_eq!(anchor_of(trailing(11), &anchors), Some(Anchor::Until));
        // After `}`.
        assert_eq!(anchor_of(trailing(12), &anchors), Some(Anchor::Block));
    }

    #[test]
    fn an_own_line_comment_outside_every_anchor_is_left_to_the_caller() {
        assert_eq!(anchor_of(own_line(12), &anchors()), None);
    }
}
