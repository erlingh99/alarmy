from datetime import date as date_t
from datetime import datetime, timezone
from datetime import time as time_t
from typing import Literal
from uuid import UUID, uuid4

from pydantic import BaseModel, Field, field_validator


class RecurrencePattern(BaseModel):
    recurrence_type: Literal["daily", "weekly", "custom"]
    days: tuple[bool, bool, bool, bool, bool, bool, bool] | None = None
    custom_dates: list[date_t] | None = None

    @field_validator("custom_dates")
    @classmethod
    def max_five_dates(cls, v: list[date_t] | None) -> list[date_t] | None:
        if v is not None and len(v) > 5:
            raise ValueError("custom_dates may contain at most 5 dates")
        return v


class ControlPoint(BaseModel):
    x: float = Field(ge=0, le=100)
    y: float = Field(ge=0, le=100)


class IntensityCurve(BaseModel):
    start_intensity: int = Field(ge=0, le=100)
    end_intensity: int = Field(ge=0, le=100)
    curve: Literal["linear", "asymptotic", "s-curve", "custom"]
    hyper_parameter: int | None = Field(default=None, ge=-128, le=127)
    control_points: list[ControlPoint] | None = None

    @field_validator("control_points")
    @classmethod
    def max_five_points(cls, v: list[ControlPoint] | None) -> list[ControlPoint] | None:
        if v is not None and len(v) > 5:
            raise ValueError("control_points may contain at most 5 points")
        return v


class Alarm(BaseModel):
    id: UUID = Field(default_factory=uuid4)
    created_at: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))
    updated_at: datetime = Field(default_factory=lambda: datetime.now(timezone.utc))
    name: str = Field(min_length=1, max_length=32)
    time: time_t
    color: str = Field(pattern=r"^#[0-9A-Fa-f]{6}$")
    length: int = Field(ge=1, le=255)
    intensity_curve: IntensityCurve
    is_active: bool
    recurrence: RecurrencePattern
    play_melody: bool = True


class AlarmUpdate(BaseModel):
    name: str | None = Field(default=None, min_length=1, max_length=32)
    time: time_t | None = None
    color: str | None = Field(default=None, pattern=r"^#[0-9A-Fa-f]{6}$")
    length: int | None = Field(default=None, ge=1, le=255)
    intensity_curve: IntensityCurve | None = None
    is_active: bool | None = None
    recurrence: RecurrencePattern | None = None
    play_melody: bool | None = None


class AlarmSync(BaseModel):
    """Everything the Pico needs in one payload.

    The flag travels with the alarms so there is no window on boot where the
    device holds alarms but not yet the arming state.
    """

    alarms: list[Alarm]
    alarms_enabled: bool
