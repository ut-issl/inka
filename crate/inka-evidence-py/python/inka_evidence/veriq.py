"""Emit inka calc evidence from a veriq project.

Runnable as ``python -m inka_evidence.veriq`` or through the
``inka-evidence-veriq`` script. For every verification of the project (or
the ``--target`` selection) it emits one calc evidence target: the
reviewable source of the verification's definition-level upstream subgraph,
the inka nodes that subgraph assumes, and the verification's leaf results (as returned by the verification; a
failed assumption is reported through ``valid``).
The wire format and its validation live in the shared Rust crate; veriq is
only imported here, as an optional dependency (``inka-evidence[veriq]``).

The source of a definition is its own text followed by that of every
project class it reaches: through field and return annotations, type
arguments, PEP 695 aliases and base classes. Classes of veriq, pydantic and
the standard library are left out, so upgrading those does not disturb an
approval; a class of any other package counts as project code, and so does
one of a project module that merely shares a standard library module's name.
An enum that ``@vq.with_range`` generated has no text of its own and is
emitted as the list of its members.

What a function merely calls or reads — helper functions, module constants —
is not followed: only veriq definitions and the classes they name are hashed.

Assumptions are declared in veriq node metadata under the ``inka`` key::

    @power.calculation(metadata={"inka": {"assumes": ["power::eclipse_model_valid"]}})
    def budget(...): ...

and unioned over the upstream subgraph. Whether the IDs name real nodes is
inka's business; this adapter only checks that ``assumes`` is a list of
strings and that ``inka`` holds nothing else.

Rather than emit evidence that says less than it seems to, the adapter
refuses a leaf result that is neither a ``bool`` nor ``None``, malformed
``inka`` metadata, a project without any verification, an annotation that
does not resolve, and two different classes that go by one name.
"""

from __future__ import annotations

import argparse
import enum
import functools
import inspect
import reprlib
import sys
import sysconfig
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

#: Top-level packages whose classes are library code. Their source is not
#: what a project's reviewers approve, and hashing it would tie every
#: approval to the installed version of the library. The standard library is
#: library code as well; see :func:`_is_library_module`.
LIBRARY_PACKAGES = frozenset({"pydantic", "pydantic_core", "veriq"})

#: Directory names that hold installed packages, also when they sit inside
#: the standard library's own directory.
SITE_DIRECTORIES = frozenset({"site-packages", "dist-packages"})

#: The module veriq files the enums of ``@vq.with_range`` under. The decorator
#: rebuilds the class through the functional ``IntEnum`` API, so it carries
#: veriq's module and has no source text — yet which members it has is the
#: project's decision, and part of what a reviewer approves.
RANGE_ENUM_MODULE = "veriq._range_enum"


class AdapterError(Exception):
    """The veriq project cannot be turned into calc evidence."""


def _type_name(value: object) -> str:
    cls = type(value)
    return cls.__qualname__ if cls.__module__ == "builtins" else f"{cls.__module__}.{cls.__qualname__}"


# ---------------------------------------------------------------------------
# Sources
# ---------------------------------------------------------------------------


def _source(obj: object, what: str) -> str:
    try:
        return inspect.getsource(obj)  # type: ignore[arg-type]
    except (OSError, TypeError) as error:
        msg = f"cannot read the source of {what}: {error}"
        raise AdapterError(msg) from error


@functools.cache
def _stdlib_directories() -> tuple[Path, ...]:
    names = ("stdlib", "platstdlib")
    return tuple({Path(path).resolve() for path in map(sysconfig.get_path, names) if path})


