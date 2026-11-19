//! Completion: what may be typed at the cursor, from the text around it.
//!
//! Completion runs while the text is being written, which is exactly when it
//! does not parse — a `children:` list ending in a comma, a block without its
//! required properties yet. So unlike every other request this one never
//! loads the project. The context comes from lexing the text *before* the
//! cursor, which is well-formed far more often than the whole file; the
//! candidates come from reading each input on its own (manifest, values,
//! every argument file's tokens), so one broken file costs only its own
//! candidates.
//!
//! What is offered, by position:
//!
//! - between blocks: the mode keywords;
//! - where a property may start: the properties the block's mode allows and
//!   it does not have yet;
//! - after `children:` or a `,` in that list: the ids of other blocks, minus
//!   the objective and the ones already listed;
//! - after `until:`: the phases of `phases.order`;
//! - after `promote:`: the modes the block may promote to;
//! - inside a description, after `{$`: the value keys.
//!
//! Anything else — a new block id, a tag, a `test:` string, a comment, plain
//! description text — answers nothing, so the editor's own word suggestions
//! stay out of the way of free text only.

use std::path::Path;

use lsp_types::{
    CompletionItem, CompletionItemKind, CompletionTextEdit, Documentation, MarkupContent,
    MarkupKind, Range, TextEdit,
};

use inka_core::lexer::{Lexer, Token, TokenKind};
use inka_core::manifest::{Manifest, parse_manifest};
use inka_core::normalize;
use inka_core::read::ReadText;
use inka_core::source::{FileId, SourceFile, SourceMap};
use inka_core::values::load_values;

use crate::position;

/// The characters that open a completion by themselves: the colon ending a
/// property name, the comma between children, and the `$` of `{$`.
pub const TRIGGER_CHARACTERS: [&str; 3] = [":", ",", "$"];

/// The lexer's message for a fence the text has not closed — before the
/// cursor, the sign that the cursor sits inside a description.
const UNCLOSED_FENCE: &str = "unclosed fenced description";

/// The completions for `offset` in the document at `path`, whose current
/// text is `text`, in the project rooted at `root`. Each one replaces the
/// part of the word already typed before the cursor.
pub fn complete(
    root: &Path,
    path: &Path,
    text: &str,
    offset: usize,
    reader: &dyn ReadText,
) -> Vec<CompletionItem> {
    let Some(prefix) = text.get(..offset) else {
        return Vec::new();
    };
    let Some((context, start)) = context(prefix) else {
        return Vec::new();
    };

    let mut map = SourceMap::new();
    let file_id = map.add_file(path.to_path_buf(), text.to_owned());
    let file = map.file(file_id).expect("the file was just added");
    let range = Range::new(
        position::position(file, start),
        position::position(file, offset),
    );

    let candidates = match context {
        Context::Keyword => keywords(),
        Context::Property { mode, present } => property_names(mode, &present),
        Context::Child { own, listed } => children(
            &inventory(root, path, file, offset, reader),
            own.as_deref(),
            &listed,
        ),
        Context::Phase => phases(root, reader),
        Context::Promote(mode) => promote_targets(mode)
            .iter()
            .map(|target| Candidate::new(target, CompletionItemKind::KEYWORD))
            .collect(),
        // Close the reference unless the editor already did (VS Code pairs
        // `{` with `}` as it is typed).
        Context::Value => value_keys(root, reader, !text[offset..].starts_with('}')),
    };
    candidates
        .into_iter()
        .map(|candidate| candidate.into_item(range))
        .collect()
}

/// One completion before it is placed in the text.
struct Candidate {
    label: String,
    kind: CompletionItemKind,
    detail: Option<String>,
    documentation: Option<String>,
    /// The order key, when the label's own order is not the one to show.
    sort: Option<String>,
    /// What is inserted, when it is not the label.
    insert: Option<String>,
}

impl Candidate {
    fn new(label: &str, kind: CompletionItemKind) -> Self {
        Self {
            label: label.to_owned(),
            kind,
            detail: None,
            documentation: None,
            sort: None,
            insert: None,
        }
    }

    fn into_item(self, range: Range) -> CompletionItem {
        CompletionItem {
            text_edit: Some(CompletionTextEdit::Edit(TextEdit {
                range,
                new_text: self.insert.unwrap_or_else(|| self.label.clone()),
            })),
            label: self.label,
            kind: Some(self.kind),
            detail: self.detail,
            documentation: self.documentation.map(|value| {
                Documentation::MarkupContent(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value,
                })
            }),
            sort_text: self.sort,
            ..CompletionItem::default()
        }
    }
}

