from fastapi import APIRouter, BackgroundTasks, HTTPException, Query

from ..communication import send_message_to_led_controller
from ..database import get_alarms_enabled, load_toggle_history, set_alarms_enabled
from ..dependencies import DbSession
from ..models import AlarmsEnabledRequest, AlarmToggleEvent, AnimationRequest

mode_router = APIRouter(prefix="/api/mode", tags=["mode"])


@mode_router.post("/animation")
async def post_animation(body: AnimationRequest):
    response = await send_message_to_led_controller(body.serialize_as_rust_enum())

    if not response or "OK" not in response:
        raise HTTPException(status_code=400, detail=response)
    return {"message": "Command sent successfully"}


@mode_router.get("/alarms")
async def get_alarms_mode(db: DbSession) -> dict[str, bool]:
    return {"enabled": await get_alarms_enabled(db)}


@mode_router.put("/alarms")
async def put_alarms_mode(
    body: AlarmsEnabledRequest, background_tasks: BackgroundTasks, db: DbSession
) -> dict[str, bool]:
    if await set_alarms_enabled(db, body.enabled):
        background_tasks.add_task(
            send_message_to_led_controller, '"AlarmsUpdated"', rethrow=False
        )
    return {"enabled": body.enabled}


@mode_router.get("/alarms/history", response_model=list[AlarmToggleEvent])
async def get_alarms_history(
    db: DbSession, limit: int = Query(default=50, ge=1, le=500)
) -> list[AlarmToggleEvent]:
    return await load_toggle_history(db, limit)
