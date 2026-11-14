"""A small veriq project exercising what the inka adapter must handle.

A diamond (``@budget`` feeds ``?verify_margin`` directly and through
``@reserve``), a table-valued verification, an expected failure, a
``@vq.assume`` edge, nested model and enum classes, and inka assumptions
declared on the root model and on calculations — one of them twice.
"""

from enum import StrEnum, unique
from typing import Annotated

from pydantic import BaseModel

import veriq as vq

project = vq.Project("PowerProject")
power = vq.Scope("Power")
project.add_scope(power)


@unique
class Phase(StrEnum):
    DAYLIGHT = "daylight"
    ECLIPSE = "eclipse"


class Battery(BaseModel):
    capacity_wh: float


@power.root_model(metadata={"inka": {"assumes": ["power::loads_complete"]}})
class Config(BaseModel):
    generation_w: vq.Table[Phase, float]
    consumption_w: vq.Table[Phase, float]
    battery: Battery


class Budget(BaseModel):
    margin_w: vq.Table[Phase, float]


@power.calculation(metadata={"inka": {"assumes": ["power::eclipse_model_valid"]}})
def budget(
    generation_w: Annotated[vq.Table[Phase, float], vq.Ref("$.generation_w")],
    consumption_w: Annotated[vq.Table[Phase, float], vq.Ref("$.consumption_w")],
) -> Budget:
    return Budget(margin_w=vq.Table({p: generation_w[p] - consumption_w[p] for p in Phase}))


@power.calculation(
    metadata={
        "inka": {"assumes": ["power::eclipse_model_valid", "power::battery_model_valid"]},
        "compute_time_s": 3,
    },
)
def reserve(
    battery: Annotated[Battery, vq.Ref("$.battery")],
    margin_w: Annotated[vq.Table[Phase, float], vq.Ref("@budget.margin_w")],
) -> float:
    return battery.capacity_wh + margin_w[Phase.ECLIPSE]


@power.verification()
def verify_margin(
    margin_w: Annotated[vq.Table[Phase, float], vq.Ref("@budget.margin_w")],
    reserve_wh: Annotated[float, vq.Ref("@reserve")],
) -> vq.Table[Phase, bool]:
    return vq.Table({p: margin_w[p] > 0 and reserve_wh > 0 for p in Phase})


@power.verification(xfail=True)
def verify_reserve(reserve_wh: Annotated[float, vq.Ref("@reserve")]) -> bool:
    return reserve_wh > 1000.0


@power.verification()
@vq.assume(vq.Ref("?verify_reserve"))
def verify_assuming(reserve_wh: Annotated[float, vq.Ref("@reserve")]) -> bool:
    return reserve_wh > 0.0
