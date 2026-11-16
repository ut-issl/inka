//! Abstract syntax tree for the Inka DSL.
//!
//! Each discharge mode has its own block type, and [`Block`] provides the
//! common enum over them. This makes invalid combinations, such as a fiat with
//! children or a structure with a source, unrepresentable in the AST.
//!
//! The AST preserves the parsed source structure only. Operations such as
//! interpolation, hashing, and evidence matching are handled by later passes.
//! Source locations used for diagnostics are represented with [`Spanned`].
//!
//! Comments are part of the tree: each one sits on the element it is attached
//! to — a block, one of its properties, or one child entry — as a [`Comment`].
//!
//! Block types are generic over their description type `D`, which encodes the
//! processing stage: [`Description`] (the default) holds the raw fence text
//! straight from the parser, and interpolation rebuilds the block around a
//! rendered description type that no longer carries the raw text. A pass that
//! must only see interpolated text takes the rendered form, so handing it
//! pre-interpolation text is a type error rather than a convention.

use crate::source::{Span, Spanned};

/// A block in one of the four discharge modes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block<D = Description> {
    Objective(ObjectiveBlock<D>),
    Structure(StructureBlock<D>),
    Bind(BindBlock<D>),
    Fiat(FiatBlock<D>),
}

impl<D> Block<D> {
    /// Returns the fields shared by all block types.
    #[must_use]
    pub fn common(&self) -> &BlockCommon<D> {
        match self {
            Block::Objective(b) => &b.common,
            Block::Structure(b) => &b.common,
            Block::Bind(b) => &b.common,
            Block::Fiat(b) => &b.common,
        }
    }

    /// Returns the child entries of the block.
    ///
    /// Bind and fiat blocks are leaves by construction, so they yield an
    /// empty slice rather than an `Option`.
    #[must_use]
    pub fn children(&self) -> &[ChildEntry] {
        match self {
            Block::Objective(b) => &b.children,
            Block::Structure(b) => &b.children,
            Block::Bind(_) | Block::Fiat(_) => &[],
        }
    }

    /// Returns the block's `until:` annotation, whichever mode carries it.
    ///
    /// Promotion targets differ by mode, so they stay on the mode-specific
    /// types; this is the part every mode shares.
    #[must_use]
    pub fn deadline(&self) -> Option<&Deadline> {
        match self {
            Block::Objective(b) => b.deadline.as_ref().map(|d| &d.deadline),
            Block::Structure(b) => b.deadline.as_ref(),
            Block::Bind(b) => b.deadline.as_ref(),
            Block::Fiat(b) => b.deadline.as_ref().map(|d| &d.deadline),
        }
    }

    /// Rebuilds the block around a new description, moving every other field.
    ///
    /// This is the only way a block changes description stage, so a stage
    /// transition always replaces the previous description instead of
    /// carrying both.
    #[must_use]
    pub fn with_description<D2>(self, description: D2) -> Block<D2> {
        match self {
            Block::Objective(b) => Block::Objective(ObjectiveBlock {
                common: b.common.with_description(description),
                deadline: b.deadline,
                children: b.children,
                children_comment: b.children_comment,
            }),
            Block::Structure(b) => Block::Structure(StructureBlock {
                common: b.common.with_description(description),
                deadline: b.deadline,
                children: b.children,
                children_comment: b.children_comment,
            }),
            Block::Bind(b) => Block::Bind(BindBlock {
                common: b.common.with_description(description),
                deadline: b.deadline,
                source: b.source,
                source_comment: b.source_comment,
            }),
            Block::Fiat(b) => Block::Fiat(FiatBlock {
                common: b.common.with_description(description),
                deadline: b.deadline,
            }),
        }
    }
}

/// Fields shared by every block, including its header, description, and span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockCommon<D = Description> {
    pub id: Spanned<String>,
    /// Tags without the leading `#`.
    ///
    /// Tags use a free-form vocabulary and are excluded from hashing.
    pub tags: Vec<Spanned<String>>,
    pub description: D,
    /// The comment attached to the block itself.
    pub comment: Option<Comment>,
    /// Span covering the entire block, from the mode keyword through the
    /// closing brace.
    pub span: Span,
}

impl<D> BlockCommon<D> {
    /// Moves every field except the description, which is replaced.
    #[must_use]
    pub fn with_description<D2>(self, description: D2) -> BlockCommon<D2> {
        BlockCommon {
            id: self.id,
            tags: self.tags,
            description,
            comment: self.comment,
            span: self.span,
        }
    }
}

