"""Emit inka test and calc evidence v1 JSON from Python adapters.

Validation and serialization live in the shared Rust crate; this package
re-exports its binding and ships the pytest plugin on top of it (see
``inka_evidence.pytest_plugin``).
"""

from inka_evidence._native import (
    VERSION as VERSION,
    CalcEvidenceBuilder as CalcEvidenceBuilder,
    Definition as Definition,
    EvidenceBuilder as EvidenceBuilder,
    Execution as Execution,
    InvalidEvidenceError as InvalidEvidenceError,
    Leaf as Leaf,
    merge as merge,
)

__all__ = [
    "VERSION",
    "CalcEvidenceBuilder",
    "Definition",
    "EvidenceBuilder",
    "Execution",
    "InvalidEvidenceError",
    "Leaf",
    "merge",
]
