import json
from typing import Literal

from pydantic import BaseModel, model_validator


class AnimationRequest(BaseModel):
    type: Literal["color", "fire", "rainbow", "idle"]
    value: str | None = None

    @model_validator(mode="after")
    def value_required_for_color(self) -> "AnimationRequest":
        if self.type == "color" and self.value is None:
            raise ValueError("value is required when type is 'color'")
        return self

    def serialize_as_rust_enum(self) -> str:
        if self.type == "color":
            return f'{{"Color": {json.dumps(self.value)}}}'
        return f'"{self.type.capitalize()}"'


class AlarmsEnabledRequest(BaseModel):
    enabled: bool
