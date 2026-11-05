//! Recursive-descent parser: a lexer token stream becomes a span-carrying AST.
//!
//! The parser's only job is to fit text into the [`crate::ast`] types. Anything
//! the types cannot represent — a mode/property mismatch, an illegal `promote:`
//! target, a missing or duplicated property — becomes an [`Error`] instead
//! of aborting the parse.
//!
//! Recovery is per block: a malformed entry is diagnosed and the parser skips to
//! the next `}` or mode keyword, so one broken block never swallows the rest of
//! the file. Fences are single tokens from the lexer, so a `}` inside a
//! description can never be mistaken for the end of a block.
//!
//! Comments never cause an error. The parser reads the comment-free token
//! stream, records which tokens each anchor of a block covers, and then hands
//! every comment to [`crate::attach`] to find the element it belongs to.

use crate::ast::{
    BindBlock, Block, BlockCommon, ChildEntry, Comment, Deadline, Description, FiatBlock,
    FiatDeadline, FiatPromoteTarget, ObjectiveBlock, ObjectiveDeadline, ObjectivePromoteTarget,
    Promotion, SourceRef, StructureBlock,
};
use crate::attach::{
    Anchor, AnchorRange, CommentQueue, CommentSlots, PendingComment, Position, distribute,
};
use crate::lexer::{Token, TokenKind};
use crate::source::{Error, FileId, Span, Spanned};

/// The outcome of a parse: every block that could be constructed, plus every
/// error collected along the way. A non-empty `errors` does not imply
/// an empty `blocks`; recovery keeps producing well-formed blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parse {
    /// Every block that assembled cleanly enough to be represented in the AST.
    pub blocks: Vec<Block>,
    /// The comment attached to the file rather than to any block.
    pub comment: Option<Comment>,
    /// Every problem found, in source order.
    pub errors: Vec<Error>,
}

/// Parses a token stream into blocks and errors.
#[must_use]
pub fn parse(file_id: FileId, tokens: &[Token]) -> Parse {
    Parser::new(file_id, tokens).parse_file()
}

/// The discharge mode named by a block's opening keyword.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Objective,
    Structure,
    Bind,
    Fiat,
}

/// A `promote:` target before it is validated against the enclosing mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PromoteMode {
    Structure,
    Bind,
}

/// Which evidence scheme a `test:` / `veriq:` property names.
#[derive(Debug, Clone, Copy)]
enum SourceScheme {
    Test,
    Veriq,
}

/// Properties collected from a block body before mode-specific assembly.
///
/// Everything is optional here: the parser gathers whatever the body provides,
/// then [`Parser::assemble`] decides what is required, forbidden, or duplicated
/// for the specific mode.
#[derive(Default)]
struct RawBlock {
    descriptions: Vec<Description>,
    children: Option<Spanned<Vec<Spanned<String>>>>,
    source: Option<Spanned<SourceRef>>,
    until: Option<Spanned<String>>,
    promote: Option<Spanned<PromoteMode>>,
    /// The token range of every property and child entry that was accepted.
    anchors: Vec<AnchorRange>,
}

/// The validated common pieces of a block, handed to a per-mode builder.
///
/// `description` stays optional here so that a block missing its description
/// still runs every mode-specific validation — all of its diagnostics surface
/// in one pass — before the block itself is dropped.
struct BlockParts {
    id: Spanned<String>,
    tags: Vec<Spanned<String>>,
    span: Span,
    description: Option<Description>,
    deadline: Option<Deadline>,
    promote: Option<Spanned<PromoteMode>>,
    children: Option<Spanned<Vec<Spanned<String>>>>,
    source: Option<Spanned<SourceRef>>,
    comments: CommentSlots,
}

struct Parser<'a> {
    file_id: FileId,
    /// The token stream without its comments.
    tokens: Vec<&'a Token>,
    pos: usize,
    end_offset: usize,
    errors: Vec<Error>,
    /// The comments not yet attached.
    comments: CommentQueue,
}

impl<'a> Parser<'a> {
    fn new(file_id: FileId, tokens: &'a [Token]) -> Self {
        let mut significant = Vec::with_capacity(tokens.len());
        let mut comments = Vec::new();
        for token in tokens {
            if let TokenKind::Comment {
                text,
                trailing,
                blank_line_after,
            } = &token.value
            {
                comments.push(PendingComment {
                    line: Spanned {
                        span: token.span,
                        value: text.clone(),
                    },
                    position: Position {
                        next: significant.len(),
                        trailing: *trailing,
                    },
                    blank_line_after: *blank_line_after,
                });
            } else {
                significant.push(token);
            }
        }
        // Diagnostics at the end of input point past the last real token, not
        // past a comment that follows it.
        let end_offset = significant.last().map_or(0, |token| token.span.end);
        Self {
            file_id,
            tokens: significant,
            pos: 0,
            end_offset,
            errors: Vec::new(),
            comments: CommentQueue::new(comments),
        }
    }

    fn parse_file(mut self) -> Parse {
        let mut blocks = Vec::new();
        let mut file_lines = self.comments.take_file_header();

        while !self.at_eof() {
            if self.at_mode_keyword() {
                if let Some(block) = self.parse_block() {
                    blocks.push(block);
                }
            } else {
                let span = self.current_span();
                self.error(
                    span,
                    "expected a block (`objective`, `structure`, `bind`, or `fiat`)",
                );
                self.recover_to_next_block();
            }
            // Comments inside tokens no block could be read from belong to
            // the file, so they never land on the next, unrelated block.
            let skipped = self.comments.take_through(self.pos.saturating_sub(1));
            file_lines.extend(skipped.into_iter().map(|comment| comment.line));
        }

        // Comments after the last block belong to the file.
        file_lines.extend(self.comments.take_rest());

        Parse {
            blocks,
            comment: Comment::from_lines(file_lines),
            errors: self.errors,
        }
    }

    // --- Block structure -------------------------------------------------

