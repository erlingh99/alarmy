from datetime import datetime, timezone
from uuid import UUID

from fastapi import APIRouter, BackgroundTasks, HTTPException

from ..communication import send_message_to_led_controller
from ..database import delete_alarm as db_delete_alarm
from ..database import get_alarm_by_id, get_alarms_enabled, load_alarms, save_alarm
from ..database import update_alarm as db_update_alarm
from ..dependencies import DbSession
from ..models import Alarm, AlarmSync, AlarmUpdate

alarm_router = APIRouter(prefix="/api/alarms", tags=["alarms"])


@alarm_router.get("", response_model=list[Alarm])
async def get_alarms(db: DbSession) -> list[Alarm]:
    alarms = await load_alarms(db)
    return list(alarms.values())


@alarm_router.get("/sync", response_model=AlarmSync)
async def sync_alarms(db: DbSession) -> AlarmSync:
    """Device-facing payload: alarms and arming state in one response."""
    alarms = await load_alarms(db)
    return AlarmSync(
        alarms=list(alarms.values()),
        alarms_enabled=await get_alarms_enabled(db),
    )


@alarm_router.get(
    "/{alarm_id}",
    response_model=Alarm,
    responses={404: {"description": "Alarm not found"}},
)
async def get_alarm(alarm_id: UUID, db: DbSession) -> Alarm:
    alarm = await get_alarm_by_id(db, alarm_id)
    if alarm:
        return alarm
    raise HTTPException(status_code=404, detail="Alarm not found")


@alarm_router.delete("/{alarm_id}", status_code=204)
async def delete_alarm(
    alarm_id: UUID, background_tasks: BackgroundTasks, db: DbSession
) -> None:
    await db_delete_alarm(db, alarm_id)
    background_tasks.add_task(
        send_message_to_led_controller, '"AlarmsUpdated"', rethrow=False
    )


@alarm_router.put("", response_model=Alarm, status_code=200)
async def set_alarm(alarm: Alarm, background_tasks: BackgroundTasks, db: DbSession):
    await save_alarm(db, alarm)
    background_tasks.add_task(
        send_message_to_led_controller, '"AlarmsUpdated"', rethrow=False
    )
    return alarm


@alarm_router.patch(
    "/{alarm_id}",
    response_model=Alarm,
    responses={404: {"description": "Alarm not found"}},
)
async def update_alarm(
    alarm_id: UUID,
    partial_alarm: AlarmUpdate,
    background_tasks: BackgroundTasks,
    db: DbSession,
):
    alarm = await get_alarm_by_id(db, alarm_id)
    if alarm:
        update_data = alarm.model_dump() | partial_alarm.model_dump(exclude_unset=True)
        updated_alarm = Alarm(**update_data)
        updated_alarm.updated_at = datetime.now(timezone.utc)
        await db_update_alarm(db, updated_alarm)
        background_tasks.add_task(
            send_message_to_led_controller, '"AlarmsUpdated"', rethrow=False
        )
        return updated_alarm
    raise HTTPException(status_code=404, detail="Alarm not found")
