//! Recursive-descent parser: a lexer token stream becomes a span-carrying AST.
//!
//! The parser's only job is to fit text into the [`crate::ast`] types. Anything
//! the types cannot represent — a mode/property mismatch, an illegal `promote:`
//! target, a missing or duplicated property — becomes a [`Diagnostic`] instead
//! of aborting the parse.
//!
//! Recovery is per block: a malformed entry is diagnosed and the parser skips to
//! the next `}` or mode keyword, so one broken block never swallows the rest of
//! the file. Fences are single tokens from the lexer, so a `}` inside a
//! description can never be mistaken for the end of a block.

use crate::ast::{
    BindBlock, Block, BlockCommon, Deadline, Description, FiatBlock, FiatDeadline,
    FiatPromoteTarget, ObjectiveBlock, ObjectiveDeadline, ObjectivePromoteTarget, SourceRef,
    StructureBlock,
};
use crate::lexer::{Token, TokenKind};
use crate::source::{FileId, Span, Spanned};

/// A parse diagnostic: a message anchored to the span it concerns.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct Diagnostic {
    pub span: Span,
    pub message: String,
}

/// The outcome of a parse: every block that could be constructed, plus every
/// diagnostic collected along the way. A non-empty `diagnostics` does not imply
/// an empty `blocks`; recovery keeps producing well-formed blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parse {
    pub blocks: Vec<Block>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Parses a token stream into blocks and diagnostics.
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

/// Which evidence scheme a `ci:` / `veriq:` property names.
#[derive(Debug, Clone, Copy)]
enum SourceScheme {
    Ci,
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
    reason: Option<Spanned<String>>,
    promote: Option<Spanned<PromoteMode>>,
}

/// The validated common pieces of a block, handed to a per-mode builder.
struct BlockParts {
    common: BlockCommon,
    deadline: Option<Deadline>,
    promote: Option<Spanned<PromoteMode>>,
    children: Option<Spanned<Vec<Spanned<String>>>>,
    source: Option<Spanned<SourceRef>>,
}

struct Parser<'a> {
    file_id: FileId,
    tokens: &'a [Token],
    pos: usize,
    end_offset: usize,
    diagnostics: Vec<Diagnostic>,
}

impl<'a> Parser<'a> {
    fn new(file_id: FileId, tokens: &'a [Token]) -> Self {
        let end_offset = tokens.last().map_or(0, |token| token.span.end);
        Self {
            file_id,
            tokens,
            pos: 0,
            end_offset,
            diagnostics: Vec::new(),
        }
    }

    fn parse_file(mut self) -> Parse {
        let mut blocks = Vec::new();

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
        }