    fn parse_block(&mut self) -> Option<Block> {
        let first = self.pos;
        let (mode, start) = {
            let token = self.peek()?;
            (mode_of(&token.value)?, token.span.start)
        };
        self.pos += 1;

        let Some(id) = self.parse_block_id() else {
            self.recover_to_next_block();
            return None;
        };
        let tags = self.parse_tags();

        if !matches!(self.peek_kind(), Some(TokenKind::LeftBrace)) {
            let span = self.current_span();
            self.error(span, "expected `{` to open the block body");
            self.recover_to_next_block();
            return None;
        }
        self.pos += 1;

        let mut raw = RawBlock::default();
        let end = self.parse_block_body(&mut raw);

        let last = self.pos.saturating_sub(1).max(first);
        let mut anchors = std::mem::take(&mut raw.anchors);
        anchors.push(AnchorRange {
            anchor: Anchor::Block,
            first,
            last,
        });
        // A block left unclosed at the end of input still owns the comments
        // after its last token: they are inside its braces.
        let unclosed_at_eof = self.at_eof()
            && !matches!(
                self.tokens.get(last).map(|t| &t.value),
                Some(TokenKind::RightBrace)
            );
        let through = if unclosed_at_eof {
            self.tokens.len()
        } else {
            last
        };
        let comments = distribute(self.comments.take_through(through), &anchors);

        let span = Span {
            file_id: self.file_id,
            start,
            end,
        };
        self.assemble(mode, id, tags, span, raw, comments)
    }

    fn parse_block_id(&mut self) -> Option<Spanned<String>> {
        if let Some(Spanned {
            span,
            value: TokenKind::Ident(name),
        }) = self.peek()
        {
            let id = Spanned {
                span: *span,
                value: name.clone(),
            };
            self.pos += 1;
            Some(id)
        } else {
            let span = self.current_span();
            self.error(span, "expected a block identifier after the mode keyword");
            None
        }
    }

    fn parse_tags(&mut self) -> Vec<Spanned<String>> {
        let mut tags = Vec::new();
        while let Some(Spanned {
            span,
            value: TokenKind::Tag(name),
        }) = self.peek()
        {
            tags.push(Spanned {
                span: *span,
                value: name.clone(),
            });
            self.pos += 1;
        }
        tags
    }

    /// Parses entries until the block closes, returning the block's end offset.
    fn parse_block_body(&mut self, raw: &mut RawBlock) -> usize {
        loop {
            match self.peek_kind() {
                None => {
                    let span = self.eof_span();
                    self.error(span, "unclosed block: expected `}`");
                    return self.end_offset;
                }
                Some(TokenKind::RightBrace) => {
                    let end = self.current_span().end;
                    self.pos += 1;
                    return end;
                }
                Some(kind) if is_mode_keyword(kind) => {
                    let span = self.current_span();
                    self.error(span, "unclosed block: expected `}` before the next block");
                    return span.start;
                }
                Some(TokenKind::FenceBlock { .. }) => self.parse_description(raw),
                Some(TokenKind::Ident(_)) => self.parse_property(raw),
                Some(_) => {
                    let span = self.current_span();
                    self.error(span, "expected a property or `}`");
                    self.skip_to_entry_boundary();
                }
            }
        }
    }

    fn parse_description(&mut self, raw: &mut RawBlock) {
        let Some(Spanned {
            value:
                TokenKind::FenceBlock {
                    raw: text,
                    info,
                    content_span,
                },
            ..
        }) = self.peek()
        else {
            return;
        };
        let description = Description {
            raw: text.clone(),
            info: info.clone(),
            span: *content_span,
        };
        self.pos += 1;
        raw.descriptions.push(description);
    }

    // --- Properties ------------------------------------------------------

    fn parse_property(&mut self, raw: &mut RawBlock) {
        let first = self.pos;
        let (name, name_span) = {
            let Some(Spanned {
                span,
                value: TokenKind::Ident(name),
            }) = self.peek()
            else {
                return;
            };
            (name.clone(), *span)
        };
        self.pos += 1;

        if !matches!(self.peek_kind(), Some(TokenKind::Colon)) {
            let span = self.current_span();
            self.error(span, format!("expected `:` after `{name}`"));
            self.skip_to_entry_boundary();
            return;
        }
        self.pos += 1;

        let anchor = match name.as_str() {
            "children" => self.parse_children(raw, name_span),
            "test" => self.parse_source(raw, name_span, SourceScheme::Test),
            "veriq" => self.parse_source(raw, name_span, SourceScheme::Veriq),
            "until" => self.parse_until(raw, name_span),
            "promote" => self.parse_promote(raw, name_span),
            _ => {
                self.error(name_span, format!("unknown property `{name}`"));
                self.skip_to_entry_boundary();
                None
            }
        };
        if let Some(anchor) = anchor {
            raw.anchors.push(AnchorRange {
                anchor,
                first,
                last: self.pos - 1,
            });
        }
    }

    /// Parses a `children:` list, returning its anchor when the property is
    /// accepted. Each entry covers its reference and the `,` after it.
    fn parse_children(&mut self, raw: &mut RawBlock, name_span: Span) -> Option<Anchor> {
        let mut ids = Vec::new();
        let mut entries = Vec::new();
        let mut message = "expected a child identifier";
        loop {
            let entry_first = self.pos;
            let Some(id) = self.parse_child_ref(message) else {
                self.skip_to_entry_boundary();
                break;
            };
            let has_comma = matches!(self.peek_kind(), Some(TokenKind::Comma));
            if has_comma {
                self.pos += 1;
            }
            entries.push(AnchorRange {
                anchor: Anchor::Child(ids.len()),
                first: entry_first,
                last: self.pos - 1,
            });
            ids.push(id);
            if !has_comma {
                break;
            }
            message = "expected a child identifier after `,`";
        }

        if raw.children.is_some() {
            self.error(name_span, "duplicate property `children`");
            return None;
        }
        raw.anchors.extend(entries);
        raw.children = Some(Spanned {
            span: name_span,
            value: ids,
        });
        Some(Anchor::Children)
    }

    /// Parses one child reference: an identifier optionally qualified with
    /// `::` namespace segments (`power::x`), collapsed into a single spanned
    /// string exactly as written.
    fn parse_child_ref(&mut self, message: &str) -> Option<Spanned<String>> {
        let mut id = self.expect_ident(message)?;
        while matches!(self.peek_kind(), Some(TokenKind::DoubleColon)) {
            self.pos += 1;
            let segment = self.expect_ident("expected an identifier after `::`")?;
            id = Spanned {
                span: join(id.span, segment.span),
                value: format!("{}::{}", id.value, segment.value),
            };
        }
        Some(id)
    }

