"""Emit inka calc evidence from a veriq project.

Runnable as ``python -m inka_evidence.veriq`` or through the
``inka-evidence-veriq`` script. For every verification of the project (or
the ``--target`` selection) it emits one calc evidence target: the
reviewable source of the verification's definition-level upstream subgraph,
the inka nodes that subgraph assumes, and the verification's leaf results.
The wire format and its validation live in the shared Rust crate; veriq is
only imported here, as an optional dependency (``inka-evidence[veriq]``).

Assumptions are declared in veriq node metadata under the ``inka`` key::

    @power.calculation(metadata={"inka": {"assumes": ["power::eclipse_model_valid"]}})
    def budget(...): ...

and unioned over the upstream subgraph. Whether the IDs name real nodes is
inka's business; this adapter only checks that they are strings.
"""

from __future__ import annotations

import argparse
import enum
import inspect
import sys
import typing
from collections.abc import Mapping
from pathlib import Path
from typing import TYPE_CHECKING

from inka_evidence import CalcEvidenceBuilder, Definition, InvalidEvidenceError, Leaf

if TYPE_CHECKING:
    from collections.abc import Iterable

    import veriq as vq
    from pydantic import BaseModel

#: The metadata key under which veriq nodes carry inka-facing declarations.
METADATA_KEY = "inka"

#: The entry of :data:`METADATA_KEY` listing assumed inka node IDs.
ASSUMES_KEY = "assumes"


class AdapterError(Exception):
    """The veriq project cannot be turned into calc evidence."""


# ---------------------------------------------------------------------------
# Sources
# ---------------------------------------------------------------------------


def _source(obj: object, what: str) -> str:
    try:
        return inspect.getsource(obj)  # type: ignore[arg-type]
    except (OSError, TypeError) as error:
        msg = f"cannot read the source of {what}: {error}"
        raise AdapterError(msg) from error


def _is_user_class(candidate: object) -> bool:
    """A class the project defines, as opposed to a library or builtin one."""
    from pydantic import BaseModel  # noqa: PLC0415

    if not isinstance(candidate, type):
        return False
    if issubclass(candidate, BaseModel):
        return candidate is not BaseModel
    if issubclass(candidate, enum.Enum):
        return candidate.__module__ not in ("enum", "builtins")
    return False


def _referenced_classes(annotation: object, found: dict[str, type]) -> None:
    """Collects the user model and enum classes an annotation reaches."""
    from pydantic import BaseModel  # noqa: PLC0415

    if _is_user_class(annotation):
        assert isinstance(annotation, type)
        key = f"{annotation.__module__}.{annotation.__qualname__}"
        if key in found:
            return
        found[key] = annotation
        if issubclass(annotation, BaseModel):
            for field in annotation.model_fields.values():
                _referenced_classes(field.annotation, found)
        return
    for argument in typing.get_args(annotation):
        _referenced_classes(argument, found)


def model_source(root_model: type[BaseModel]) -> str:
    """The reviewable source of a root model: its class, then every user
    model and enum class its fields reach, each once, in name order."""
    found: dict[str, type] = {}
    _referenced_classes(root_model, found)
    root_key = f"{root_model.__module__}.{root_model.__qualname__}"
    nested = sorted(key for key in found if key != root_key)
    sources = [_source(root_model, f"root model {root_model.__qualname__}")]
    sources.extend(_source(found[key], f"model class {key}") for key in nested)
    return "\n".join(sources)


def function_source(func: object, what: str) -> str:
    """The reviewable source of a calculation or verification: the function,
    then every user model and enum class its return annotation reaches."""
    sources = [_source(func, what)]
    try:
        returned = typing.get_type_hints(func).get("return")  # type: ignore[arg-type]
    except (NameError, TypeError):
        returned = inspect.signature(func).return_annotation  # type: ignore[arg-type]
    found: dict[str, type] = {}
    _referenced_classes(returned, found)
    sources.extend(_source(found[key], f"class {key}") for key in sorted(found))
    return "\n".join(sources)