def _is_stdlib_module(name: str) -> bool:
    """Whether the loaded module of this name is the standard library's.

    The name alone does not say: veriq imports a project script under its
    file stem, and ``profile.py`` or a ``test`` package is the project's own
    code whatever the standard library calls its modules. What settles it is
    where the module was loaded from. A module that cannot be placed counts
    as the project's, so it is hashed or refused, never silently left out.
    """
    module = sys.modules.get(name)
    if module is None:
        return False
    if getattr(getattr(module, "__spec__", None), "origin", None) in {"built-in", "frozen"}:
        return True
    file = getattr(module, "__file__", None)
    if file is None:
        return False
    location = Path(file).resolve()
    return any(
        location.is_relative_to(directory) and SITE_DIRECTORIES.isdisjoint(location.relative_to(directory).parts)
        for directory in _stdlib_directories()
    )


def _is_library_module(name: str) -> bool:
    top_level = name.partition(".")[0]
    if top_level in LIBRARY_PACKAGES:
        return True
    return top_level in sys.stdlib_module_names and _is_stdlib_module(name)


def _is_user_class(candidate: object) -> bool:
    """A class the project answers for, as opposed to a library or builtin one.

    Deliberately conservative: only :data:`LIBRARY_PACKAGES` and the standard
    library are excluded, so a class of any other third-party package stays
    in the hashed source.
    """
    if not isinstance(candidate, type):
        return False
    return _is_range_enum(candidate) or not _is_library_module(candidate.__module__)


def _is_range_enum(candidate: object) -> bool:
    """An enum ``@vq.with_range`` generated from a class of the project."""
    return (
        isinstance(candidate, type)
        and issubclass(candidate, enum.Enum)
        and candidate.__module__ == RANGE_ENUM_MODULE
    )


def _range_enum_members(cls: type[enum.Enum]) -> list[str]:
    return [f"{name} = {member.value!r}" for name, member in cls.__members__.items()]


def _class_key(cls: type) -> str:
    name = f"{cls.__module__}.{cls.__qualname__}"
    if not _is_range_enum(cls):
        return name
    # Every range enum carries veriq's module, so two of one name differ only
    # in their members; the key must keep them apart.
    return f"{name}({', '.join(_range_enum_members(cls))})"


def _class_source(cls: type, what: str) -> str:
    """The text of a class; for a range enum, which has none, its members."""
    if not _is_range_enum(cls):
        return _source(cls, what)
    body = "".join(f"    {member}\n" for member in _range_enum_members(cls)) or "    pass\n"
    return f"class {cls.__qualname__}(IntEnum):  # members generated by veriq.with_range\n{body}"


def _is_type_alias(annotation: object) -> bool:
    """A PEP 695 alias: a ``type X = ...`` statement or a ``TypeAliasType``
    call. Told by the class name, so that asking does not evaluate the alias."""
    return type(annotation).__name__ == "TypeAliasType"


def _parametrization(cls: type) -> tuple[object, ...] | None:
    """The origin and type arguments of a parametrized pydantic generic
    (``Report[Unit]``), which is a class of its own without source text."""
    metadata = getattr(cls, "__pydantic_generic_metadata__", None)
    if not isinstance(metadata, Mapping) or metadata.get("origin") is None:
        return None
    return (metadata["origin"], *metadata.get("args", ()))


def _successors(annotation: object) -> tuple[object, ...]:
    """What an annotation leads to: the places to look for further classes."""
    from pydantic import BaseModel  # noqa: PLC0415

    if _is_type_alias(annotation):
        return (annotation.__value__,)  # type: ignore[attr-defined]
    origin = typing.get_origin(annotation)
    if origin is not None:
        return (origin, *typing.get_args(annotation))
    if isinstance(annotation, (list, tuple)):  # the parameter list of a Callable
        return tuple(annotation)
    if not isinstance(annotation, type):
        return ()
    parametrization = _parametrization(annotation)
    if parametrization is not None:
        return parametrization
    if not _is_user_class(annotation):
        return ()
    fields = annotation.model_fields.values() if issubclass(annotation, BaseModel) else ()
    return (*annotation.__mro__[1:], *(field.annotation for field in fields))


