import os
from datetime import datetime, timedelta
from zoneinfo import ZoneInfo

from fastapi import APIRouter

time_router = APIRouter(prefix="/api/time", tags=["time"])


@time_router.get("")
async def get_time() -> dict[str, int]:
    now = datetime.now(ZoneInfo(os.environ.get("TZ", "UTC")))
    utc_offset = now.utcoffset() or timedelta()
    return {
        "timestamp": int(now.timestamp()),
        "utc_offset_seconds": int(utc_offset.total_seconds()),
    }
