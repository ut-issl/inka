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
