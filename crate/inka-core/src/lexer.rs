use unicode_normalization::UnicodeNormalization;

use crate::source::{FileId, Span, Spanned};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    Objective,
    Structure,
    Bind,
    Fiat,

    Ident(String),
    Tag(String),
    StringLiteral(String),

    LeftBrace,
    RightBrace,
    Colon,
    Comma,
    DoubleColon,
    FenceBlock {
        raw: String,
        info: Option<String>,
        /// Number of backticks in the opening fence.
        fence_length: usize,
        content_span: Span,
    },
    /// A `//` comment outside a fence, running to the end of its line.
    Comment {
        /// The comment's text: `//`, one following space, and trailing
        /// whitespace removed, NFC-normalized.
        text: String,
        /// Whether another token precedes the comment on its line.
        trailing: bool,
        /// Whether the line right after the comment is blank.
        blank_line_after: bool,
    },
}

pub type Token = Spanned<TokenKind>;

/// An error produced while lexing: the shared [`crate::source::Error`]
/// shape under the name callers of this module expect.
pub type LexError = crate::source::Error;

pub struct Lexer<'a> {
    file_id: FileId,
    input: &'a str,
    offset: usize,
    /// Whether a token other than a comment has been emitted on the current
    /// line, which is what makes a comment trailing.
    line_has_token: bool,
}

impl<'a> Lexer<'a> {
    #[must_use]
    pub fn new(file_id: FileId, input: &'a str) -> Self {
        Self {
            file_id,
            input,
            offset: 0,
            line_has_token: false,
        }
    }

    /// Tokenizes the whole input into a flat token stream.
    ///
    /// Comments are tokens too, in source order; the parser attaches each one
    /// to an element.
    ///
    /// # Errors
    ///
    /// Returns a [`LexError`] for an unterminated fence or string literal, a
    /// `#` tag with no name, or a character that cannot start a token.
    pub fn lex(mut self) -> Result<Vec<Token>, LexError> {
        let mut tokens = Vec::new();

        while !self.is_eof() {
            /*
             * Fence detection must happen before comment handling because
             * `//` inside a fenced description is ordinary text.
             */
            if self.is_at_line_start() {
                self.line_has_token = false;
                if let Some(fence) = self.try_lex_fence()? {
                    tokens.push(fence);
                    continue;
                }
            }

            self.skip_horizontal_whitespace();

            if self.is_eof() {
                break;
            }

            if self.starts_with("\n") || self.starts_with("\r") {
                self.offset += 1;
                continue;
            }

            if self.starts_with("//") {
                tokens.push(self.lex_comment());
                continue;
            }

            let token = match self.current_byte() {
                Some(b'{') => self.single_char_token(TokenKind::LeftBrace),
                Some(b'}') => self.single_char_token(TokenKind::RightBrace),
                Some(b',') => self.single_char_token(TokenKind::Comma),

                Some(b':') if self.starts_with("::") => {
                    let start = self.offset;
                    self.offset += 2;
                    self.token(start, self.offset, TokenKind::DoubleColon)
                }

                Some(b':') => self.single_char_token(TokenKind::Colon),

                Some(b'#') => self.lex_tag()?,

                Some(b'"') => self.lex_string()?,

                Some(byte) if is_ident_start(byte) => self.lex_identifier(),

                Some(_) => {
                    return Err(self.error_at_current("unexpected character"));
                }

                None => break,
            };

            self.line_has_token = true;
            tokens.push(token);
        }

        Ok(tokens)
    }

    fn lex_comment(&mut self) -> Token {
        let start = self.offset;
        let line_end = self.find_line_end(start);
        let body = &self.input[start + "//".len()..line_end];
        let body = body.strip_prefix(' ').unwrap_or(body);
        let text: String = body.trim_end().nfc().collect();

        let next_line_start = self.next_line_start(line_end);
        let next_line = &self.input[next_line_start..self.find_line_end(next_line_start)];
        let blank_line_after = next_line_start < self.input.len() && next_line.trim().is_empty();

        self.offset = line_end;
        self.token(
            start,
            line_end,
            TokenKind::Comment {
                text,
                trailing: self.line_has_token,
                blank_line_after,
            },
        )
    }