def _collect(found: dict[str, type], cls: type, what: str) -> None:
    """Files a class under its name, refusing a second, different one.

    A source lists each name once, so the other class would be left out of
    what is hashed and shown. The same text met twice (a class a function
    builds on every call) is one class as far as a reviewer is concerned.
    """
    key = _class_key(cls)
    known = found.setdefault(key, cls)
    if known is cls or _class_source(known, f"class {key}") == _class_source(cls, f"class {key}"):
        return
    msg = f"{what} reaches two different classes named {key}; give them distinct names"
    raise AdapterError(msg)


def _referenced_classes(annotation: object, what: str) -> dict[str, type]:
    """The user classes an annotation reaches, by qualified name.

    Followed: type arguments, PEP 695 aliases, the bases of every user class
    and the fields of every user model. A library class is passed through —
    its type arguments are followed — without being collected.
    """
    try:
        return _walk_classes(annotation, what)
    except NameError as error:  # an alias is evaluated when it is followed
        msg = f"cannot resolve a type alias that {what} reaches: {error}"
        raise AdapterError(msg) from error


def _walk_classes(annotation: object, what: str) -> dict[str, type]:
    found: dict[str, type] = {}
    # Classes and aliases already expanded; a recursive alias or a model that
    # refers to itself would otherwise never end. Holding the objects keeps
    # their ids from being reused.
    expanded: dict[int, object] = {}
    pending: list[object] = [annotation]
    while pending:
        current = pending.pop()
        if isinstance(current, type) or _is_type_alias(current):
            if id(current) in expanded:
                continue
            expanded[id(current)] = current
        if _is_user_class(current) and _parametrization(current) is None:
            _collect(found, current, what)  # type: ignore[arg-type]
        pending.extend(_successors(current))
    return found


def _class_sources(classes: Mapping[str, type], what: str) -> list[str]:
    return [_class_source(classes[key], f"{what} {key}") for key in sorted(classes)]


def model_source(root_model: type[BaseModel]) -> str:
    """The reviewable source of a root model: its class, then every user
    class its bases and fields reach, each once, in name order."""
    what = f"root model {root_model.__qualname__}"
    root_key = _class_key(root_model)
    nested = {key: cls for key, cls in _referenced_classes(root_model, what).items() if key != root_key}
    return "\n".join([_source(root_model, what), *_class_sources(nested, "model class")])


def function_source(func: object, what: str) -> str:
    """The reviewable source of a calculation or verification: the function,
    then every user class its return annotation reaches, in name order."""
    returned = _return_annotation(func, what)
    return "\n".join([_source(func, what), *_class_sources(_referenced_classes(returned, what), "class")])


def _return_annotation(func: object, what: str) -> object:
    try:
        return typing.get_type_hints(func).get("return")  # type: ignore[arg-type]
    except TypeError:
        # Not a plain function (a callable object): there are no names to
        # resolve, and the signature holds the annotation as written.
        return inspect.signature(func).return_annotation  # type: ignore[arg-type]
    except (NameError, AttributeError, SyntaxError) as error:
        # A name inside the annotation that does not resolve. Carrying on
        # would leave the classes behind it out of the source.
        msg = f"cannot resolve the return annotation of {what}: {error}"
        raise AdapterError(msg) from error


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


