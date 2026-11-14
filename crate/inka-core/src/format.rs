//! The canonical layout of a `.inka` file, as `formatting.md` defines it.
//!
//! Formatting is printing the syntax tree back out: the file is lexed and
//! parsed, and every element is written in a fixed place — comments directly
//! above their anchor, properties in a fixed order, one child entry per line.
//! Nothing is carried over from the source layout except what the tree holds,
//! so the output depends only on the tree, which makes formatting idempotent.
//!
//! Because comments are part of the tree and each one is written where the
//! attachment rules read it back from, reparsing the output yields the same
//! tree. Fence content is the one place the text itself changes; see
//! [`fence`] for how that is kept from changing a view hash.

mod fence;

use std::collections::HashMap;
use std::hash::BuildHasher;

use crate::ast::{
    Block, BlockCommon, ChildEntry, Comment, Deadline, FiatPromoteTarget, ObjectivePromoteTarget,
    SourceRef,
};
use crate::lexer::Lexer;
use crate::parser::{Parse, parse};
use crate::source::{Error, FileId};

/// One level of indentation.
const INDENT: &str = "    ";

/// Formats one `.inka` source text into the canonical layout.
///
/// `values` are the project's values, which decide whether re-indenting a
/// fence would change its contract and so must be skipped.
///
/// # Errors
///
/// Returns the lexer or parser errors of a text that does not parse; such a
/// text is never formatted. As a last line of defence, the output is parsed
/// again too, and a text whose output would not parse is refused with an
/// error rather than handed back to be written over the original.
pub fn format_source<S: BuildHasher>(
    file_id: FileId,
    text: &str,
    values: &HashMap<String, String, S>,
) -> Result<String, Vec<Error>> {
    let parsed = lex_and_parse(file_id, text)?;
    let formatted = print(&parsed, values);
    if lex_and_parse(file_id, &formatted).is_err() {
        return Err(vec![Error::unanchored(
            "formatting this file would produce text that does not parse, so it is left \
             as written; this is a bug in the formatter",
        )]);
    }
    Ok(formatted)
}

fn lex_and_parse(file_id: FileId, text: &str) -> Result<Parse, Vec<Error>> {
    let tokens = Lexer::new(file_id, text)
        .lex()
        .map_err(|error| vec![error])?;
    let parsed = parse(file_id, &tokens);
    if parsed.errors.is_empty() {
        Ok(parsed)
    } else {
        Err(parsed.errors)
    }
}

/// Prints an error-free parse in the canonical layout.
fn print<S: BuildHasher>(parsed: &Parse, values: &HashMap<String, String, S>) -> String {
    let mut out = Printer::default();
    if let Some(comment) = &parsed.comment {
        out.comment(0, Some(comment));
        if !parsed.blocks.is_empty() {
            out.blank_line();
        }
    }
    for (index, block) in parsed.blocks.iter().enumerate() {
        if index > 0 {
            out.blank_line();
        }
        out.block(&Body::of(block), values);
    }
    out.text
}

/// A block's printable parts, whatever its mode.
struct Body<'a> {
    keyword: &'static str,
    common: &'a BlockCommon,
    children: Option<Children<'a>>,
    source: Option<(&'a SourceRef, Option<&'a Comment>)>,
    deadline: Option<&'a Deadline>,
    promote: Option<(&'static str, Option<&'a Comment>)>,
}

/// A `children:` property: its entries and its own comment.
struct Children<'a> {
    entries: &'a [ChildEntry],
    comment: Option<&'a Comment>,
}