    fn try_lex_fence(&mut self) -> Result<Option<Token>, LexError> {
        let line_start = self.offset;
        let line_end = self.find_line_end(line_start);
        let line = &self.input[line_start..line_end];

        let Some(opening) = parse_opening_fence(line) else {
            return Ok(None);
        };

        let token_start = line_start;
        let opening_line_end = self.next_line_start(line_end);
        let content_start = opening_line_end;

        self.offset = opening_line_end;

        while !self.is_eof() {
            let closing_line_start = self.offset;
            let closing_line_end = self.find_line_end(closing_line_start);
            let closing_line = &self.input[closing_line_start..closing_line_end];

            if is_closing_fence(closing_line, opening.backtick_count) {
                let raw = self.input[content_start..closing_line_start].to_owned();
                let token_end = self.next_line_start(closing_line_end);

                let token = Spanned {
                    span: self.span(token_start, token_end),
                    value: TokenKind::FenceBlock {
                        raw,
                        info: opening.info,
                        fence_length: opening.backtick_count,
                        content_span: self.span(content_start, closing_line_start),
                    },
                };

                self.offset = token_end;
                return Ok(Some(token));
            }

            self.offset = self.next_line_start(closing_line_end);
        }

        Err(LexError::new(
            self.span(token_start, self.input.len()),
            "unclosed fenced description",
        ))
    }

    fn lex_identifier(&mut self) -> Token {
        let start = self.offset;

        self.offset += 1;

        while let Some(byte) = self.current_byte() {
            if !is_ident_continue(byte) {
                break;
            }

            self.offset += 1;
        }

        let text = &self.input[start..self.offset];

        let kind = match text {
            "objective" => TokenKind::Objective,
            "structure" => TokenKind::Structure,
            "bind" => TokenKind::Bind,
            "fiat" => TokenKind::Fiat,
            _ => TokenKind::Ident(text.to_owned()),
        };

        self.token(start, self.offset, kind)
    }

    fn lex_tag(&mut self) -> Result<Token, LexError> {
        let start = self.offset;
        self.offset += 1;

        let value_start = self.offset;

        while let Some(byte) = self.current_byte() {
            if !is_ident_continue(byte) {
                break;
            }

            self.offset += 1;
        }

        if self.offset == value_start {
            return Err(LexError::new(
                self.span(start, self.offset),
                "expected a tag name after `#`",
            ));
        }

        let value = self.input[value_start..self.offset].to_owned();

        Ok(self.token(start, self.offset, TokenKind::Tag(value)))
    }

    fn lex_string(&mut self) -> Result<Token, LexError> {
        let start = self.offset;
        self.offset += 1;

        let value_start = self.offset;

        while let Some(byte) = self.current_byte() {
            match byte {
                b'"' => {
                    let value = self.input[value_start..self.offset].to_owned();
                    self.offset += 1;

                    return Ok(self.token(start, self.offset, TokenKind::StringLiteral(value)));
                }

                b'\n' | b'\r' => {
                    return Err(LexError::new(
                        self.span(start, self.offset),
                        "string literal must end on the same line",
                    ));
                }

                _ => {
                    self.offset += 1;
                }
            }
        }

        Err(LexError::new(
            self.span(start, self.input.len()),
            "unclosed string literal",
        ))
    }

    fn skip_horizontal_whitespace(&mut self) {
        while matches!(self.current_byte(), Some(b' ' | b'\t')) {
            self.offset += 1;
        }
    }

    fn single_char_token(&mut self, kind: TokenKind) -> Token {
        let start = self.offset;
        self.offset += 1;
        self.token(start, self.offset, kind)
    }

    fn token(&self, start: usize, end: usize, value: TokenKind) -> Token {
        Spanned {
            span: self.span(start, end),
            value,
        }
    }

    fn error_at_current(&self, message: impl Into<String>) -> LexError {
        // Span the whole UTF-8 character so both ends stay on char
        // boundaries and the range remains safe to slice when a diagnostic
        // renders it. A single `+ 1` would split a multi-byte character.
        let width = self.input[self.offset..]
            .chars()
            .next()
            .map_or(0, char::len_utf8);
        let end = self.offset + width;

        LexError::new(self.span(self.offset, end), message)
    }

