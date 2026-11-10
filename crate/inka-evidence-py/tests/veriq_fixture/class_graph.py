"""A veriq project whose definitions reach their classes indirectly.

The root model and a calculation's output model inherit from user base
classes (a pydantic model, a parametrized generic one and a plain mixin),
one field goes through a PEP 695 alias, another through a recursive one, an
enum derives from veriq's ``StrEnumWithDoc`` and one field is a
``vq.FileRef`` — the adapter must emit every user class once and no library
one.
"""

from typing import Annotated

from pydantic import BaseModel

import veriq as vq

project = vq.Project("ClassGraphProject")
bus = vq.Scope("Bus")
project.add_scope(bus)


class Chemistry(vq.StrEnumWithDoc):
    LI_ION = "li_ion", "Lithium-ion"
    NI_H2 = "ni_h2", "Nickel-hydrogen"


class Cell(BaseModel):
    chemistry: Chemistry
    capacity_wh: float


type Cells = list[Cell]

type Harness = dict[str, Harness] | float


class Derating:
    """A plain mixin: not a pydantic model, still part of what Config means."""

    def derated(self, value: float) -> float:
        return value * 0.9


class Margins(BaseModel):
    gain: float = 2.0


@bus.root_model()
class Config(Margins, Derating):
    cells: Cells
    harness: Harness
    datasheet: vq.FileRef


class Unit(BaseModel):
    symbol: str = "Wh"


class Report[U](BaseModel):
    unit: U
    note: str = "nominal"


class Capacity(Report[Unit]):
    total_wh: float


@bus.calculation()
def capacity(
    cells: Annotated[Cells, vq.Ref("$.cells")],
    gain: Annotated[float, vq.Ref("$.gain")],
) -> Capacity:
    return Capacity(unit=Unit(), total_wh=gain * sum(cell.capacity_wh for cell in cells))


@bus.verification()
def verify_capacity(total_wh: Annotated[float, vq.Ref("@capacity.total_wh")]) -> bool:
    return total_wh > 100.0