fn keywords() -> Vec<Candidate> {
    ["objective", "structure", "bind", "fiat"]
        .into_iter()
        .map(|keyword| Candidate::new(keyword, CompletionItemKind::KEYWORD))
        .collect()
}

/// The properties `mode` allows that `present` does not rule out, each
/// inserted with its colon.
fn property_names(mode: Mode, present: &[String]) -> Vec<Candidate> {
    properties(mode)
        .iter()
        .enumerate()
        .filter(|(_, property)| {
            !property
                .excluded_by
                .iter()
                .any(|excluded| present.iter().any(|name| name == excluded))
        })
        .map(|(index, property)| Candidate {
            detail: Some(property.detail.to_owned()),
            sort: Some(format!("{index:02}")),
            insert: Some(format!("{}: ", property.name)),
            ..Candidate::new(property.name, CompletionItemKind::PROPERTY)
        })
        .collect()
}

/// The declared blocks a `children:` entry of `own` may still name: not the
/// objective (the root has no parent), not `own` itself, not one `listed`.
fn children(declared: &[Declared], own: Option<&str>, listed: &[String]) -> Vec<Candidate> {
    declared
        .iter()
        .filter(|block| block.mode != Mode::Objective)
        .filter(|block| own != Some(block.id.as_str()))
        .filter(|block| !listed.contains(&block.id))
        .map(|block| Candidate {
            detail: Some(block.mode.keyword().to_owned()),
            documentation: block.description.clone(),
            ..Candidate::new(&block.id, CompletionItemKind::REFERENCE)
        })
        .collect()
}

/// The phases of `phases.order`, in that order, the current one marked.
fn phases(root: &Path, reader: &dyn ReadText) -> Vec<Candidate> {
    let Some(manifest) = manifest(root, reader) else {
        return Vec::new();
    };
    let current = &manifest.phases.current;
    manifest
        .phases
        .order
        .iter()
        .enumerate()
        .map(|(index, phase)| Candidate {
            detail: Some(if phase == current {
                "phase (current)".to_owned()
            } else {
                "phase".to_owned()
            }),
            sort: Some(format!("{index:04}")),
            ..Candidate::new(phase, CompletionItemKind::ENUM_MEMBER)
        })
        .collect()
}

/// The keys of the project's values, each showing its value; `close` adds
/// the `}` that ends the reference.
fn value_keys(root: &Path, reader: &dyn ReadText, close: bool) -> Vec<Candidate> {
    let Some(manifest) = manifest(root, reader) else {
        return Vec::new();
    };
    let Ok(values) = load_values(root, &manifest.values.files, reader) else {
        return Vec::new();
    };
    let mut values: Vec<(String, String)> = values.into_iter().collect();
    values.sort();
    values
        .into_iter()
        .map(|(key, value)| Candidate {
            detail: Some(value),
            insert: Some(if close {
                format!("{key}}}")
            } else {
                key.clone()
            }),
            ..Candidate::new(&key, CompletionItemKind::VARIABLE)
        })
        .collect()
}

/// What the cursor position takes.
#[derive(Debug, PartialEq, Eq)]
enum Context {
    /// A mode keyword opening a new block.
    Keyword,
    /// A property name in a block of `mode` that already has `present`.
    Property { mode: Mode, present: Vec<String> },
    /// A `children:` entry of the block `own` (when its id was written),
    /// which already lists `listed`.
    Child {
        own: Option<String>,
        listed: Vec<String>,
    },
    /// The phase after `until:`.
    Phase,
    /// The target after `promote:` in a block of the given mode.
    Promote(Mode),
    /// A value key after `{$` in a description.
    Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Objective,
    Structure,
    Bind,
    Fiat,
}

impl Mode {
    fn of(kind: &TokenKind) -> Option<Self> {
        match kind {
            TokenKind::Objective => Some(Self::Objective),
            TokenKind::Structure => Some(Self::Structure),
            TokenKind::Bind => Some(Self::Bind),
            TokenKind::Fiat => Some(Self::Fiat),
            _ => None,
        }
    }

    fn keyword(self) -> &'static str {
        match self {
            Self::Objective => "objective",
            Self::Structure => "structure",
            Self::Bind => "bind",
            Self::Fiat => "fiat",
        }
    }
}