    fn span(&self, start: usize, end: usize) -> Span {
        Span {
            file_id: self.file_id,
            start,
            end,
        }
    }

    fn is_eof(&self) -> bool {
        self.offset >= self.input.len()
    }

    fn current_byte(&self) -> Option<u8> {
        self.input.as_bytes().get(self.offset).copied()
    }

    fn starts_with(&self, pattern: &str) -> bool {
        self.input[self.offset..].starts_with(pattern)
    }

    fn is_at_line_start(&self) -> bool {
        self.offset == 0
            || matches!(
                self.input.as_bytes().get(self.offset - 1),
                Some(b'\n' | b'\r')
            )
    }

    fn find_line_end(&self, start: usize) -> usize {
        self.input[start..]
            .find(['\r', '\n'])
            .map_or(self.input.len(), |relative| start + relative)
    }

    fn next_line_start(&self, line_end: usize) -> usize {
        if self.input[line_end..].starts_with("\r\n") {
            line_end + 2
        } else if self.input[line_end..].starts_with('\n')
            || self.input[line_end..].starts_with('\r')
        {
            line_end + 1
        } else {
            line_end
        }
    }
}

struct OpeningFence {
    backtick_count: usize,
    info: Option<String>,
}

fn parse_opening_fence(line: &str) -> Option<OpeningFence> {
    let trimmed = line.trim();

    let backtick_count = trimmed
        .as_bytes()
        .iter()
        .take_while(|&&byte| byte == b'`')
        .count();

    if backtick_count < 3 {
        return None;
    }

    // Everything after the fence is the info string, kept verbatim (only the
    // surrounding whitespace is trimmed). It is accepted and reserved, so
    // multi-word forms such as `rust ignore` are preserved as-is.
    let info = trimmed[backtick_count..].trim();

    Some(OpeningFence {
        backtick_count,
        info: (!info.is_empty()).then(|| info.to_owned()),
    })
}

fn is_closing_fence(line: &str, minimum_length: usize) -> bool {
    let trimmed = line.trim();

    let backtick_count = trimmed
        .as_bytes()
        .iter()
        .take_while(|&&byte| byte == b'`')
        .count();

    backtick_count >= minimum_length && backtick_count == trimmed.len()
}