def definition_of(project: vq.Project, node: vq.DefinitionNode) -> Definition:
    """The calc evidence definition for one node of an upstream subgraph."""
    from veriq import NodeKind  # noqa: PLC0415

    scope = project.scopes[node.id.scope]
    root = node.id.root
    path = str(node.id)
    if node.kind == NodeKind.MODEL:
        return Definition(path, "model", model_source(scope.get_root_model()))
    if node.kind == NodeKind.CALCULATION:
        calculation = scope.calculations[root.removeprefix("@")]
        return Definition(path, "calculation", function_source(calculation.func, f"calculation {path}"))
    verification = scope.verifications[root.removeprefix("?")]
    return Definition(path, "verification", function_source(verification.func, f"verification {path}"))


# ---------------------------------------------------------------------------
# Assumptions
# ---------------------------------------------------------------------------


def assumptions_of(subgraph: vq.UpstreamSubgraph) -> list[str]:
    """The inka node IDs the subgraph's definitions declare, each once, sorted."""
    assumed: set[str] = set()
    for node in subgraph.nodes.values():
        declared = node.user_metadata.get(METADATA_KEY)
        if declared is None:
            continue
        if not isinstance(declared, Mapping):
            msg = f"metadata {METADATA_KEY!r} of {node.id} must be a mapping, got {type(declared).__name__}"
            raise AdapterError(msg)
        ids = declared.get(ASSUMES_KEY, ())
        if isinstance(ids, str) or not all(isinstance(entry, str) for entry in ids):
            msg = f"metadata {METADATA_KEY!r}.{ASSUMES_KEY!r} of {node.id} must be a list of inka node ids"
            raise AdapterError(msg)
        assumed.update(ids)
    return sorted(assumed)


# ---------------------------------------------------------------------------
# Collection
# ---------------------------------------------------------------------------


def _leaves(outputs: Iterable[vq.ProjectPath]) -> list[vq.ProjectPath]:
    """The leaf outputs of a definition: veriq's graph also carries a node for
    a table-valued output as a whole, which has no result of its own."""
    outputs = list(outputs)

    def is_prefix_of_another(candidate: vq.ProjectPath) -> bool:
        parts = candidate.path.parts
        return any(
            other is not candidate
            and other.path.root == candidate.path.root
            and len(other.path.parts) > len(parts)
            and other.path.parts[: len(parts)] == parts
            for other in outputs
        )

    return [output for output in outputs if not is_prefix_of_another(output)]


def _leaf(result: vq.EvaluationResult, path: vq.ProjectPath) -> Leaf:
    try:
        value = result.get_value(path)
    except KeyError:
        return Leaf(str(path))
    if value is None:
        return Leaf(str(path))
    return Leaf(str(path), bool(value))


def collect(
    project: vq.Project,
    model_data: Mapping[str, BaseModel],
    *,
    targets: Iterable[str] | None = None,
    builder: CalcEvidenceBuilder | None = None,
) -> CalcEvidenceBuilder:
    """Evaluates the project and records one target per verification.

    ``targets`` restricts the emitted verifications to the given
    ``Scope::?name`` paths; by default every verification is emitted.
    Returns the builder the targets were added to.
    """
    import veriq as vq  # noqa: PLC0415

    builder = CalcEvidenceBuilder() if builder is None else builder
    result = vq.evaluate_project(project, model_data)
    spec = vq.build_graph_spec(project)

    paths = (
        sorted(
            f"{scope_name}::?{name}"
            for scope_name, scope in project.scopes.items()
            for name in scope.verifications
        )
        if targets is None
        else list(targets)
    )
    for path in paths:
        try:
            subgraph = vq.upstream_subgraph(spec, path)
        except KeyError as error:
            msg = f"target {path!r} is not a verification of the project"
            raise AdapterError(msg) from error
        if subgraph.nodes[subgraph.target].kind != vq.NodeKind.VERIFICATION:
            msg = f"target {path!r} is not a verification"
            raise AdapterError(msg)
        scope = project.scopes[subgraph.target.scope]
        verification = scope.verifications[subgraph.target.root.removeprefix("?")]
        outputs = _leaves(subgraph.nodes[subgraph.target].outputs)
        try:
            builder.add_target(
                str(subgraph.target),
                [definition_of(project, node) for node in subgraph.nodes.values()],
                assumes=assumptions_of(subgraph),
                leaves=[_leaf(result, output) for output in outputs],
                valid=all(result.validity.get(output, True) for output in outputs),
                xfail=verification.xfail,
            )
        except InvalidEvidenceError as error:
            msg = f"target {path!r}: {error}"
            raise AdapterError(msg) from error
    return builder


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------