/// One property a mode allows, and the present properties that rule it out:
/// itself, and for `test:`/`veriq:` the other one too (a bind takes exactly
/// one).
struct Property {
    name: &'static str,
    detail: &'static str,
    excluded_by: &'static [&'static str],
}

const CHILDREN: Property = Property {
    name: "children",
    detail: "the nodes this one is discharged by",
    excluded_by: &["children"],
};
const UNTIL: Property = Property {
    name: "until",
    detail: "the phase this node expires at",
    excluded_by: &["until"],
};
const PROMOTE: Property = Property {
    name: "promote",
    detail: "the mode this node is to become at its deadline (with until:)",
    excluded_by: &["promote"],
};
const TEST: Property = Property {
    name: "test",
    detail: "the test this bind is discharged by",
    excluded_by: &["test", "veriq"],
};
const VERIQ: Property = Property {
    name: "veriq",
    detail: "the veriq verification this bind is discharged by",
    excluded_by: &["test", "veriq"],
};

/// The properties of a mode, in the order they are offered.
fn properties(mode: Mode) -> &'static [Property] {
    match mode {
        Mode::Objective => &[CHILDREN, UNTIL, PROMOTE],
        Mode::Structure => &[CHILDREN, UNTIL],
        Mode::Bind => &[TEST, VERIQ, UNTIL],
        Mode::Fiat => &[UNTIL, PROMOTE],
    }
}

/// The modes `promote:` may name in a block of `mode`.
fn promote_targets(mode: Mode) -> &'static [&'static str] {
    match mode {
        Mode::Objective => &["structure"],
        Mode::Fiat => &["structure", "bind"],
        Mode::Structure | Mode::Bind => &[],
    }
}