fn is_ident_start(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_ident_continue(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::SourceMap;

    fn lex(input: &str) -> Result<Vec<Token>, LexError> {
        let mut map = SourceMap::new();
        let file_id = map.add_file("test.inka".into(), input.to_string());

        Lexer::new(file_id, input).lex()
    }

    fn kinds(tokens: &[Token]) -> Vec<&TokenKind> {
        tokens.iter().map(|token| &token.value).collect()
    }
    #[test]
    fn lexes_block_header_and_punctuation() {
        let tokens = lex("objective root #critical { children: child-a, child-b }")
            .expect("input should lex successfully");

        assert_eq!(
            kinds(&tokens),
            vec![
                &TokenKind::Objective,
                &TokenKind::Ident("root".to_owned()),
                &TokenKind::Tag("critical".to_owned()),
                &TokenKind::LeftBrace,
                &TokenKind::Ident("children".to_owned()),
                &TokenKind::Colon,
                &TokenKind::Ident("child-a".to_owned()),
                &TokenKind::Comma,
                &TokenKind::Ident("child-b".to_owned()),
                &TokenKind::RightBrace,
            ]
        );
    }

    #[test]
    fn recognizes_all_mode_keywords() {
        let tokens = lex("objective structure bind fiat").expect("input should lex successfully");

        assert_eq!(
            kinds(&tokens),
            vec![
                &TokenKind::Objective,
                &TokenKind::Structure,
                &TokenKind::Bind,
                &TokenKind::Fiat,
            ]
        );
    }

    fn comment(text: &str, trailing: bool, blank_line_after: bool) -> TokenKind {
        TokenKind::Comment {
            text: text.to_owned(),
            trailing,
            blank_line_after,
        }
    }

    #[test]
    fn emits_comments_outside_fences_and_marks_trailing_ones() {
        let tokens = lex(r"
// leading comment
objective root { // opening comment
    children: child //   indented  text
}
")
        .expect("input should lex successfully");

        assert_eq!(
            kinds(&tokens),
            vec![
                &comment("leading comment", false, false),
                &TokenKind::Objective,
                &TokenKind::Ident("root".to_owned()),
                &TokenKind::LeftBrace,
                &comment("opening comment", true, false),
                &TokenKind::Ident("children".to_owned()),
                &TokenKind::Colon,
                &TokenKind::Ident("child".to_owned()),
                &comment("  indented  text", true, false),
                &TokenKind::RightBrace,
            ]
        );
    }

    #[test]
    fn a_comment_token_spans_from_the_slashes_to_the_line_end() {
        let input = "//x  \r\nfiat";
        let tokens = lex(input).expect("input should lex successfully");

        assert_eq!(tokens[0].value, comment("x", false, false));
        assert_eq!(&input[tokens[0].span.start..tokens[0].span.end], "//x  ");
    }

    #[test]
    fn records_whether_a_blank_line_follows_a_comment() {
        let tokens = lex("// a\n// b\n  \n// c\nfiat").expect("input should lex successfully");

        assert_eq!(
            kinds(&tokens),
            vec![
                &comment("a", false, false),
                &comment("b", false, true),
                &comment("c", false, false),
                &TokenKind::Fiat,
            ]
        );
    }

    #[test]
    fn composes_comment_text_to_nfc() {
        // `e` followed by a combining acute composes to a single `é`.
        let tokens = lex("// re\u{301}sume\u{301}").expect("input should lex successfully");

        assert_eq!(tokens[0].value, comment("r\u{e9}sum\u{e9}", false, false));
    }

    #[test]
    fn preserves_comment_markers_and_braces_inside_fence() {
        let input = concat!(
            "objective root {\n",
            "```\n",
            "Text before // this is not a comment\n",
            "{ braces are ordinary text }\n",
            "```\n",
            "}\n",
        );

        let tokens = lex(input).expect("input should lex successfully");

        let fence = tokens
            .iter()
            .find_map(|token| match &token.value {
                TokenKind::FenceBlock {
                    raw,
                    info,
                    content_span,
                    ..
                } => Some((raw, info, content_span)),
                _ => None,
            })
            .expect("a fence token should be emitted");

        assert_eq!(
            fence.0,
            concat!(
                "Text before // this is not a comment\n",
                "{ braces are ordinary text }\n",
            )
        );
        assert_eq!(fence.1, &None);
        assert_eq!(&input[fence.2.start..fence.2.end], fence.0);
    }

    #[test]
    fn lexes_fence_info_string() {
        let tokens = lex(concat!(
            "objective root {\n",
            "```md\n",
            "description\n",
            "```\n",
            "}\n",
        ))
        .expect("input should lex successfully");

        let info = tokens.iter().find_map(|token| match &token.value {
            TokenKind::FenceBlock { info, .. } => Some(info),
            _ => None,
        });

        assert_eq!(info, Some(&Some("md".to_owned())));
    }

    #[test]
    fn longer_fence_may_close_shorter_fence() {
        let tokens = lex(concat!("```\n", "description\n", "````\n",))
            .expect("a longer fence should close the block");

        assert!(matches!(
            tokens.as_slice(),
            [Spanned {
                value: TokenKind::FenceBlock { raw, .. },
                ..
            }] if raw == "description\n"
        ));
    }

    #[test]
    fn shorter_fence_does_not_close_longer_fence() {
        let tokens = lex(concat!(
            "````\n",
            "first line\n",
            "```\n",
            "second line\n",
            "````\n",
        ))
        .expect("the final fence should close the block");

        assert!(matches!(
            tokens.as_slice(),
            [Spanned {
                value: TokenKind::FenceBlock { raw, fence_length: 4, .. },
                ..
            }] if raw == concat!(
                "first line\n",
                "```\n",
                "second line\n",
            )
        ));
    }

    #[test]
    fn rejects_unclosed_fence() {
        let error = lex(concat!("```\n", "description\n",))
            .expect_err("an unclosed fence should be rejected");

        assert_eq!(error.message, "unclosed fenced description");
        let span = error.span.expect("lex errors are anchored");
        assert_eq!(span.start, 0);
        assert_eq!(span.end, "```\ndescription\n".len());
    }

    #[test]
    fn preserves_interpolation_syntax_inside_fence() {
        let tokens = lex(concat!("```\n", "Value is {$value}.\n", "```\n",))
            .expect("interpolation syntax should remain raw text");

        assert!(matches!(
            tokens.as_slice(),
            [Spanned {
                value: TokenKind::FenceBlock { raw, .. },
                ..
            }] if raw == "Value is {$value}.\n"
        ));
    }

    #[test]
    fn lexes_string_literal_without_quotes() {
        let tokens = lex(r#"test: "tests/assumption.rs""#).expect("input should lex successfully");

        assert_eq!(
            kinds(&tokens),
            vec![
                &TokenKind::Ident("test".to_owned()),
                &TokenKind::Colon,
                &TokenKind::StringLiteral("tests/assumption.rs".to_owned()),
            ]
        );
    }

    #[test]
    fn rejects_unclosed_string_literal() {
        let input = r#"test: "tests/assumption.rs"#;

        let error = lex(input).expect_err("an unclosed string literal should be rejected");

        assert_eq!(error.message, "unclosed string literal");
        let span = error.span.expect("lex errors are anchored");
        assert_eq!(span.start, "test: ".len());
        assert_eq!(span.end, input.len());
    }

    #[test]
    fn rejects_multiline_string_literal() {
        let error = lex(concat!("test: \"first line\n", "second line\"\n",))
            .expect_err("string literals must remain on one line");

        assert_eq!(error.message, "string literal must end on the same line");
    }

    #[test]
    fn rejects_tag_without_name() {
        let error = lex("objective root # {").expect_err("a tag must have a name");

        assert_eq!(error.message, "expected a tag name after `#`");
    }

    #[test]
    fn lexes_double_colon_as_one_token() {
        let tokens = lex("namespace::item").expect("input should lex successfully");

        assert_eq!(
            kinds(&tokens),
            vec![
                &TokenKind::Ident("namespace".to_owned()),
                &TokenKind::DoubleColon,
                &TokenKind::Ident("item".to_owned()),
            ]
        );
    }

    #[test]
    fn records_half_open_token_spans() {
        let input = "objective root";

        let tokens = lex(input).expect("input should lex successfully");

        assert_eq!(
            &input[tokens[0].span.start..tokens[0].span.end],
            "objective"
        );
        assert_eq!(&input[tokens[1].span.start..tokens[1].span.end], "root");

        assert_eq!(tokens[0].span.start, 0);
        assert_eq!(tokens[0].span.end, 9);

        assert_eq!(tokens[1].span.start, 10);
        assert_eq!(tokens[1].span.end, 14);
    }

    #[test]
    fn fence_span_includes_delimiters_but_content_span_does_not() {
        let input = concat!("```md\n", "hello\n", "```\n",);

        let tokens = lex(input).expect("input should lex successfully");

        let token = tokens.first().expect("a fence token should exist");

        let TokenKind::FenceBlock {
            raw, content_span, ..
        } = &token.value
        else {
            panic!("expected a fence token");
        };

        assert_eq!(&input[token.span.start..token.span.end], input);
        assert_eq!(&input[content_span.start..content_span.end], "hello\n");
        assert_eq!(raw, "hello\n");
    }

    #[test]
    fn handles_crlf_without_corrupting_spans() {
        let input = concat!(
            "objective root {\r\n",
            "```md\r\n",
            "description\r\n",
            "```\r\n",
            "}\r\n",
        );

        let tokens = lex(input).expect("CRLF input should lex successfully");

        assert_eq!(tokens.len(), 5);
        assert!(matches!(tokens[0].value, TokenKind::Objective));
        assert_eq!(tokens[1].value, TokenKind::Ident("root".to_owned()));
        assert!(matches!(tokens[2].value, TokenKind::LeftBrace));
        assert!(matches!(tokens[4].value, TokenKind::RightBrace));

        let TokenKind::FenceBlock {
            raw,
            info,
            content_span,
            ..
        } = &tokens[3].value
        else {
            panic!("expected a fence token");
        };

        assert_eq!(raw, "description\r\n");
        assert_eq!(info.as_deref(), Some("md"));
        assert_eq!(&input[content_span.start..content_span.end], raw);

        for token in &tokens {
            assert!(token.span.start <= token.span.end);
            assert!(token.span.end <= input.len());
        }
    }

    #[test]
    fn handles_cr_only_line_endings() {
        let input = concat!(
            "objective root {\r",
            "```md\r",
            "description\r",
            "```\r",
            "}\r",
        );

        let tokens = lex(input).expect("CR-only input should lex successfully");

        assert_eq!(tokens.len(), 5);
        assert!(matches!(tokens[0].value, TokenKind::Objective));
        assert_eq!(tokens[1].value, TokenKind::Ident("root".to_owned()));
        assert!(matches!(tokens[2].value, TokenKind::LeftBrace));
        assert!(matches!(tokens[4].value, TokenKind::RightBrace));

        let TokenKind::FenceBlock {
            raw,
            info,
            content_span,
            ..
        } = &tokens[3].value
        else {
            panic!("expected a fence token");
        };

        assert_eq!(raw, "description\r");
        assert_eq!(info.as_deref(), Some("md"));
        assert_eq!(&input[content_span.start..content_span.end], raw);
    }

    #[test]
    fn rejects_unrecognized_character() {
        let error = lex("objective root @").expect_err("unknown characters should be rejected");

        assert_eq!(error.message, "unexpected character");
        let span = error.span.expect("lex errors are anchored");
        assert_eq!(span.start, "objective root ".len());
        assert_eq!(span.end, "objective root @".len());
    }

    #[test]
    fn lexes_test_source_target_with_slash() {
        let tokens =
            lex(r#"test: "tests/hash_props.rs""#).expect("a quoted test target should lex");

        assert_eq!(
            kinds(&tokens),
            vec![
                &TokenKind::Ident("test".to_owned()),
                &TokenKind::Colon,
                &TokenKind::StringLiteral("tests/hash_props.rs".to_owned()),
            ]
        );
    }

    #[test]
    fn lexes_calc_source_target() {
        let tokens =
            lex(r#"calc: "AOCS::?verify_attitude""#).expect("a quoted calc target should lex");

        assert_eq!(
            kinds(&tokens),
            vec![
                &TokenKind::Ident("calc".to_owned()),
                &TokenKind::Colon,
                &TokenKind::StringLiteral("AOCS::?verify_attitude".to_owned()),
            ]
        );
    }

    #[test]
    fn lexes_empty_input_to_no_tokens() {
        let tokens = lex("").expect("empty input should lex");

        assert_eq!(tokens, []);
    }

    #[test]
    fn spans_non_ascii_character_on_char_boundary() {
        let input = "objective √";

        let error = lex(input).expect_err("a non-ASCII stray char should be rejected");

        assert_eq!(error.message, "unexpected character");
        // The span must stay on char boundaries so a renderer can slice it.
        let span = error.span.expect("lex errors are anchored");
        assert_eq!(&input[span.start..span.end], "√");
    }

    #[test]
    fn lexes_tab_indented_fence() {
        let tokens = lex(concat!("\t```md\n", "\tdescription\n", "\t```\n"))
            .expect("a tab-indented fence should lex");

        assert!(
            tokens
                .iter()
                .any(|token| matches!(token.value, TokenKind::FenceBlock { .. }))
        );
    }

    #[test]
    fn preserves_multi_word_fence_info_string() {
        let tokens = lex(concat!("```rust ignore\n", "x\n", "```\n"))
            .expect("a multi-word fence info string should be accepted");

        let info = tokens.iter().find_map(|token| match &token.value {
            TokenKind::FenceBlock { info, .. } => Some(info.clone()),
            _ => None,
        });

        assert_eq!(info, Some(Some("rust ignore".to_owned())));
    }
}
