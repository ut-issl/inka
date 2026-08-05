//! Abstract syntax tree for the Inka DSL.
//!
//! Each discharge mode has its own block type, and [`Block`] provides the
//! common enum over them. This makes invalid combinations, such as a fiat with
//! children or a structure with a source, unrepresentable in the AST.
//!
//! The AST preserves the parsed source structure only. Operations such as
//! interpolation, hashing, and evidence matching are handled by later passes.
//! Source locations used for diagnostics are represented with [`Spanned`].

use crate::source::{Span, Spanned};

/// A block in one of the four discharge modes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Objective(ObjectiveBlock),
    Structure(StructureBlock),
    Bind(BindBlock),
    Fiat(FiatBlock),
}

impl Block {
    /// Returns the fields shared by all block types.
    #[must_use]
    pub fn common(&self) -> &BlockCommon {
        match self {
            Block::Objective(b) => &b.common,
            Block::Structure(b) => &b.common,
            Block::Bind(b) => &b.common,
            Block::Fiat(b) => &b.common,
        }
    }
}

/// Fields shared by every block, including its header, description, and span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockCommon {
    pub id: Spanned<String>,
    /// Tags without the leading `#`.
    ///
    /// Tags use a free-form vocabulary and are excluded from hashing.
    pub tags: Vec<Spanned<String>>,
    pub description: Description,
    /// Span covering the entire block, from the mode keyword through the
    /// closing brace.
    pub span: Span,
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
    /// Span of the content inside the fence.
    ///
    /// Interpolation errors such as invalid `{$var}` references are located
    /// relative to the start of this span.
    pub span: Span,
}

/// A deadline annotation introduced by `until:`.
///
/// `reason:` and, where supported, `promote:` are only valid when `until:` is
/// present. The AST types encode that constraint directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deadline {
    /// Phase after which approvals for this node no longer count.
    pub until: Spanned<String>,
    /// Optional rationale, stored without its surrounding quotes.
    ///
    /// The rationale is included in the node's contract hash.
    pub reason: Option<Spanned<String>>,
}

/// Deadline annotation for a fiat block, optionally including a target mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FiatDeadline {
    pub deadline: Deadline,
    pub promote: Option<Spanned<FiatPromoteTarget>>,
}

/// Deadline annotation for the objective block.
///
/// An objective may only announce promotion to a structure block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectiveDeadline {
    pub deadline: Deadline,
    pub promote: Option<Spanned<ObjectivePromoteTarget>>,
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
    /// A `veriq:<module>.<name>` reference.
    ///
    /// The dotted Python name is preserved verbatim.
    Veriq(String),
    /// A `ci:<test-target>` reference, such as `ci:tests/hash_props.rs`.
    Ci(String),
}

/// The root block of an argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObjectiveBlock {
    pub common: BlockCommon,
    pub deadline: Option<ObjectiveDeadline>,
    pub children: Vec<Spanned<String>>,
}

/// A proposition discharged by decomposition into child blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructureBlock {
    pub common: BlockCommon,
    pub deadline: Option<Deadline>,
    pub children: Vec<Spanned<String>>,
}

/// A proposition discharged by reference to machine-verifiable evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindBlock {
    pub common: BlockCommon,
    pub deadline: Option<Deadline>,
    pub source: Spanned<SourceRef>,
}

/// A proposition accepted without further argument.
///
/// Fiat blocks are always leaves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FiatBlock {
    pub common: BlockCommon,
    pub deadline: Option<FiatDeadline>,
}
