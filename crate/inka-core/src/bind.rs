//! Resolving a bind's evidence reference against the imported evidence.
//!
//! Evaluation, the review queue and the recording command all ask the same
//! question of a bind: which imported evidence does its `test:` or `calc:`
//! reference name, and what does that evidence contribute to the bind's view
//! hash? Answering it in one place keeps the three from disagreeing about
//! which binds have a current hash at all.
//!
//! A test reference contributes its interface hash. A calc reference
//! contributes its interface hash and the nodes its target assumes: their
//! rendered descriptions fill the assumption slot of the bind hash, and their
//! establishment is a dependency of the bind's own. A reference no loaded
//! evidence answers resolves to `None` — the per-bind "missing evidence"
//! outcome, never an input error.

use crate::calc_evidence::{CalcEvidenceSet, CalcTarget};
use crate::eval::EvalError;
use crate::evidence::{EvidenceSet, TestEvidence};
use crate::hash::{EvidenceContract, LocalViewError};
use crate::view::{ArgumentView, EvidenceKind, ViewNode};

/// The imported evidence a bind's reference names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BindEvidence<'a> {
    /// A `test:` reference, answered by imported test evidence.
    Test(&'a TestEvidence),
    /// A `calc:` reference, answered by an imported calc target, with the
    /// argument nodes the target assumes in sorted block-ID order.
    Calc {
        target: &'a CalcTarget,
        assumed: Vec<&'a ViewNode>,
    },
}

impl<'a> BindEvidence<'a> {
    /// What the evidence contributes to the bind's view hash: its interface
    /// hash and, for a calc target, the assumed nodes' contract text.
    #[must_use]
    pub fn contract(&self) -> EvidenceContract<'a> {
        match self {
            BindEvidence::Test(test) => test.interface_hash.into(),
            BindEvidence::Calc { target, assumed } => EvidenceContract {
                interface: target.interface_hash,
                assumptions: assumed
                    .iter()
                    .map(|node| node.description.as_str())
                    .collect(),
            },
        }
    }

    /// The scheme of the reference this evidence answers.
    #[must_use]
    pub fn kind(&self) -> EvidenceKind {
        match self {
            BindEvidence::Test(_) => EvidenceKind::Test,
            BindEvidence::Calc { .. } => EvidenceKind::Calc,
        }
    }
}

/// Resolves a bind's reference against the imported evidence.
///
/// Returns `Ok(None)` when no loaded evidence provides the reference.
///
/// # Errors
///
/// Returns [`LocalViewError::MissingInterfaceHash`] (as an [`EvalError`])
/// for a bind without an evidence reference, which only a hand-built view
/// can contain, and [`EvalError::UnknownAssumption`] when a calc target
/// assumes a node the view does not define — a checked project cannot trip
/// either, since `inka check` resolves every assumption at load time.
pub fn resolve<'a>(
    view: &'a ArgumentView,
    node: &'a ViewNode,
    tests: &'a EvidenceSet,
    calcs: &'a CalcEvidenceSet,
) -> Result<Option<BindEvidence<'a>>, EvalError> {
    let Some(reference) = node.evidence.as_ref() else {
        return Err(LocalViewError::MissingInterfaceHash {
            id: node.id.clone(),
        }
        .into());
    };
    match reference.kind {
        EvidenceKind::Test => Ok(tests.get(&reference.target).map(BindEvidence::Test)),
        EvidenceKind::Calc => {
            let Some(target) = calcs.get(&reference.target) else {
                return Ok(None);
            };
            let mut assumed = Vec::with_capacity(target.assumes.len());
            for id in &target.assumes {
                let assumed_node = view.node(id).ok_or_else(|| EvalError::UnknownAssumption {
                    bind: node.id.clone(),
                    id: id.clone(),
                })?;
                assumed.push(assumed_node);
            }
            assumed.sort_by(|a, b| a.id.cmp(&b.id));
            Ok(Some(BindEvidence::Calc { target, assumed }))
        }
    }
}