def _declared_assumptions(node: vq.DefinitionNode) -> tuple[str, ...]:
    """The inka node IDs one definition declares.

    Anything but a list of strings under the one known key is an error: a
    misspelt key or a bare string would silently drop a dependency edge.
    """
    declared = node.user_metadata.get(METADATA_KEY)
    if declared is None:
        return ()
    if not isinstance(declared, Mapping):
        msg = f"metadata {METADATA_KEY!r} of {node.id} must be a mapping, got {_type_name(declared)}"
        raise AdapterError(msg)
    unknown = sorted(repr(key) for key in declared if key != ASSUMES_KEY)
    if unknown:
        msg = (
            f"metadata {METADATA_KEY!r} of {node.id} has unknown key{'s' if len(unknown) > 1 else ''} "
            f"{', '.join(unknown)}; only {ASSUMES_KEY!r} is recognized"
        )
        raise AdapterError(msg)
    if ASSUMES_KEY not in declared:
        return ()
    ids = declared[ASSUMES_KEY]
    expected = f"metadata {METADATA_KEY!r}.{ASSUMES_KEY!r} of {node.id} must be a list of inka node ids"
    if not isinstance(ids, (list, tuple)):
        msg = f"{expected}, got {_type_name(ids)}"
        raise AdapterError(msg)
    strangers = sorted({_type_name(entry) for entry in ids if not isinstance(entry, str)})
    if strangers:
        msg = f"{expected}, got an entry of type {', '.join(strangers)}"
        raise AdapterError(msg)
    return tuple(ids)


def assumptions_of(subgraph: vq.UpstreamSubgraph) -> list[str]:
    """The inka node IDs the subgraph's definitions declare, each once, sorted."""
    return sorted({assumed for node in subgraph.nodes.values() for assumed in _declared_assumptions(node)})


# ---------------------------------------------------------------------------
# Collection
# ---------------------------------------------------------------------------


def _leaf(result: vq.EvaluationResult, path: vq.ProjectPath) -> Leaf:
    # The raw value: what the verification returned. veriq overrides the
    # value of a verification whose assumption failed to False; that fact is
    # reported through `valid`, not by rewriting the result.
    try:
        value = result.get_value(path, raw=True)
    except KeyError:
        return Leaf(str(path))
    if value is None:
        return Leaf(str(path))
    # veriq does not hold a verification to `-> bool` at run time and judges
    # with `is True`; coercing would report a returned 6.0 or 1 as a pass.
    if not isinstance(value, bool):
        msg = (
            f"leaf {path} is {reprlib.repr(value)} of type {_type_name(value)}; "
            "a verification result must be a bool"
        )
        raise AdapterError(msg)
    return Leaf(str(path), value)


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

    Raises :class:`AdapterError` when the project cannot be turned into
    evidence — among others when it defines no verification at all: an
    empty evidence file would pass for a run that verified something.
    """
    import veriq as vq  # noqa: PLC0415

    if targets is None:
        paths = sorted(
            f"{scope_name}::?{name}"
            for scope_name, scope in project.scopes.items()
            for name in scope.verifications
        )
        if not paths:
            msg = "the project defines no verification, so there is no calc evidence to emit"
            raise AdapterError(msg)
    else:
        paths = list(targets)

    builder = CalcEvidenceBuilder() if builder is None else builder
    result = vq.evaluate_project(project, model_data)
    spec = vq.build_graph_spec(project)

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
        outputs = subgraph.nodes[subgraph.target].outputs
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


def main(argv: list[str] | None = None) -> None:
    parser = argparse.ArgumentParser(
        prog="inka-evidence-veriq",
        description="Emit inka calc evidence from a veriq project.",
    )
    parser.add_argument(
        "project",
        nargs="?",
        metavar="SCRIPT|MODULE:VAR",
        help="the Python file defining the veriq project, or `module.path:variable`; "
        "defaults to [tool.veriq].project of the nearest pyproject.toml",
    )
    parser.add_argument(
        "--project-var",
        metavar="NAME",
        help="the Project variable in SCRIPT; inferred when there is exactly one",
    )
    parser.add_argument(
        "--input",
        metavar="PATH",
        help="the input TOML the project is evaluated with; "
        "defaults to [tool.veriq].input of the nearest pyproject.toml",
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
        config = vq.get_config()
        project = vq.load_project(arguments.project, project_var=arguments.project_var, config=config)
    except (vq.ConfigError, ImportError, ValueError, TypeError, FileNotFoundError) as error:
        raise SystemExit(f"error: cannot load the project: {error}") from error

    input_path = Path(arguments.input) if arguments.input else config.input
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