    fn parse_source(
        &mut self,
        raw: &mut RawBlock,
        name_span: Span,
        scheme: SourceScheme,
    ) -> Option<Anchor> {
        let Some(value) = self.expect_string("expected a quoted reference") else {
            self.skip_to_entry_boundary();
            return None;
        };
        let source = match scheme {
            SourceScheme::Test => SourceRef::Test(value.value),
            SourceScheme::Veriq => SourceRef::Veriq(value.value),
        };
        let spanned = Spanned {
            span: join(name_span, value.span),
            value: source,
        };

        if raw.source.is_some() {
            self.error(
                name_span,
                "duplicate source reference; a bind takes exactly one of `test:` or `veriq:`",
            );
            return None;
        }
        raw.source = Some(spanned);
        Some(Anchor::Source)
    }

    fn parse_until(&mut self, raw: &mut RawBlock, name_span: Span) -> Option<Anchor> {
        let Some(phase) = self.expect_ident("expected a phase name after `until:`") else {
            self.skip_to_entry_boundary();
            return None;
        };
        if raw.until.is_some() {
            self.error(name_span, "duplicate property `until`");
            return None;
        }
        raw.until = Some(phase);
        Some(Anchor::Until)
    }

    fn parse_promote(&mut self, raw: &mut RawBlock, name_span: Span) -> Option<Anchor> {
        let (target_span, mode) = match self.peek() {
            Some(Spanned {
                span,
                value: TokenKind::Bind,
            }) => (*span, Some(PromoteMode::Bind)),
            Some(Spanned {
                span,
                value: TokenKind::Structure,
            }) => (*span, Some(PromoteMode::Structure)),
            Some(Spanned {
                span,
                value: TokenKind::Objective | TokenKind::Fiat | TokenKind::Ident(_),
            }) => (*span, None),
            _ => {
                let span = self.current_span();
                self.error(span, "expected a promote target after `promote:`");
                self.skip_to_entry_boundary();
                return None;
            }
        };
        self.pos += 1;

        let Some(mode) = mode else {
            self.error(
                target_span,
                "invalid promote target; expected `bind` or `structure`",
            );
            return None;
        };

        if raw.promote.is_some() {
            self.error(name_span, "duplicate property `promote`");
            return None;
        }
        raw.promote = Some(Spanned {
            span: join(name_span, target_span),
            value: mode,
        });
        Some(Anchor::Promote)
    }

    // --- Assembly --------------------------------------------------------

    fn assemble(
        &mut self,
        mode: Mode,
        id: Spanned<String>,
        tags: Vec<Spanned<String>>,
        span: Span,
        mut raw: RawBlock,
        mut comments: CommentSlots,
    ) -> Option<Block> {
        let description = self.resolve_description(&mut raw, id.span);

        let deadline = raw.until.take().map(|until| Deadline {
            until,
            comment: Comment::from_lines(std::mem::take(&mut comments.until)),
        });

        let parts = BlockParts {
            id,
            tags,
            span,
            description,
            deadline,
            promote: raw.promote.take(),
            children: raw.children.take(),
            source: raw.source.take(),
            comments,
        };

        match mode {
            Mode::Objective => self.build_objective(parts),
            Mode::Structure => self.build_structure(parts),
            Mode::Bind => self.build_bind(parts),
            Mode::Fiat => self.build_fiat(parts),
        }
    }

    fn resolve_description(&mut self, raw: &mut RawBlock, header: Span) -> Option<Description> {
        match raw.descriptions.len() {
            0 => {
                self.error(header, "a block must contain exactly one description");
                None
            }
            1 => Some(raw.descriptions.remove(0)),
            _ => {
                let extra_spans: Vec<Span> = raw
                    .descriptions
                    .iter()
                    .skip(1)
                    .map(|description| description.span)
                    .collect();
                for extra in extra_spans {
                    self.error(extra, "duplicate description; a block has exactly one");
                }
                Some(raw.descriptions.remove(0))
            }
        }
    }

    fn build_objective(&mut self, mut parts: BlockParts) -> Option<Block> {
        self.forbid_source(parts.source.as_ref(), "objective");
        let promote_comment = Comment::from_lines(std::mem::take(&mut parts.comments.promote));
        let deadline = match (parts.deadline, parts.promote) {
            (Some(deadline), promote) => Some(ObjectiveDeadline {
                deadline,
                promote: self.objective_promote(promote).map(|target| Promotion {
                    target,
                    comment: promote_comment,
                }),
            }),
            (None, promote) => {
                self.promote_needs_until(promote.as_ref());
                None
            }
        };
        let children = self.require_children(
            parts.children,
            &mut parts.comments,
            parts.id.span,
            "objective",
        );
        Some(Block::Objective(ObjectiveBlock {
            children_comment: Comment::from_lines(parts.comments.children),
            common: common(
                parts.id,
                parts.tags,
                parts.description?,
                parts.comments.block,
                parts.span,
            ),
            deadline,
            children,
        }))
    }

    fn build_structure(&mut self, mut parts: BlockParts) -> Option<Block> {
        self.forbid_source(parts.source.as_ref(), "structure");
        self.forbid_promote(parts.promote.as_ref(), "structure");
        let children = self.require_children(
            parts.children,
            &mut parts.comments,
            parts.id.span,
            "structure",
        );
        Some(Block::Structure(StructureBlock {
            children_comment: Comment::from_lines(parts.comments.children),
            common: common(
                parts.id,
                parts.tags,
                parts.description?,
                parts.comments.block,
                parts.span,
            ),
            deadline: parts.deadline,
            children,
        }))
    }