/// The context of the end of `prefix` (the text before the cursor), and the
/// offset where the word being typed starts — what a completion replaces.
fn context(prefix: &str) -> Option<(Context, usize)> {
    let start = prefix.len()
        - prefix
            .bytes()
            .rev()
            .take_while(|&byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            .count();

    let tokens = match Lexer::new(scratch_file(), prefix).lex() {
        Ok(tokens) => tokens,
        Err(error) if error.message == UNCLOSED_FENCE => {
            return value_reference(prefix).map(|start| (Context::Value, start));
        }
        Err(_) => return None,
    };

    let mut tokens = tokens.as_slice();
    if let [rest @ .., last] = tokens
        && last.span.end == prefix.len()
    {
        let word = last.span.start == start
            && (matches!(last.value, TokenKind::Ident(_)) || Mode::of(&last.value).is_some());
        // A description's token runs through the newline after its closing
        // fence, so a cursor at the start of the next line touches it.
        let opens = matches!(
            last.value,
            TokenKind::Colon
                | TokenKind::Comma
                | TokenKind::LeftBrace
                | TokenKind::FenceBlock { .. }
        );
        if word {
            tokens = rest;
        } else if !opens {
            // The cursor touches a token that takes no completion: a tag, a
            // string, a comment, a brace, `::`.
            return None;
        }
    }
    walk(tokens).map(|context| (context, start))
}

/// After `{$` and an unfinished value path on the cursor's line, the offset
/// where the path starts.
///
/// The whole dotted path is the word being typed, so `{$instrument.d`
/// replaces from `instrument` and filters against keys like
/// `instrument.diameter_cm`.
fn value_reference(prefix: &str) -> Option<usize> {
    let line = &prefix[prefix.rfind('\n').map_or(0, |newline| newline + 1)..];
    let at = line.rfind("{$")?;
    let path = &line[at + 2..];
    let valid = path.split('.').enumerate().all(|(index, segment)| {
        segment
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
            && match segment.bytes().next() {
                Some(first) => first.is_ascii_alphabetic() || first == b'_',
                // Only the segment being typed may still be empty.
                None => index == path.split('.').count() - 1,
            }
    });
    valid.then_some(prefix.len() - path.len())
}

/// Where the token stream leaves the cursor.
fn walk(tokens: &[Token]) -> Option<Context> {
    /// Where the walk is.
    enum State {
        /// Between blocks; `clean` is false after a token no block starts
        /// with.
        Between { clean: bool },
        /// In a block header, after the mode keyword (and the id, if any).
        Header { mode: Mode, id: Option<String> },
        /// Inside a block's braces.
        Body(Body),
    }

    let mut state = State::Between { clean: true };
    for token in tokens {
        let kind = &token.value;
        if matches!(kind, TokenKind::Comment { .. }) {
            continue;
        }
        state = match state {
            State::Between { .. } => match Mode::of(kind) {
                Some(mode) => State::Header { mode, id: None },
                None => State::Between {
                    clean: matches!(kind, TokenKind::RightBrace),
                },
            },
            State::Header { mode, id } => match kind {
                TokenKind::LeftBrace => State::Body(Body::new(mode, id)),
                TokenKind::Ident(name) if id.is_none() => State::Header {
                    mode,
                    id: Some(name.clone()),
                },
                _ => State::Header { mode, id },
            },
            State::Body(_) if matches!(kind, TokenKind::RightBrace) => {
                State::Between { clean: true }
            }
            State::Body(mut body) => {
                body.step(kind);
                State::Body(body)
            }
        };
    }

    match state {
        State::Between { clean: true } => Some(Context::Keyword),
        State::Between { clean: false } | State::Header { .. } => None,
        State::Body(body) => body.context(),
    }
}

/// The walk inside one block's braces.
struct Body {
    mode: Mode,
    id: Option<String>,
    /// The properties written so far.
    present: Vec<String>,
    /// The `children:` entries written so far.
    listed: Vec<String>,
    /// The property whose value is being read.
    property: Option<String>,
    last: Last,
}

/// The last token seen inside a block, in the terms that decide what may
/// follow it.
#[derive(Clone, PartialEq, Eq)]
enum Last {
    /// `{`, a description, or a finished property value: a property may
    /// start.
    Boundary,
    /// An identifier where a property name goes.
    Name(String),
    /// The colon after a property name.
    Colon(String),
    /// A comma in a `children:` list.
    Comma,
    /// A value of the current property.
    Value,
    /// Something this walk does not follow.
    Other,
}

impl Body {
    fn new(mode: Mode, id: Option<String>) -> Self {
        Self {
            mode,
            id,
            present: Vec::new(),
            listed: Vec::new(),
            property: None,
            last: Last::Boundary,
        }
    }

    fn in_children(&self) -> bool {
        self.property.as_deref() == Some("children")
    }

    /// Takes one token other than the closing brace.
    fn step(&mut self, kind: &TokenKind) {
        let after_colon = matches!(self.last, Last::Colon(_));
        self.last = match kind {
            TokenKind::FenceBlock { .. } => {
                self.property = None;
                Last::Boundary
            }
            TokenKind::Ident(name) => match self.last {
                Last::Colon(_) | Last::Comma => {
                    if self.in_children() {
                        self.listed.push(name.clone());
                    }
                    Last::Value
                }
                Last::Boundary | Last::Value => Last::Name(name.clone()),
                _ => Last::Other,
            },
            TokenKind::Colon => match std::mem::replace(&mut self.last, Last::Other) {
                Last::Name(name) => {
                    self.present.push(name.clone());
                    self.property = Some(name.clone());
                    Last::Colon(name)
                }
                _ => Last::Other,
            },
            TokenKind::Comma if self.last == Last::Value && self.in_children() => Last::Comma,
            // `promote:` takes a mode, which lexes as its keyword.
            TokenKind::StringLiteral(_) if after_colon => Last::Value,
            _ if after_colon && Mode::of(kind).is_some() => Last::Value,
            _ => Last::Other,
        };
    }

    fn context(self) -> Option<Context> {
        match self.last {
            // A finished `children:` entry may be followed by a property
            // just as well as by the comma the user has not typed.
            Last::Boundary | Last::Value => Some(Context::Property {
                mode: self.mode,
                present: self.present,
            }),
            Last::Comma => Some(Context::Child {
                own: self.id,
                listed: self.listed,
            }),
            Last::Colon(property) => match property.as_str() {
                "children" => Some(Context::Child {
                    own: self.id,
                    listed: self.listed,
                }),
                "until" => Some(Context::Phase),
                "promote" => Some(Context::Promote(self.mode)),
                _ => None,
            },
            Last::Name(_) | Last::Other => None,
        }
    }
}

/// A block an argument file declares, as far as its tokens tell.
struct Declared {
    mode: Mode,
    id: String,
    /// The block's description, normalized, when it has one.
    description: Option<String>,
}

/// Every block declared under `argument/`. Read from tokens, not parsed: a
/// block still missing a required property is a perfectly good child to
/// offer. The open document is `file` (its current text); when the whole of
/// it does not lex, the part before the cursor still does more often than
/// not.
fn inventory(
    root: &Path,
    path: &Path,
    file: &SourceFile,
    offset: usize,
    reader: &dyn ReadText,
) -> Vec<Declared> {
    let Ok(entries) = std::fs::read_dir(root.join("argument")) else {
        return Vec::new();
    };
    let mut paths: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|candidate| candidate.extension().is_some_and(|ext| ext == "inka"))
        .collect();
    if !paths.iter().any(|candidate| candidate == path) {
        paths.push(path.to_path_buf());
    }
    paths.sort();

    let mut declared = Vec::new();
    for candidate in paths {
        let tokens = if candidate == path {
            Lexer::new(scratch_file(), &file.text)
                .lex()
                .or_else(|_| Lexer::new(scratch_file(), &file.text[..offset]).lex())
        } else {
            match reader.read_text(&candidate) {
                Ok(text) => Lexer::new(scratch_file(), &text).lex(),
                Err(_) => continue,
            }
        };
        if let Ok(tokens) = tokens {
            declarations(&tokens, &mut declared);
        }
    }
    declared
}

