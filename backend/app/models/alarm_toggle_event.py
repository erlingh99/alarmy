from datetime import datetime

from pydantic import BaseModel


class AlarmToggleEvent(BaseModel):
    """A single global alarm enable/disable transition."""

    id: int | None = None
    enabled: bool
    changed_at: datetime | None = None
