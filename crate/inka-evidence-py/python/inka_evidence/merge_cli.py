"""Merge per-configuration evidence files into one evidence file.

Runnable as ``python -m inka_evidence.merge_cli`` or through the
``inka-evidence-merge`` script. Inputs are ``[LABEL=]FILE`` pairs; a label
prefixes every case id of its file as ``label/case``. The merge semantics
live in the shared Rust crate — this module only parses arguments and does
the file I/O.
"""

from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path

from inka_evidence import InvalidEvidenceError
from inka_evidence._native import merge


def _split_input(raw: str) -> tuple[str | None, str]:
    """Splits one ``[LABEL=]FILE`` argument.

    The part before the first ``=`` is a label only when it contains no
    path separator, so a file name with an ``=`` still works when written
    with a leading ``./``.
    """
    label, separator, file = raw.partition("=")
    if separator and "/" not in label and "\\" not in label:
        return label, file
    return None, raw


def _split_provenance(raw: str) -> tuple[str, str]:
    key, separator, value = raw.partition("=")
    if not separator or not key:
        raise SystemExit(f"error: provenance entry {raw!r} is not key=value")
    return key, value


def main(argv: list[str] | None = None) -> None:
    parser = argparse.ArgumentParser(
        prog="inka-evidence-merge",
        description="Merge per-configuration evidence files into one.",
    )
    parser.add_argument(
        "--output",
        default="-",
        metavar="PATH",
        help="where to write the merged evidence JSON; `-` means stdout",
    )
    parser.add_argument(
        "--provenance",
        action="append",
        default=[],
        metavar="KEY=VALUE",
        help="extra provenance entry for the merged file (repeatable)",
    )
    parser.add_argument(
        "inputs",
        nargs="+",
        metavar="[LABEL=]FILE",
        help="evidence files to merge, each optionally labeled",
    )
    arguments = parser.parse_args(argv)

    pairs = []
    for raw in arguments.inputs:
        label, file = _split_input(raw)
        if not file:
            raise SystemExit(f"error: input {raw!r} has an empty file name")
        try:
            pairs.append((label, Path(file).read_text(encoding="utf-8")))
        except (OSError, UnicodeDecodeError) as error:
            raise SystemExit(f"error: cannot read {file}: {error}") from error
    provenance = [_split_provenance(raw) for raw in arguments.provenance]

    try:
        merged = merge(pairs, provenance, os.environ.get("CAPSULA_RUN_ID"))
    except InvalidEvidenceError as error:
        raise SystemExit(f"error: {error}") from error

    if arguments.output == "-":
        sys.stdout.write(merged)
    else:
        try:
            Path(arguments.output).write_text(merged, encoding="utf-8")
        except OSError as error:
            raise SystemExit(
                f"error: cannot write {arguments.output}: {error}"
            ) from error


if __name__ == "__main__":
    main()