    fn build_bind(&mut self, parts: BlockParts) -> Option<Block> {
        self.forbid_children(parts.children.as_ref(), "bind");
        self.forbid_promote(parts.promote.as_ref(), "bind");
        if parts.source.is_none() {
            self.error(
                parts.id.span,
                "a bind requires exactly one of `test:` or `veriq:`",
            );
        }
        Some(Block::Bind(BindBlock {
            common: common(
                parts.id,
                parts.tags,
                parts.description?,
                parts.comments.block,
                parts.span,
            ),
            deadline: parts.deadline,
            source: parts.source?,
            source_comment: Comment::from_lines(parts.comments.source),
        }))
    }

    fn build_fiat(&mut self, parts: BlockParts) -> Option<Block> {
        self.forbid_children(parts.children.as_ref(), "fiat");
        self.forbid_source(parts.source.as_ref(), "fiat");
        let deadline = match (parts.deadline, parts.promote) {
            (Some(deadline), promote) => Some(FiatDeadline {
                deadline,
                promote: fiat_promote(promote).map(|target| Promotion {
                    target,
                    comment: Comment::from_lines(parts.comments.promote),
                }),
            }),
            (None, promote) => {
                self.promote_needs_until(promote.as_ref());
                None
            }
        };
        Some(Block::Fiat(FiatBlock {
            common: common(
                parts.id,
                parts.tags,
                parts.description?,
                parts.comments.block,
                parts.span,
            ),
            deadline,
        }))
    }

    // --- Mode-specific validation helpers --------------------------------

    fn objective_promote(
        &mut self,
        promote: Option<Spanned<PromoteMode>>,
    ) -> Option<Spanned<ObjectivePromoteTarget>> {
        let promote = promote?;
        match promote.value {
            PromoteMode::Structure => Some(Spanned {
                span: promote.span,
                value: ObjectivePromoteTarget::Structure,
            }),
            PromoteMode::Bind => {
                self.error(promote.span, "an objective may only promote to `structure`");
                None
            }
        }
    }

    /// Builds the child entries, each with its comment, or reports the
    /// missing `children:` property.
    fn require_children(
        &mut self,
        children: Option<Spanned<Vec<Spanned<String>>>>,
        comments: &mut CommentSlots,
        header: Span,
        mode: &str,
    ) -> Vec<ChildEntry> {
        let Some(children) = children else {
            self.error(header, format!("a {mode} requires `children:`"));
            return Vec::new();
        };
        children
            .value
            .into_iter()
            .enumerate()
            .map(|(index, id)| ChildEntry {
                id,
                comment: comments.take_child(index),
            })
            .collect()
    }

    fn forbid_children(&mut self, children: Option<&Spanned<Vec<Spanned<String>>>>, mode: &str) {
        if let Some(children) = children {
            self.error(children.span, format!("a {mode} cannot have `children:`"));
        }
    }

    fn forbid_source(&mut self, source: Option<&Spanned<SourceRef>>, mode: &str) {
        if let Some(source) = source {
            self.error(source.span, format!("a {mode} cannot have a source"));
        }
    }

    fn forbid_promote(&mut self, promote: Option<&Spanned<PromoteMode>>, mode: &str) {
        if let Some(promote) = promote {
            self.error(promote.span, format!("a {mode} cannot declare `promote:`"));
        }
    }

    fn promote_needs_until(&mut self, promote: Option<&Spanned<PromoteMode>>) {
        if let Some(promote) = promote {
            self.error(promote.span, "`promote:` requires `until:`");
        }
    }

    // --- Token helpers ---------------------------------------------------

    fn expect_ident(&mut self, message: &str) -> Option<Spanned<String>> {
        if let Some(Spanned {
            span,
            value: TokenKind::Ident(name),
        }) = self.peek()
        {
            let ident = Spanned {
                span: *span,
                value: name.clone(),
            };
            self.pos += 1;
            return Some(ident);
        }

        let span = self.current_span();

        /*
         * A mode keyword in value position is a common mistake; naming it
         * beats the generic message. It is consumed so it cannot cascade
         * into a bogus "unclosed block" — unless it is followed by an
         * identifier, in which case it likely really is the next block's
         * header and block-level recovery must see it.
         */
        if let Some(keyword) = self.peek_kind().and_then(keyword_text)
            && !matches!(self.peek_kind_at(1), Some(TokenKind::Ident(_)))
        {
            self.error(
                span,
                format!("`{keyword}` is a reserved keyword and cannot be used as an identifier"),
            );
            self.pos += 1;
            return None;
        }

        self.error(span, message.to_owned());
        None
    }

    fn expect_string(&mut self, message: &str) -> Option<Spanned<String>> {
        if let Some(Spanned {
            span,
            value: TokenKind::StringLiteral(text),
        }) = self.peek()
        {
            let literal = Spanned {
                span: *span,
                value: text.clone(),
            };
            self.pos += 1;
            Some(literal)
        } else {
            let span = self.current_span();
            self.error(span, message.to_owned());
            None
        }
    }

    /// Skips a malformed entry up to the next entry boundary — the next
    /// property (an identifier directly followed by `:`), description fence,
    /// `}`, mode keyword, or EOF — so the rest of the block still parses.
    fn skip_to_entry_boundary(&mut self) {
        loop {
            let at_boundary = match self.peek_kind() {
                None | Some(TokenKind::RightBrace | TokenKind::FenceBlock { .. }) => true,
                Some(kind) if is_mode_keyword(kind) => true,
                Some(TokenKind::Ident(_)) => {
                    matches!(self.peek_kind_at(1), Some(TokenKind::Colon))
                }
                Some(_) => false,
            };
            if at_boundary {
                break;
            }
            self.pos += 1;
        }
    }

    fn recover_to_next_block(&mut self) {
        while !self.at_eof() && !self.at_mode_keyword() {
            self.pos += 1;
        }
    }

    fn error(&mut self, span: Span, message: impl Into<String>) {
        self.errors.push(Error::new(span, message));
    }

    fn peek(&self) -> Option<&'a Token> {
        self.tokens.get(self.pos).copied()
    }

    fn peek_kind(&self) -> Option<&TokenKind> {
        self.peek().map(|token| &token.value)
    }

    fn peek_kind_at(&self, offset: usize) -> Option<&TokenKind> {
        self.tokens.get(self.pos + offset).map(|token| &token.value)
    }

    fn current_span(&self) -> Span {
        self.peek()
            .map_or_else(|| self.eof_span(), |token| token.span)
    }

    fn eof_span(&self) -> Span {
        Span {
            file_id: self.file_id,
            start: self.end_offset,
            end: self.end_offset,
        }
    }

    fn at_eof(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    fn at_mode_keyword(&self) -> bool {
        self.peek_kind().is_some_and(is_mode_keyword)
    }
}

