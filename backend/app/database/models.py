"""
SQLAlchemy models for the database
"""

from datetime import datetime, timezone
from datetime import time as dtime
from uuid import UUID

from sqlalchemy import JSON, DateTime, Integer, String, Time, func
from sqlalchemy.orm import Mapped, mapped_column

from ..models import (
    Alarm,
    AlarmToggleEvent,
    IntensityCurve,
    LogEntry,
    RecurrencePattern,
)
from .setup import Base


class AlarmDB(Base):
    """Database model for alarms"""

    __tablename__ = "alarms"

    id: Mapped[UUID] = mapped_column(primary_key=True)
    created_at: Mapped[datetime] = mapped_column(DateTime(timezone=True))
    updated_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True), onupdate=func.now()
    )
    name: Mapped[str] = mapped_column(String(100))
    time: Mapped[dtime] = mapped_column(Time)
    color: Mapped[str] = mapped_column(String(7))
    length: Mapped[int] = mapped_column(Integer)
    intensity_curve: Mapped[dict] = mapped_column(JSON)
    is_active: Mapped[bool]
    recurrence: Mapped[dict] = mapped_column(JSON)
    play_melody: Mapped[bool] = mapped_column(default=True)

    @classmethod
    def from_model(cls, alarm: Alarm) -> "AlarmDB":
        """Convert from Pydantic model to SQLAlchemy model"""
        return cls(
            id=alarm.id,
            created_at=alarm.created_at,
            updated_at=alarm.updated_at,
            name=alarm.name,
            time=alarm.time,
            color=alarm.color,
            length=alarm.length,
            intensity_curve=alarm.intensity_curve.model_dump(),
            is_active=alarm.is_active,
            recurrence=alarm.recurrence.model_dump(),
            play_melody=alarm.play_melody,
        )

    def to_model(self) -> Alarm:
        """Convert to Pydantic model"""
        return Alarm(
            id=self.id,
            created_at=self.created_at,
            updated_at=self.updated_at,
            name=self.name,
            time=self.time,
            color=self.color,
            length=self.length,
            intensity_curve=IntensityCurve.model_validate(self.intensity_curve),
            is_active=self.is_active,
            recurrence=RecurrencePattern.model_validate(self.recurrence),
            play_melody=self.play_melody,
        )


class LogEntryDB(Base):
    """Database model for log entries"""

    __tablename__ = "logs"

    id: Mapped[int] = mapped_column(primary_key=True, autoincrement=True)
    message: Mapped[str] = mapped_column(String(128))
    timestamp: Mapped[datetime] = mapped_column(
        DateTime(timezone=True), default=lambda: datetime.now(timezone.utc)
    )

    def to_model(self) -> LogEntry:
        """Convert to Pydantic model"""
        return LogEntry(id=self.id, message=self.message, timestamp=self.timestamp)


class AlarmToggleEventDB(Base):
    """Database model for global alarm enable/disable transitions"""

    __tablename__ = "alarm_toggle_events"

    id: Mapped[int] = mapped_column(primary_key=True, autoincrement=True)
    enabled: Mapped[bool]
    changed_at: Mapped[datetime] = mapped_column(
        DateTime(timezone=True),
        index=True,
        default=lambda: datetime.now(timezone.utc),
    )

    def to_model(self) -> AlarmToggleEvent:
        """Convert to Pydantic model"""
        return AlarmToggleEvent(
            id=self.id, enabled=self.enabled, changed_at=self.changed_at
        )
