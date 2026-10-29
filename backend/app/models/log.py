from datetime import datetime

from pydantic import BaseModel, Field


class LogEntry(BaseModel):
    id: int | None = None
    message: str = Field(max_length=256)
    timestamp: datetime | None = None