fn common(
    id: Spanned<String>,
    tags: Vec<Spanned<String>>,
    description: Description,
    comment: Vec<Spanned<String>>,
    span: Span,
) -> BlockCommon {
    BlockCommon {
        id,
        tags,
        description,
        comment: Comment::from_lines(comment),
        span,
    }
}

fn fiat_promote(promote: Option<Spanned<PromoteMode>>) -> Option<Spanned<FiatPromoteTarget>> {
    promote.map(|promote| Spanned {
        span: promote.span,
        value: match promote.value {
            PromoteMode::Structure => FiatPromoteTarget::Structure,
            PromoteMode::Bind => FiatPromoteTarget::Bind,
        },
    })
}

fn mode_of(kind: &TokenKind) -> Option<Mode> {
    match kind {
        TokenKind::Objective => Some(Mode::Objective),
        TokenKind::Structure => Some(Mode::Structure),
        TokenKind::Bind => Some(Mode::Bind),
        TokenKind::Fiat => Some(Mode::Fiat),
        _ => None,
    }
}

fn is_mode_keyword(kind: &TokenKind) -> bool {
    keyword_text(kind).is_some()
}

/// The source text of a mode keyword token, for use in diagnostics.
fn keyword_text(kind: &TokenKind) -> Option<&'static str> {
    match kind {
        TokenKind::Objective => Some("objective"),
        TokenKind::Structure => Some("structure"),
        TokenKind::Bind => Some("bind"),
        TokenKind::Fiat => Some("fiat"),
        _ => None,
    }
}

