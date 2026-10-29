from typing import Annotated

from fastapi import APIRouter
from pydantic import Field

from ..database import get_logs, save_logs
from ..dependencies import DbSession
from ..models import LogEntry

logging_router = APIRouter(prefix="/api/logs", tags=["log"])

LogMessage = Annotated[str, Field(min_length=1, max_length=256)]
LogBatch = Annotated[list[LogMessage], Field(min_length=1)]


@logging_router.post("")
async def post_log(messages: LogBatch, db: DbSession):
    await save_logs(db, [LogEntry(message=m) for m in messages])
    return {"message": "Logs saved successfully"}


@logging_router.get("", response_model=list[LogEntry])
async def get_log_entries(db: DbSession, max: int = 100):
    return await get_logs(db, max)