/// The blocks a token stream declares: a mode keyword between blocks and the
/// identifier after it, with the first description inside its braces.
fn declarations(tokens: &[Token], into: &mut Vec<Declared>) {
    let mut depth = 0usize;
    let mut pending: Option<Mode> = None;
    // The block whose braces the walk is in, when its header named an id.
    let mut current: Option<usize> = None;
    for token in tokens {
        match &token.value {
            TokenKind::LeftBrace => depth += 1,
            TokenKind::RightBrace => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    current = None;
                }
            }
            TokenKind::Ident(id) if depth == 0 => {
                if let Some(mode) = pending.take() {
                    current = Some(into.len());
                    into.push(Declared {
                        mode,
                        id: id.clone(),
                        description: None,
                    });
                }
                continue;
            }
            TokenKind::FenceBlock { raw, .. } if depth == 1 => {
                if let Some(block) = current.and_then(|index| into.get_mut(index))
                    && block.description.is_none()
                {
                    block.description = Some(normalize::description(raw));
                }
            }
            kind if depth == 0 => {
                pending = Mode::of(kind);
                continue;
            }
            _ => {}
        }
        pending = None;
    }
}

/// A file id for lexing text that no source map holds: tokens only carry it
/// along, and nothing here resolves it.
fn scratch_file() -> FileId {
    SourceMap::new().add_file(std::path::PathBuf::new(), String::new())
}