def _split_provenance(raw: str) -> tuple[str, str]:
    key, separator, value = raw.partition("=")
    if not separator or not key:
        raise SystemExit(f"error: provenance entry {raw!r} is not key=value")
    return key, value


def _load_project(path: str, project_var: str | None) -> vq.Project:
    from veriq._cli.discover import (  # noqa: PLC0415
        load_project_from_module_path,
        load_project_from_script,
    )

    if ":" in path and not Path(path).exists():
        return load_project_from_module_path(path)
    return load_project_from_script(Path(path), project_var)


def _configured_input() -> Path | None:
    from veriq._cli.config import ConfigError, get_config  # noqa: PLC0415

    try:
        return get_config().input
    except ConfigError as error:
        raise SystemExit(f"error: {error}") from error


def main(argv: list[str] | None = None) -> None:
    parser = argparse.ArgumentParser(
        prog="inka-evidence-veriq",
        description="Emit inka calc evidence from a veriq project.",
    )
    parser.add_argument(
        "project",
        metavar="SCRIPT|MODULE:VAR",
        help="the Python file defining the veriq project, or `module.path:variable`",
    )
    parser.add_argument(
        "--project-var",
        metavar="NAME",
        help="the Project variable in SCRIPT; inferred when there is exactly one",
    )
    parser.add_argument(
        "--input",
        metavar="PATH",
        help="the input TOML; defaults to [tool.veriq].input of the nearest pyproject.toml",
    )
    parser.add_argument(
        "--output",
        default="-",
        metavar="PATH",
        help="where to write the evidence JSON; `-` means stdout",
    )
    parser.add_argument(
        "--target",
        action="append",
        default=[],
        metavar="SCOPE::?NAME",
        help="emit only this verification (repeatable); default: every verification",
    )
    parser.add_argument(
        "--provenance",
        action="append",
        default=[],
        metavar="KEY=VALUE",
        help="provenance entry for the file (repeatable)",
    )
    arguments = parser.parse_args(argv)

    try:
        import veriq as vq  # noqa: PLC0415
    except ImportError as error:
        raise SystemExit(
            "error: veriq is not installed; install `inka-evidence[veriq]`"
        ) from error

    try:
        project = _load_project(arguments.project, arguments.project_var)
    except (ImportError, ValueError, TypeError) as error:
        raise SystemExit(f"error: cannot load the project: {error}") from error

    input_path = Path(arguments.input) if arguments.input else _configured_input()
    if input_path is None:
        raise SystemExit("error: an input TOML is required; pass --input or configure [tool.veriq].input")

    builder = CalcEvidenceBuilder()
    try:
        for key, value in (_split_provenance(raw) for raw in arguments.provenance):
            builder.provenance(key, value)
    except InvalidEvidenceError as error:
        raise SystemExit(f"error: {error}") from error
    builder.capsula_env()

    try:
        model_data = vq.load_model_data_from_toml(project, input_path)
        collect(project, model_data, targets=arguments.target or None, builder=builder)
    except (AdapterError, RuntimeError, OSError, ValueError) as error:
        raise SystemExit(f"error: {error}") from error

    text = builder.finish()
    if arguments.output == "-":
        sys.stdout.write(text)
    else:
        try:
            Path(arguments.output).write_text(text, encoding="utf-8")
        except OSError as error:
            raise SystemExit(f"error: cannot write {arguments.output}: {error}") from error


if __name__ == "__main__":
    main()