        Parse {
            blocks,
            diagnostics: self.diagnostics,
        }
    }

    // --- Block structure -------------------------------------------------

    fn parse_block(&mut self) -> Option<Block> {
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

        let span = Span {
            file_id: self.file_id,
            start,
            end,
        };
        self.assemble(mode, id, tags, span, raw)
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
                    self.recover_within_block();
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
            self.recover_within_block();
            return;
        }
        self.pos += 1;

        match name.as_str() {
            "children" => self.parse_children(raw, name_span),
            "ci" => self.parse_source(raw, name_span, SourceScheme::Ci),
            "veriq" => self.parse_source(raw, name_span, SourceScheme::Veriq),
            "until" => self.parse_until(raw),
            "reason" => self.parse_reason(raw),
            "promote" => self.parse_promote(raw, name_span),
            _ => {
                self.error(name_span, format!("unknown property `{name}`"));
                self.skip_property_value();
            }
        }
    }

    fn parse_children(&mut self, raw: &mut RawBlock, name_span: Span) {
        let mut ids = Vec::new();
        if let Some(id) = self.expect_ident("expected a child identifier") {
            ids.push(id);
            while matches!(self.peek_kind(), Some(TokenKind::Comma)) {
                self.pos += 1;
                match self.expect_ident("expected a child identifier after `,`") {
                    Some(id) => ids.push(id),
                    None => break,
                }
            }
        }

        if raw.children.is_some() {
            self.error(name_span, "duplicate property `children`");
            return;
        }
        raw.children = Some(Spanned {
            span: name_span,
            value: ids,
        });
    }

    fn parse_source(&mut self, raw: &mut RawBlock, name_span: Span, scheme: SourceScheme) {
        let Some(value) = self.expect_string("expected a quoted reference") else {
            return;
        };
        let source = match scheme {
            SourceScheme::Ci => SourceRef::Ci(value.value),
            SourceScheme::Veriq => SourceRef::Veriq(value.value),
        };
        let spanned = Spanned {
            span: join(name_span, value.span),
            value: source,
        };

        if raw.source.is_some() {
            self.error(
                name_span,
                "duplicate source reference; a bind takes exactly one of `ci:` or `veriq:`",
            );
            return;
        }
        raw.source = Some(spanned);
    }

    fn parse_until(&mut self, raw: &mut RawBlock) {
        let Some(phase) = self.expect_ident("expected a phase name after `until:`") else {
            return;
        };
        if raw.until.is_some() {
            self.error(phase.span, "duplicate property `until`");
            return;
        }
        raw.until = Some(phase);
    }

    fn parse_reason(&mut self, raw: &mut RawBlock) {
        let Some(text) = self.expect_string("expected a quoted reason after `reason:`") else {
            return;
        };
        if raw.reason.is_some() {
            self.error(text.span, "duplicate property `reason`");
            return;
        }
        raw.reason = Some(text);
    }

    fn parse_promote(&mut self, raw: &mut RawBlock, name_span: Span) {
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
                return;
            }
        };
        self.pos += 1;

        let Some(mode) = mode else {
            self.error(
                target_span,
                "invalid promote target; expected `bind` or `structure`",
            );
            return;
        };

        if raw.promote.is_some() {
            self.error(name_span, "duplicate property `promote`");
            return;
        }
        raw.promote = Some(Spanned {
            span: join(name_span, target_span),
            value: mode,
        });
    }

    // --- Assembly --------------------------------------------------------

    fn assemble(
        &mut self,
        mode: Mode,
        id: Spanned<String>,
        tags: Vec<Spanned<String>>,
        span: Span,
        mut raw: RawBlock,
    ) -> Option<Block> {
        let description = self.resolve_description(&mut raw, id.span)?;
        let common = BlockCommon {
            id,
            tags,
            description,
            span,
        };

        let deadline = if let Some(until) = raw.until.take() {
            Some(Deadline {
                until,
                reason: raw.reason.take(),
            })
        } else {
            if let Some(reason_span) = raw.reason.as_ref().map(|reason| reason.span) {
                self.error(reason_span, "`reason:` requires `until:`");
            }
            None
        };

        let parts = BlockParts {
            common,
            deadline,
            promote: raw.promote.take(),
            children: raw.children.take(),
            source: raw.source.take(),
        };

        match mode {
            Mode::Objective => Some(self.build_objective(parts)),
            Mode::Structure => Some(self.build_structure(parts)),
            Mode::Bind => self.build_bind(parts),
            Mode::Fiat => Some(self.build_fiat(parts)),
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

    fn build_objective(&mut self, parts: BlockParts) -> Block {
        self.forbid_source(parts.source.as_ref(), "objective");
        let deadline = match (parts.deadline, parts.promote) {
            (Some(deadline), promote) => Some(ObjectiveDeadline {
                deadline,
                promote: self.objective_promote(promote),
            }),
            (None, promote) => {
                self.promote_needs_until(promote.as_ref());
                None
            }
        };
        let children = self.require_children(parts.children, parts.common.id.span, "objective");
        Block::Objective(ObjectiveBlock {
            common: parts.common,
            deadline,
            children,
        })
    }

    fn build_structure(&mut self, parts: BlockParts) -> Block {
        self.forbid_source(parts.source.as_ref(), "structure");
        self.forbid_promote(parts.promote.as_ref(), "structure");
        let children = self.require_children(parts.children, parts.common.id.span, "structure");
        Block::Structure(StructureBlock {
            common: parts.common,
            deadline: parts.deadline,
            children,
        })
    }

    fn build_bind(&mut self, parts: BlockParts) -> Option<Block> {
        self.forbid_children(parts.children.as_ref(), "bind");
        self.forbid_promote(parts.promote.as_ref(), "bind");
        let Some(source) = parts.source else {
            self.error(
                parts.common.id.span,
                "a bind requires exactly one of `ci:` or `veriq:`",
            );
            return None;
        };
        Some(Block::Bind(BindBlock {
            common: parts.common,
            deadline: parts.deadline,
            source,
        }))
    }

    fn build_fiat(&mut self, parts: BlockParts) -> Block {
        self.forbid_children(parts.children.as_ref(), "fiat");
        self.forbid_source(parts.source.as_ref(), "fiat");
        let deadline = match (parts.deadline, parts.promote) {
            (Some(deadline), promote) => Some(FiatDeadline {
                deadline,
                promote: fiat_promote(promote),
            }),
            (None, promote) => {
                self.promote_needs_until(promote.as_ref());
                None
            }
        };
        Block::Fiat(FiatBlock {
            common: parts.common,
            deadline,
        })
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

    fn require_children(
        &mut self,
        children: Option<Spanned<Vec<Spanned<String>>>>,
        header: Span,
        mode: &str,
    ) -> Vec<Spanned<String>> {
        if let Some(children) = children {
            children.value
        } else {
            self.error(header, format!("a {mode} requires `children:`"));
            Vec::new()
        }
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
            Some(ident)
        } else {
            let span = self.current_span();
            self.error(span, message.to_owned());
            None
        }
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

    /// Skips a malformed or unknown property's value up to the next entry
    /// boundary, so the rest of the block still parses.
    fn skip_property_value(&mut self) {
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

    fn recover_within_block(&mut self) {
        while !self.at_eof() && !self.at_right_brace() && !self.at_mode_keyword() {
            self.pos += 1;
        }
    }

    fn error(&mut self, span: Span, message: impl Into<String>) {
        self.diagnostics.push(Diagnostic {
            span,
            message: message.into(),
        });
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
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

    fn at_right_brace(&self) -> bool {
        matches!(self.peek_kind(), Some(TokenKind::RightBrace))
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
    matches!(
        kind,
        TokenKind::Objective | TokenKind::Structure | TokenKind::Bind | TokenKind::Fiat
    )
}

fn join(start: Span, end: Span) -> Span {
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
        parse
            .diagnostics
            .iter()
            .map(|d| d.message.as_str())
            .collect()
    }

    fn has_message(parse: &Parse, needle: &str) -> bool {
        parse.diagnostics.iter().any(|d| d.message.contains(needle))
    }

    #[test]
    fn parses_objective_with_children() {
        let parse = parse_str("objective root {\n```\nRoot claim\n```\nchildren: alpha, beta\n}\n");

        assert!(parse.diagnostics.is_empty(), "{:?}", messages(&parse));
        let Some(Block::Objective(block)) = parse.blocks.first() else {
            panic!("expected an objective block, got {:?}", parse.blocks);
        };
        assert_eq!(block.common.id.value, "root");
        assert_eq!(block.common.description.raw, "Root claim\n");
        let child_ids: Vec<&str> = block.children.iter().map(|c| c.value.as_str()).collect();
        assert_eq!(child_ids, vec!["alpha", "beta"]);
        assert!(block.deadline.is_none());
    }

    #[test]
    fn parses_structure_with_tag() {
        let parse =
            parse_str("structure safe_mode #safety {\n```\nSafe mode claim\n```\nchildren: a\n}\n");

        assert!(parse.diagnostics.is_empty(), "{:?}", messages(&parse));
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

        assert!(parse.diagnostics.is_empty(), "{:?}", messages(&parse));
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
            parse_str("bind hashed {\n```\nHash claim\n```\nci: \"tests/hash_props.rs\"\n}\n");

        assert!(parse.diagnostics.is_empty(), "{:?}", messages(&parse));
        let Some(Block::Bind(block)) = parse.blocks.first() else {
            panic!("expected a bind block");
        };
        assert_eq!(
            block.source.value,
            SourceRef::Ci("tests/hash_props.rs".to_owned())
        );
    }

    #[test]
    fn parses_fiat_with_until_and_promote() {
        let parse = parse_str("fiat mtq_ok {\n```\nMTQ claim\n```\nuntil: FM\npromote: bind\n}\n");

        assert!(parse.diagnostics.is_empty(), "{:?}", messages(&parse));
        let Some(Block::Fiat(block)) = parse.blocks.first() else {
            panic!("expected a fiat block");
        };
        let deadline = block.deadline.as_ref().expect("a deadline");
        assert_eq!(deadline.deadline.until.value, "FM");
        assert_eq!(
            deadline.promote.as_ref().map(|p| p.value),
            Some(FiatPromoteTarget::Bind)
        );
    }

    #[test]
    fn parses_reason_only_with_until() {
        let parse = parse_str(
            "fiat assumed {\n```\nAssumed claim\n```\nuntil: M3\nreason: \"awaiting the test\"\n}\n",
        );

        assert!(parse.diagnostics.is_empty(), "{:?}", messages(&parse));
        let Some(Block::Fiat(block)) = parse.blocks.first() else {
            panic!("expected a fiat block");
        };
        let deadline = block.deadline.as_ref().expect("a deadline");
        assert_eq!(
            deadline.deadline.reason.as_ref().map(|r| r.value.as_str()),
            Some("awaiting the test")
        );
    }

    #[test]
    fn parses_objective_promote_to_structure() {
        let parse = parse_str(
            "objective root {\n```\nRoot claim\n```\nchildren: a\nuntil: M2\npromote: structure\n}\n",
        );

        assert!(parse.diagnostics.is_empty(), "{:?}", messages(&parse));
        let Some(Block::Objective(block)) = parse.blocks.first() else {
            panic!("expected an objective block");
        };
        let deadline = block.deadline.as_ref().expect("a deadline");
        assert_eq!(
            deadline.promote.as_ref().map(|p| p.value),
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

    // --- Diagnostics: the breakage fixtures from 01-syntax.md -------------

    #[test]
    fn rejects_block_without_description() {
        let parse = parse_str("fiat empty {\n}\n");

        assert!(parse.blocks.is_empty());
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

        assert!(parse.blocks.is_empty());
        assert!(
            has_message(&parse, "a bind requires exactly one"),
            "{:?}",
            messages(&parse)
        );
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
    fn rejects_reason_without_until() {
        let parse = parse_str("fiat f {\n```\nClaim\n```\nreason: \"why\"\n}\n");

        assert!(
            has_message(&parse, "`reason:` requires `until:`"),
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

        assert!(parse.diagnostics.is_empty(), "{:?}", messages(&parse));
        assert_eq!(parse.blocks.len(), 2);
    }

    #[test]
    fn parses_empty_input_to_no_blocks() {
        let parse = parse_str("");

        assert!(parse.blocks.is_empty());
        assert!(parse.diagnostics.is_empty());
    }
}