/// The project's manifest, when it reads and parses.
fn manifest(root: &Path, reader: &dyn ReadText) -> Option<Manifest> {
    let text = reader.read_text(&root.join("inka.toml")).ok()?;
    parse_manifest(&text).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at_end(prefix: &str) -> Option<Context> {
        context(prefix).map(|(context, _)| context)
    }

    #[allow(clippy::unnecessary_wraps)] // compared against `at_end`
    fn property(mode: Mode, present: &[&str]) -> Option<Context> {
        Some(Context::Property {
            mode,
            present: present.iter().map(|name| (*name).to_owned()).collect(),
        })
    }

    #[allow(clippy::unnecessary_wraps)] // compared against `at_end`
    fn child(own: &str, listed: &[&str]) -> Option<Context> {
        Some(Context::Child {
            own: Some(own.to_owned()),
            listed: listed.iter().map(|name| (*name).to_owned()).collect(),
        })
    }

    const HEAD: &str = "structure s {\n    ```\n    text\n    ```\n";

    #[test]
    fn an_empty_file_and_the_gap_after_a_block_take_a_keyword() {
        assert_eq!(at_end(""), Some(Context::Keyword));
        assert_eq!(
            at_end("fiat f {\n  ```\n  t\n  ```\n}\n\n"),
            Some(Context::Keyword)
        );
        assert_eq!(at_end("// note\nstru"), Some(Context::Keyword));
    }

    #[test]
    fn a_header_takes_nothing() {
        assert_eq!(at_end("fiat "), None);
        assert_eq!(at_end("fiat new_id"), None);
        assert_eq!(at_end("fiat f #ta"), None);
    }

    #[test]
    fn after_the_description_a_property_starts() {
        assert_eq!(
            at_end(&format!("{HEAD}    ")),
            property(Mode::Structure, &[])
        );
        assert_eq!(at_end(HEAD), property(Mode::Structure, &[]));
        assert_eq!(at_end("fiat f {"), property(Mode::Fiat, &[]));
        assert_eq!(
            at_end(&format!("{HEAD}    chi")),
            property(Mode::Structure, &[])
        );
    }

    #[test]
    fn present_properties_are_reported() {
        let text = format!("{HEAD}    until: P0\n    ");
        assert_eq!(at_end(&text), property(Mode::Structure, &["until"]));
    }

    #[test]
    fn children_take_ids_after_the_colon_and_each_comma() {
        assert_eq!(at_end(&format!("{HEAD}    children:")), child("s", &[]));
        assert_eq!(at_end(&format!("{HEAD}    children: le")), child("s", &[]));
        assert_eq!(
            at_end(&format!(
                "{HEAD}    children:\n        a,\n        b,\n        "
            )),
            child("s", &["a", "b"])
        );
    }

    #[test]
    fn a_child_without_a_comma_may_be_followed_by_a_property() {
        let text = format!("{HEAD}    children: a\n    ");
        assert_eq!(at_end(&text), property(Mode::Structure, &["children"]));
    }

    #[test]
    fn until_takes_phases_and_promote_takes_modes() {
        let fiat = "fiat f {\n  ```\n  t\n  ```\n  until: P0\n  promote: ";
        assert_eq!(at_end(fiat), Some(Context::Promote(Mode::Fiat)));
        assert_eq!(
            at_end("fiat f {\n  ```\n  t\n  ```\n  until: "),
            Some(Context::Phase)
        );
        assert_eq!(
            at_end("fiat f {\n  ```\n  t\n  ```\n  promote: bi"),
            Some(Context::Promote(Mode::Fiat))
        );
    }

    #[test]
    fn after_a_promote_target_a_property_starts() {
        let fiat = "fiat f {\n  ```\n  t\n  ```\n  promote: bind\n  ";
        assert_eq!(at_end(fiat), property(Mode::Fiat, &["promote"]));
    }

    #[test]
    fn strings_comments_and_tags_take_nothing() {
        assert_eq!(at_end("bind b {\n  ```\n  t\n  ```\n  test: \"x"), None);
        assert_eq!(at_end("bind b {\n  ```\n  t\n  ```\n  test: "), None);
        assert_eq!(at_end(&format!("{HEAD}    // chi")), None);
    }

    #[test]
    fn a_description_takes_only_value_keys() {
        assert_eq!(at_end("fiat f {\n  ```\n  plain wor"), None);
        let (context, start) = context("fiat f {\n  ```\n  mass {$ma").expect("a context");
        assert_eq!(context, Context::Value);
        assert_eq!(start, "fiat f {\n  ```\n  mass {$".len());
        assert_eq!(at_end("fiat f {\n  ```\n  {$"), Some(Context::Value));
        assert_eq!(at_end("fiat f {\n  ```\n  {$a b"), None);
    }

    #[test]
    fn a_dotted_value_path_is_one_word() {
        let (context, start) = context("fiat f {\n  ```\n  d {$instrument.di").expect("a context");
        assert_eq!(context, Context::Value);
        assert_eq!(start, "fiat f {\n  ```\n  d {$".len());
        assert_eq!(
            at_end("fiat f {\n  ```\n  {$instrument."),
            Some(Context::Value)
        );
        assert_eq!(at_end("fiat f {\n  ```\n  {$.a"), None);
        assert_eq!(at_end("fiat f {\n  ```\n  {$a..b"), None);
        assert_eq!(at_end("fiat f {\n  ```\n  {$a.1"), None);
    }

    #[test]
    fn declarations_come_from_tokens_even_when_a_block_is_incomplete() {
        let text = "structure half {\n  ```\n  Half written\n  ```\n}\n\nfiat f {\n  ```\n  t\n  ```\n  promote: structure\n  until: P0\n}\n";
        let tokens = Lexer::new(scratch_file(), text).lex().expect("lexes");
        let mut declared = Vec::new();

        declarations(&tokens, &mut declared);

        let found: Vec<(Mode, &str, Option<&str>)> = declared
            .iter()
            .map(|block| (block.mode, block.id.as_str(), block.description.as_deref()))
            .collect();
        assert_eq!(
            found,
            [
                (Mode::Structure, "half", Some("Half written")),
                (Mode::Fiat, "f", Some("t")),
            ]
        );
    }
}