impl<'a> Body<'a> {
    fn of(block: &'a Block) -> Self {
        let common = block.common();
        let deadline = block.deadline();
        match block {
            Block::Objective(b) => Self {
                keyword: "objective",
                common,
                children: Some(Children {
                    entries: &b.children,
                    comment: b.children_comment.as_ref(),
                }),
                source: None,
                deadline,
                promote: b
                    .deadline
                    .as_ref()
                    .and_then(|d| d.promote.as_ref())
                    .map(|p| {
                        let ObjectivePromoteTarget::Structure = p.target.value;
                        ("structure", p.comment.as_ref())
                    }),
            },
            Block::Structure(b) => Self {
                keyword: "structure",
                common,
                children: Some(Children {
                    entries: &b.children,
                    comment: b.children_comment.as_ref(),
                }),
                source: None,
                deadline,
                promote: None,
            },
            Block::Bind(b) => Self {
                keyword: "bind",
                common,
                children: None,
                source: Some((&b.source.value, b.source_comment.as_ref())),
                deadline,
                promote: None,
            },
            Block::Fiat(b) => Self {
                keyword: "fiat",
                common,
                children: None,
                source: None,
                deadline,
                promote: b
                    .deadline
                    .as_ref()
                    .and_then(|d| d.promote.as_ref())
                    .map(|p| {
                        let target = match p.target.value {
                            FiatPromoteTarget::Structure => "structure",
                            FiatPromoteTarget::Bind => "bind",
                        };
                        (target, p.comment.as_ref())
                    }),
            },
        }
    }
}

/// Accumulates the formatted text line by line.
#[derive(Default)]
struct Printer {
    text: String,
}

impl Printer {
    fn line(&mut self, level: usize, content: &str) {
        for _ in 0..level {
            self.text.push_str(INDENT);
        }
        self.text.push_str(content);
        self.text.push('\n');
    }

    fn blank_line(&mut self) {
        self.text.push('\n');
    }

    /// Writes a comment on its own lines at `level`, one `//` line per text
    /// line; nothing when there is no comment.
    fn comment(&mut self, level: usize, comment: Option<&Comment>) {
        for line in comment.iter().flat_map(|comment| comment.lines()) {
            if line.value.is_empty() {
                self.line(level, "//");
            } else {
                self.line(level, &format!("// {}", line.value));
            }
        }
    }

    fn block<S: BuildHasher>(&mut self, body: &Body<'_>, values: &HashMap<String, String, S>) {
        let common = body.common;
        self.comment(0, common.comment.as_ref());
        let mut header = format!("{} {}", body.keyword, common.id.value);
        for tag in &common.tags {
            header.push_str(" #");
            header.push_str(&tag.value);
        }
        header.push_str(" {");
        self.line(0, &header);

        self.description(common, values);
        if let Some(children) = &body.children {
            self.children(children);
        }
        if let Some((source, comment)) = body.source {
            self.comment(1, comment);
            let (key, reference) = match source {
                SourceRef::Test(reference) => ("test", reference),
                SourceRef::Calc(reference) => ("calc", reference),
            };
            self.line(1, &format!("{key}: \"{reference}\""));
        }
        if let Some(deadline) = body.deadline {
            self.comment(1, deadline.comment.as_ref());
            self.line(1, &format!("until: {}", deadline.until.value));
        }
        if let Some((target, comment)) = body.promote {
            self.comment(1, comment);
            self.line(1, &format!("promote: {target}"));
        }
        self.line(0, "}");
    }

    fn description<S: BuildHasher>(
        &mut self,
        common: &BlockCommon,
        values: &HashMap<String, String, S>,
    ) {
        let description = &common.description;
        let fence = "`".repeat(description.fence_length);
        // The info string follows the fence directly, except that one which
        // itself starts with a backtick would otherwise read as part of a
        // longer fence.
        let info = match description.info.as_deref() {
            Some(info) if info.starts_with('`') => format!(" {info}"),
            Some(info) => info.to_owned(),
            None => String::new(),
        };
        self.line(1, &format!("{fence}{info}"));
        self.text
            .push_str(&fence::content(description, INDENT, values));
        self.line(1, &fence);
    }

    fn children(&mut self, children: &Children<'_>) {
        self.comment(1, children.comment);
        if let [only] = children.entries
            && only.comment.is_none()
        {
            self.line(1, &format!("children: {}", only.id.value));
            return;
        }
        self.line(1, "children:");
        let last = children.entries.len().saturating_sub(1);
        for (index, entry) in children.entries.iter().enumerate() {
            self.comment(2, entry.comment.as_ref());
            let separator = if index == last { "" } else { "," };
            self.line(2, &format!("{}{separator}", entry.id.value));
        }
    }
}

#[cfg(test)]
mod tests;