/// A fenced description containing the node's contract text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Description {
    /// Fence contents before interpolation or normalization.
    pub raw: String,
    /// Optional fence info string, such as `md` in a Markdown code fence.
    ///
    /// The value is preserved for future use but currently has no semantics.
    pub info: Option<String>,
    /// Number of backticks in the opening fence, which the formatter keeps.
    pub fence_length: usize,
    /// Span of the content inside the fence.
    ///
    /// Interpolation errors such as invalid `{$var}` references are located
    /// relative to the start of this span.
    pub span: Span,
}

/// A comment attached to one element of the argument.
///
/// It holds the comment's source lines in order, each with the `//`, one
/// following space, and trailing whitespace removed, and NFC-normalized.
/// Several comments on one element merge into one, line by line. Leading and
/// trailing empty lines are dropped, so a comment always has text: an element
/// whose comment lines are all empty carries no comment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Comment {
    lines: Vec<Spanned<String>>,
}

impl Comment {
    /// Builds a comment from its source lines, or `None` when every line is
    /// empty.
    #[must_use]
    pub fn from_lines(mut lines: Vec<Spanned<String>>) -> Option<Self> {
        let first = lines.iter().position(|line| !line.value.is_empty())?;
        let last = lines.iter().rposition(|line| !line.value.is_empty())?;
        lines.truncate(last + 1);
        lines.drain(..first);
        Some(Self { lines })
    }

    /// The comment's lines, in source order.
    #[must_use]
    pub fn lines(&self) -> &[Spanned<String>] {
        &self.lines
    }

    /// The comment's text: its lines joined with `\n`.
    #[must_use]
    pub fn text(&self) -> String {
        self.lines
            .iter()
            .map(|line| line.value.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// One entry of a `children:` list: the referenced block and the comment
/// attached to the entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildEntry {
    /// The child reference, namespace-qualified exactly as written.
    pub id: Spanned<String>,
    pub comment: Option<Comment>,
}

/// A deadline annotation introduced by `until:`.
///
/// Where supported, `promote:` is only valid when `until:` is present. The AST
/// types encode that constraint directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deadline {
    /// Phase after which approvals for this node no longer count.
    pub until: Spanned<String>,
    /// The comment attached to `until:`, which is where a deadline's
    /// rationale is written.
    pub comment: Option<Comment>,
}

/// A `promote:` property: the announced target mode and its comment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Promotion<T> {
    pub target: Spanned<T>,
    pub comment: Option<Comment>,
}

/// Deadline annotation for a fiat block, optionally including a target mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FiatDeadline {
    pub deadline: Deadline,
    pub promote: Option<Promotion<FiatPromoteTarget>>,
}

/// Deadline annotation for the objective block.
///
/// An objective may only announce promotion to a structure block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectiveDeadline {
    pub deadline: Deadline,
    pub promote: Option<Promotion<ObjectivePromoteTarget>>,
}

/// Modes to which a fiat block may be promoted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FiatPromoteTarget {
    Structure,
    Bind,
}

/// Modes to which the objective block may be promoted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectivePromoteTarget {
    Structure,
}

/// A reference to machine-verifiable evidence used by a bind block.
///
/// The supported schemes are part of the language syntax, so they are modeled
/// as a closed enum. Provider-specific behavior belongs to evaluation rather
/// than to the AST.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceRef {
    /// A `calc: "<path>"` reference: the target's path in the producer's
    /// calculation graph (for veriq, `Scope::?name`), preserved verbatim.
    Calc(String),
    /// A `test: "<test-id>"` reference, such as `test: "tests/hash_props.rs::roundtrip"`.
    Test(String),
}

/// The root block of an argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectiveBlock<D = Description> {
    pub common: BlockCommon<D>,
    pub deadline: Option<ObjectiveDeadline>,
    pub children: Vec<ChildEntry>,
    /// The comment attached to the `children:` property.
    pub children_comment: Option<Comment>,
}

/// A proposition discharged by decomposition into child blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructureBlock<D = Description> {
    pub common: BlockCommon<D>,
    pub deadline: Option<Deadline>,
    pub children: Vec<ChildEntry>,
    /// The comment attached to the `children:` property.
    pub children_comment: Option<Comment>,
}

/// A proposition discharged by reference to machine-verifiable evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindBlock<D = Description> {
    pub common: BlockCommon<D>,
    pub deadline: Option<Deadline>,
    pub source: Spanned<SourceRef>,
    /// The comment attached to the `test:` / `calc:` property.
    pub source_comment: Option<Comment>,
}

/// A proposition accepted without further argument.
///
/// Fiat blocks are always leaves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FiatBlock<D = Description> {
    pub common: BlockCommon<D>,
    pub deadline: Option<FiatDeadline>,
}
