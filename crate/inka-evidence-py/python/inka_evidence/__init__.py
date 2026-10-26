"""Emit inka test evidence v1 JSON from Python test adapters.

Validation and serialization live in the shared Rust crate; this package
re-exports its binding and ships the pytest plugin on top of it (see
``inka_evidence.pytest_plugin``).
"""

from inka_evidence._native import (
    VERSION as VERSION,
    EvidenceBuilder as EvidenceBuilder,
    Execution as Execution,
    InvalidEvidenceError as InvalidEvidenceError,
)

__all__ = ["VERSION", "EvidenceBuilder", "Execution", "InvalidEvidenceError"]