fn join(start: Span, end: Span) -> Span {
    debug_assert_eq!(start.file_id, end.file_id);
    Span {
        file_id: start.file_id,
        start: start.start,
        end: end.end,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::Lexer;
    use crate::source::SourceMap;

    /// Lexes and parses `input`, panicking only on a lexer error (parser errors
    /// are collected into the returned [`Parse`], never raised).
    fn parse_str(input: &str) -> Parse {
        let mut map = SourceMap::new();
        let file_id = map.add_file("test.inka".into(), input.to_string());
        let tokens = Lexer::new(file_id, input)
            .lex()
            .expect("test input should lex");
        parse(file_id, &tokens)
    }

    fn messages(parse: &Parse) -> Vec<&str> {
        parse.errors.iter().map(|d| d.message.as_str()).collect()
    }

    fn has_message(parse: &Parse, needle: &str) -> bool {
        parse.errors.iter().any(|d| d.message.contains(needle))
    }

    #[test]
    fn parses_objective_with_children() {
        let parse = parse_str("objective root {\n```\nRoot claim\n```\nchildren: alpha, beta\n}\n");

        assert!(parse.errors.is_empty(), "{:?}", messages(&parse));
        let Some(Block::Objective(block)) = parse.blocks.first() else {
            panic!("expected an objective block, got {:?}", parse.blocks);
        };
        assert_eq!(block.common.id.value, "root");
        assert_eq!(block.common.description.raw, "Root claim\n");
        let child_ids: Vec<&str> = block.children.iter().map(|c| c.id.value.as_str()).collect();
        assert_eq!(child_ids, vec!["alpha", "beta"]);
        assert!(block.deadline.is_none());
    }

    #[test]
    fn parses_structure_with_tag() {
        let parse =
            parse_str("structure safe_mode #safety {\n```\nSafe mode claim\n```\nchildren: a\n}\n");

        assert!(parse.errors.is_empty(), "{:?}", messages(&parse));
        let Some(Block::Structure(block)) = parse.blocks.first() else {
            panic!("expected a structure block");
        };
        let tags: Vec<&str> = block.common.tags.iter().map(|t| t.value.as_str()).collect();
        assert_eq!(tags, vec!["safety"]);
    }

    #[test]
    fn parses_bind_with_veriq_source() {
        let parse = parse_str(
            "bind detumble_ok {\n```\nDetumble claim\n```\nveriq: \"aocs.verify_detumble\"\n}\n",
        );

        assert!(parse.errors.is_empty(), "{:?}", messages(&parse));
        let Some(Block::Bind(block)) = parse.blocks.first() else {
            panic!("expected a bind block");
        };
        assert_eq!(
            block.source.value,
            SourceRef::Veriq("aocs.verify_detumble".to_owned())
        );
    }

    #[test]
    fn parses_bind_with_ci_source() {
        let parse =
            parse_str("bind hashed {\n```\nHash claim\n```\ntest: \"tests/hash_props.rs\"\n}\n");

        assert!(parse.errors.is_empty(), "{:?}", messages(&parse));
        let Some(Block::Bind(block)) = parse.blocks.first() else {
            panic!("expected a bind block");
        };
        assert_eq!(
            block.source.value,
            SourceRef::Test("tests/hash_props.rs".to_owned())
        );
    }

    #[test]
    fn parses_fiat_with_until_and_promote() {
        let parse = parse_str("fiat mtq_ok {\n```\nMTQ claim\n```\nuntil: FM\npromote: bind\n}\n");

        assert!(parse.errors.is_empty(), "{:?}", messages(&parse));
        let Some(Block::Fiat(block)) = parse.blocks.first() else {
            panic!("expected a fiat block");
        };
        let deadline = block.deadline.as_ref().expect("a deadline");
        assert_eq!(deadline.deadline.until.value, "FM");
        assert_eq!(
            deadline.promote.as_ref().map(|p| p.target.value),
            Some(FiatPromoteTarget::Bind)
        );
    }

    #[test]
    fn rejects_reason_as_an_unknown_property() {
        let parse = parse_str(
            "fiat assumed {\n```\nAssumed claim\n```\nuntil: M3\nreason: \"awaiting the test\"\n}\n",
        );

        assert!(
            has_message(&parse, "unknown property `reason`"),
            "{:?}",
            messages(&parse)
        );
    }

    #[test]
    fn parses_objective_promote_to_structure() {
        let parse = parse_str(
            "objective root {\n```\nRoot claim\n```\nchildren: a\nuntil: M2\npromote: structure\n}\n",
        );

        assert!(parse.errors.is_empty(), "{:?}", messages(&parse));
        let Some(Block::Objective(block)) = parse.blocks.first() else {
            panic!("expected an objective block");
        };
        let deadline = block.deadline.as_ref().expect("a deadline");
        assert_eq!(
            deadline.promote.as_ref().map(|p| p.target.value),
            Some(ObjectivePromoteTarget::Structure)
        );
    }

    #[test]
    fn block_span_covers_keyword_through_closing_brace() {
        let input = "fiat leaf {\n```\nLeaf\n```\n}\n";
        let parse = parse_str(input);
        let block = parse.blocks.first().expect("a block");
        let span = block.common().span;
        assert_eq!(
            &input[span.start..span.end],
            "fiat leaf {\n```\nLeaf\n```\n}"
        );
    }

    // --- Diagnostics: the breakage cases from docs/en/design/spec.md ------

    #[test]
    fn rejects_block_without_description() {
        let parse = parse_str("fiat empty {\n}\n");

        assert_eq!(parse.blocks, []);
        assert!(
            has_message(&parse, "exactly one description"),
            "{:?}",
            messages(&parse)
        );
    }

    #[test]
    fn rejects_two_descriptions_but_keeps_the_block() {
        let parse = parse_str("fiat two {\n```\nfirst\n```\n```\nsecond\n```\n}\n");

        assert_eq!(parse.blocks.len(), 1, "the block is still produced");
        assert!(has_message(&parse, "duplicate description"));
    }

    #[test]
    fn rejects_children_on_fiat() {
        let parse = parse_str("fiat leaf {\n```\nLeaf\n```\nchildren: a\n}\n");

        assert!(
            has_message(&parse, "a fiat cannot have `children:`"),
            "{:?}",
            messages(&parse)
        );
    }

    #[test]
    fn rejects_bind_without_source() {
        let parse = parse_str("bind orphan {\n```\nNo source\n```\n}\n");

        assert_eq!(parse.blocks, []);
        assert!(
            has_message(&parse, "a bind requires exactly one"),
            "{:?}",
            messages(&parse)
        );
    }

    #[test]
    fn rejects_bind_with_both_ci_and_veriq() {
        let parse = parse_str(
            "bind twice {\n```\nClaim\n```\ntest: \"tests/a.rs\"\nveriq: \"m.check\"\n}\n",
        );

        assert!(
            has_message(&parse, "duplicate source reference"),
            "{:?}",
            messages(&parse)
        );
        // The first reference wins and the block still parses.
        assert_eq!(parse.blocks.len(), 1);
        let Some(Block::Bind(block)) = parse.blocks.first() else {
            panic!("expected a bind block");
        };
        assert_eq!(block.source.value, SourceRef::Test("tests/a.rs".to_owned()));
    }

    #[test]
    fn rejects_promote_on_structure() {
        let parse = parse_str(
            "structure s {\n```\nStruct\n```\nchildren: a\nuntil: M2\npromote: bind\n}\n",
        );

        assert!(
            has_message(&parse, "a structure cannot declare `promote:`"),
            "{:?}",
            messages(&parse)
        );
    }

    #[test]
    fn rejects_objective_promote_to_bind() {
        let parse = parse_str(
            "objective root {\n```\nRoot\n```\nchildren: a\nuntil: M2\npromote: bind\n}\n",
        );

        assert!(
            has_message(&parse, "may only promote to `structure`"),
            "{:?}",
            messages(&parse)
        );
    }

    #[test]
    fn rejects_promote_without_until() {
        let parse = parse_str("fiat f {\n```\nClaim\n```\npromote: bind\n}\n");

        assert!(
            has_message(&parse, "`promote:` requires `until:`"),
            "{:?}",
            messages(&parse)
        );
    }

    #[test]
    fn rejects_invalid_promote_target() {
        let parse = parse_str("fiat f {\n```\nClaim\n```\nuntil: M2\npromote: fiat\n}\n");

        assert!(
            has_message(&parse, "invalid promote target"),
            "{:?}",
            messages(&parse)
        );
    }

    #[test]
    fn rejects_unknown_property() {
        let parse = parse_str("fiat f {\n```\nClaim\n```\nweight: heavy\n}\n");

        assert!(
            has_message(&parse, "unknown property `weight`"),
            "{:?}",
            messages(&parse)
        );
        assert_eq!(
            parse.blocks.len(),
            1,
            "the block still parses around the unknown property"
        );
    }

    #[test]
    fn rejects_duplicate_property() {
        let parse = parse_str("objective root {\n```\nRoot\n```\nchildren: a\nchildren: b\n}\n");

        assert!(
            has_message(&parse, "duplicate property `children`"),
            "{:?}",
            messages(&parse)
        );
    }

    #[test]
    fn recovers_after_unclosed_block_so_next_block_survives() {
        // The structure block is never closed; the parser must stop at the next
        // mode keyword and still parse the fiat that follows.
        let parse = parse_str(
            "structure broken {\n```\nBroken\n```\nchildren: a\nfiat healthy {\n```\nHealthy\n```\n}\n",
        );

        assert!(
            has_message(&parse, "unclosed block"),
            "{:?}",
            messages(&parse)
        );
        assert_eq!(parse.blocks.len(), 2);
        let ids: Vec<&str> = parse
            .blocks
            .iter()
            .map(|b| b.common().id.value.as_str())
            .collect();
        assert_eq!(ids, vec!["broken", "healthy"]);
    }

    #[test]
    fn parses_multiple_blocks_in_sequence() {
        let parse = parse_str(
            "objective root {\n```\nRoot\n```\nchildren: a\n}\nfiat a {\n```\nLeaf\n```\n}\n",
        );

        assert!(parse.errors.is_empty(), "{:?}", messages(&parse));
        assert_eq!(parse.blocks.len(), 2);
    }

    #[test]
    fn parses_empty_input_to_no_blocks() {
        let parse = parse_str("");

        assert_eq!(parse.blocks, []);
        assert_eq!(parse.errors, []);
    }

    #[test]
    fn parses_namespaced_children() {
        let input = "objective root {\n```\nRoot\n```\nchildren: power::x, y\n}\n";
        let parse = parse_str(input);

        assert!(parse.errors.is_empty(), "{:?}", messages(&parse));
        let Some(Block::Objective(block)) = parse.blocks.first() else {
            panic!("expected an objective block");
        };
        let child_ids: Vec<&str> = block.children.iter().map(|c| c.id.value.as_str()).collect();
        assert_eq!(child_ids, vec!["power::x", "y"]);

        let first = &block.children[0];
        assert_eq!(&input[first.id.span.start..first.id.span.end], "power::x");
    }

    #[test]
    fn parses_multiple_tags() {
        let parse = parse_str("structure s #a #b {\n```\nClaim\n```\nchildren: c\n}\n");

        assert!(parse.errors.is_empty(), "{:?}", messages(&parse));
        let Some(Block::Structure(block)) = parse.blocks.first() else {
            panic!("expected a structure block");
        };
        let tags: Vec<&str> = block.common.tags.iter().map(|t| t.value.as_str()).collect();
        assert_eq!(tags, vec!["a", "b"]);
    }

    #[test]
    fn parses_bind_with_deadline() {
        let parse = parse_str("bind b {\n```\nClaim\n```\ntest: \"tests/x.rs\"\nuntil: M2\n}\n");

        assert!(parse.errors.is_empty(), "{:?}", messages(&parse));
        let Some(Block::Bind(block)) = parse.blocks.first() else {
            panic!("expected a bind block");
        };
        let deadline = block.deadline.as_ref().expect("a deadline");
        assert_eq!(deadline.until.value, "M2");
    }

    #[test]
    fn parses_fiat_promote_to_structure() {
        let parse = parse_str("fiat f {\n```\nClaim\n```\nuntil: M2\npromote: structure\n}\n");

        assert!(parse.errors.is_empty(), "{:?}", messages(&parse));
        let Some(Block::Fiat(block)) = parse.blocks.first() else {
            panic!("expected a fiat block");
        };
        let deadline = block.deadline.as_ref().expect("a deadline");
        assert_eq!(
            deadline.promote.as_ref().map(|p| p.target.value),
            Some(FiatPromoteTarget::Structure)
        );
    }

    #[test]
    fn recovers_to_next_property_after_missing_colon() {
        let parse = parse_str("objective root {\n```\nRoot\n```\nfoo\nchildren: a\n}\n");

        assert_eq!(
            messages(&parse),
            vec!["expected `:` after `foo`"],
            "recovery must not swallow the following valid property"
        );
        let Some(Block::Objective(block)) = parse.blocks.first() else {
            panic!("expected an objective block");
        };
        let child_ids: Vec<&str> = block.children.iter().map(|c| c.id.value.as_str()).collect();
        assert_eq!(child_ids, vec!["a"]);
    }

    #[test]
    fn keeps_later_property_after_bad_property_value() {
        // `until:` is given a string instead of a phase name; `promote:` must
        // still be seen (proved by its own requires-until diagnostic).
        let parse = parse_str("fiat f {\n```\nClaim\n```\nuntil: \"M2\"\npromote: bind\n}\n");

        assert!(
            has_message(&parse, "expected a phase name after `until:`"),
            "{:?}",
            messages(&parse)
        );
        assert!(
            has_message(&parse, "`promote:` requires `until:`"),
            "the property after the bad value must not be swallowed: {:?}",
            messages(&parse)
        );
    }

    #[test]
    fn duplicate_property_diagnostic_points_at_property_name() {
        let input = "fiat f {\n```\nClaim\n```\nuntil: M2\nuntil: M3\n}\n";
        let parse = parse_str(input);

        let diagnostic = parse
            .errors
            .iter()
            .find(|d| d.message.contains("duplicate property `until`"))
            .expect("a duplicate diagnostic");
        let span = diagnostic.span.expect("parse errors are anchored");
        assert_eq!(&input[span.start..span.end], "until");
    }

    #[test]
    fn reports_mode_violations_even_without_description() {
        let parse = parse_str("fiat f {\nchildren: a\n}\n");

        assert_eq!(parse.blocks, []);
        assert!(
            has_message(&parse, "exactly one description"),
            "{:?}",
            messages(&parse)
        );
        assert!(
            has_message(&parse, "a fiat cannot have `children:`"),
            "mode validation must run even when the description is missing: {:?}",
            messages(&parse)
        );
    }

    #[test]
    fn rejects_keyword_as_child_identifier_with_one_diagnostic() {
        let parse = parse_str("objective root {\n```\nRoot\n```\nchildren: fiat\n}\n");

        assert_eq!(
            messages(&parse),
            vec!["`fiat` is a reserved keyword and cannot be used as an identifier"],
            "one typo must not cascade into unclosed-block errors"
        );
        assert_eq!(parse.blocks.len(), 1, "the block itself survives");
    }

    // --- Comments --------------------------------------------------------

    fn text(comment: Option<&Comment>) -> Option<String> {
        comment.map(Comment::text)
    }

    fn only_structure(parse: &Parse) -> &StructureBlock {
        assert!(parse.errors.is_empty(), "{:?}", messages(parse));
        let Some(Block::Structure(block)) = parse.blocks.first() else {
            panic!("expected a structure block");
        };
        block
    }

    #[test]
    fn attaches_each_comment_to_the_element_the_spec_names() {
        let parse = parse_str(
            "\
// (1) file

// (2) block
structure s { // (3) block
    ```
    claim
    ```
    // (2) children
    children:
        // (2) child a
        a, // (3) child a
        b
    until: M3 // (3) until
    // (4) block
}
",
        );

        let block = only_structure(&parse);
        assert_eq!(text(parse.comment.as_ref()).as_deref(), Some("(1) file"));
        assert_eq!(
            text(block.common.comment.as_ref()).as_deref(),
            Some("(2) block\n(3) block\n(4) block")
        );
        assert_eq!(
            text(block.children_comment.as_ref()).as_deref(),
            Some("(2) children")
        );
        assert_eq!(
            text(block.children[0].comment.as_ref()).as_deref(),
            Some("(2) child a\n(3) child a")
        );
        assert_eq!(block.children[1].comment, None);
        let deadline = block.deadline.as_ref().expect("a deadline");
        assert_eq!(
            text(deadline.comment.as_ref()).as_deref(),
            Some("(3) until")
        );
    }

    #[test]
    fn a_top_of_file_comment_without_a_blank_line_belongs_to_the_first_block() {
        let parse = parse_str("// leading\nfiat f {\n```\nClaim\n```\n}\n");

        assert_eq!(parse.comment, None);
        assert_eq!(
            text(parse.blocks[0].common().comment.as_ref()).as_deref(),
            Some("leading")
        );
    }

    #[test]
    fn only_the_first_top_of_file_run_is_the_file_comment() {
        let parse = parse_str("// file\n\n// block\n\nfiat f {\n```\nClaim\n```\n}\n");

        assert_eq!(text(parse.comment.as_ref()).as_deref(), Some("file"));
        assert_eq!(
            text(parse.blocks[0].common().comment.as_ref()).as_deref(),
            Some("block")
        );
    }

    #[test]
    fn comments_between_blocks_go_to_the_next_block_and_after_the_last_to_the_file() {
        let parse = parse_str(
            "fiat a {\n```\nA\n```\n} // on a\n\n// on b\n\nfiat b {\n```\nB\n```\n}\n// tail\n",
        );

        assert!(parse.errors.is_empty(), "{:?}", messages(&parse));
        assert_eq!(
            text(parse.blocks[0].common().comment.as_ref()).as_deref(),
            Some("on a")
        );
        assert_eq!(
            text(parse.blocks[1].common().comment.as_ref()).as_deref(),
            Some("on b")
        );
        assert_eq!(text(parse.comment.as_ref()).as_deref(), Some("tail"));
    }

    #[test]
    fn a_comment_before_the_description_or_inside_the_header_belongs_to_the_block() {
        let parse = parse_str("fiat // in header\n f #t {\n// before fence\n```\nClaim\n```\n}\n");

        assert!(parse.errors.is_empty(), "{:?}", messages(&parse));
        assert_eq!(
            text(parse.blocks[0].common().comment.as_ref()).as_deref(),
            Some("in header\nbefore fence")
        );
    }

    #[test]
    fn attaches_comments_to_the_source_and_promote_properties() {
        let parse = parse_str(
            "bind b {\n```\nB\n```\n// on test\ntest: \"t\"\n}\n\
             fiat f {\n```\nF\n```\nuntil: M3\npromote: // on promote\n bind\n}\n",
        );

        assert!(parse.errors.is_empty(), "{:?}", messages(&parse));
        let Block::Bind(bind) = &parse.blocks[0] else {
            panic!("expected a bind block");
        };
        assert_eq!(
            text(bind.source_comment.as_ref()).as_deref(),
            Some("on test")
        );
        let Block::Fiat(fiat) = &parse.blocks[1] else {
            panic!("expected a fiat block");
        };
        let promote = fiat
            .deadline
            .as_ref()
            .and_then(|deadline| deadline.promote.as_ref())
            .expect("a promotion");
        assert_eq!(
            text(promote.comment.as_ref()).as_deref(),
            Some("on promote")
        );
    }

    #[test]
    fn a_comment_inside_a_property_value_belongs_to_the_property() {
        let parse = parse_str("fiat f {\n```\nF\n```\nuntil:\n// why\nM3\n}\n");

        assert!(parse.errors.is_empty(), "{:?}", messages(&parse));
        let deadline = parse.blocks[0].deadline().expect("a deadline");
        assert_eq!(text(deadline.comment.as_ref()).as_deref(), Some("why"));
    }

    #[test]
    fn empty_comment_lines_are_trimmed_and_an_all_empty_comment_is_none() {
        let parse = parse_str(
            "//\n// kept\n//\n// also kept\n//\nfiat a {\n```\nA\n```\n}\n//\n//\nfiat b {\n```\nB\n```\n}\n",
        );

        assert_eq!(
            text(parse.blocks[0].common().comment.as_ref()).as_deref(),
            Some("kept\n\nalso kept")
        );
        assert_eq!(parse.blocks[1].common().comment, None);
    }

    #[test]
    fn a_file_of_only_comments_has_a_file_comment_and_no_errors() {
        let parse = parse_str("// one\n// two\n");

        assert_eq!(parse.errors, []);
        assert_eq!(text(parse.comment.as_ref()).as_deref(), Some("one\ntwo"));
    }

    #[test]
    fn comments_inside_a_block_that_fails_to_parse_do_not_move_to_the_next_block() {
        let parse = parse_str(
            "bind {\n```\nd\n```\n// inside broken\ntest: \"t\"\n}\ngarbage // after junk\nfiat ok {\n```\nd\n```\n}\n",
        );

        assert!(
            has_message(&parse, "expected a block identifier"),
            "{:?}",
            messages(&parse)
        );
        let ok = parse.blocks.last().expect("ok survives");
        assert_eq!(ok.common().comment, None);
        assert_eq!(
            text(parse.comment.as_ref()).as_deref(),
            Some("inside broken\nafter junk")
        );
    }

    #[test]
    fn a_block_unclosed_at_the_end_of_input_keeps_its_dangling_comment() {
        let parse = parse_str("fiat a {\n```\nd\n```\n// dangling\n");

        assert!(
            has_message(&parse, "unclosed block"),
            "{:?}",
            messages(&parse)
        );
        assert_eq!(
            text(parse.blocks[0].common().comment.as_ref()).as_deref(),
            Some("dangling")
        );
        assert_eq!(parse.comment, None);
    }

    #[test]
    fn the_end_of_input_error_points_past_the_last_token_not_a_trailing_comment() {
        let input = "fiat a {\n```\nd\n```\n// dangling\n";
        let parse = parse_str(input);

        let error = parse
            .errors
            .iter()
            .find(|error| error.message.contains("unclosed block"))
            .expect("an unclosed-block error");
        let span = error.span.expect("anchored");
        assert_eq!(span.start, input.find("// dangling").expect("present"));
    }
}
