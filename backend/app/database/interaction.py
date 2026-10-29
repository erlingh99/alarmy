"""
SQLite storage backend for alarms and logs
"""

from datetime import datetime, timedelta, timezone
from uuid import UUID

from sqlalchemy import delete, select, update
from sqlalchemy.ext.asyncio import AsyncSession

from ..models import Alarm, AlarmToggleEvent, LogEntry
from .models import AlarmDB, AlarmToggleEventDB, LogEntryDB


async def load_alarms(db: AsyncSession) -> dict[UUID, Alarm]:
    """Load alarms from the database."""
    result = await db.execute(select(AlarmDB))
    alarms = result.scalars().all()
    return {alarm.id: alarm.to_model() for alarm in alarms}


async def get_alarm_by_id(db: AsyncSession, alarm_id: UUID) -> Alarm | None:
    """Get a single alarm by ID."""
    result = await db.execute(select(AlarmDB).where(AlarmDB.id == alarm_id))
    db_alarm = result.scalar_one_or_none()
    return db_alarm.to_model() if db_alarm else None


async def save_alarm(db: AsyncSession, alarm: Alarm) -> None:
    """Save a single alarm to the database."""
    db_alarm = AlarmDB.from_model(alarm)
    db.add(db_alarm)


async def update_alarm(db: AsyncSession, alarm: Alarm) -> None:
    """Update a single alarm in the database."""
    db_alarm = AlarmDB.from_model(alarm)
    await db.execute(
        update(AlarmDB)
        .where(AlarmDB.id == db_alarm.id)
        .values(
            name=db_alarm.name,
            time=db_alarm.time,
            color=db_alarm.color,
            length=db_alarm.length,
            intensity_curve=db_alarm.intensity_curve,
            is_active=db_alarm.is_active,
            recurrence=db_alarm.recurrence,
            play_melody=db_alarm.play_melody,
            updated_at=db_alarm.updated_at,
        )
    )


async def delete_alarm(db: AsyncSession, alarm_id: UUID) -> None:
    """Delete an alarm from the database."""
    result = await db.execute(select(AlarmDB).where(AlarmDB.id == alarm_id))
    alarm = result.scalar_one_or_none()
    if alarm:
        await db.delete(alarm)


async def cleanup_old_logs(db: AsyncSession) -> None:
    """Delete logs older than 2 weeks."""
    two_weeks_ago = datetime.now(timezone.utc) - timedelta(weeks=2)
    await db.execute(delete(LogEntryDB).where(LogEntryDB.timestamp < two_weeks_ago))


async def save_logs(db: AsyncSession, entries: list[LogEntry]) -> None:
    db.add_all(LogEntryDB(message=e.message) for e in entries)
    await cleanup_old_logs(db)


async def get_logs(db: AsyncSession, limit: int = 100) -> list[LogEntry]:
    """Get the most recent log entries."""
    result = await db.execute(
        select(LogEntryDB).order_by(LogEntryDB.id.desc()).limit(limit)
    )
    logs = result.scalars().all()
    return [log.to_model() for log in logs]


ALARMS_ENABLED_DEFAULT = True

_TOGGLE_ORDER = (AlarmToggleEventDB.changed_at.desc(), AlarmToggleEventDB.id.desc())


async def get_alarms_enabled(db: AsyncSession) -> bool:
    """Whether alarms are globally armed. The newest transition wins."""
    result = await db.execute(
        select(AlarmToggleEventDB.enabled).order_by(*_TOGGLE_ORDER).limit(1)
    )
    latest = result.scalar_one_or_none()
    return ALARMS_ENABLED_DEFAULT if latest is None else latest


async def set_alarms_enabled(db: AsyncSession, enabled: bool) -> bool:
    """Record a transition. Returns False and writes nothing if already in that state."""
    if await get_alarms_enabled(db) == enabled:
        return False
    db.add(AlarmToggleEventDB(enabled=enabled))
    # Flushing here is deliberate: consecutive calls within one request must see
    # each other's writes for the transition check above to work. get_db() still
    # owns the commit.
    await db.flush()
    return True


async def load_toggle_history(
    db: AsyncSession, limit: int = 50
) -> list[AlarmToggleEvent]:
    """Most recent alarm enable/disable transitions, newest first."""
    result = await db.execute(
        select(AlarmToggleEventDB).order_by(*_TOGGLE_ORDER).limit(limit)
    )
    return [event.to_model() for event in result.scalars().all()]
