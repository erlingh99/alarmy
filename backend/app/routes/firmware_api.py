from fastapi import APIRouter, HTTPException
from fastapi.responses import FileResponse

from ..communication import send_message_to_led_controller
from ..config import FIRMWARE_DIR

firmware_router = APIRouter(prefix="/api/firmware", tags=["firmware"])


@firmware_router.get("/version")
async def get_firmware_version() -> dict[str, int]:
    version_file = FIRMWARE_DIR / "version.txt"
    if not version_file.exists():
        raise HTTPException(status_code=404, detail="No firmware available")
    major, minor, patch = version_file.read_text().strip().split(".")
    return {"major": int(major), "minor": int(minor), "patch": int(patch)}


@firmware_router.get("/device-version")
async def get_device_version() -> dict[str, int]:
    response = await send_message_to_led_controller('"Version"')
    if response is None or not response.strip().startswith("VERSION:"):
        raise HTTPException(status_code=500, detail="Unexpected response from device")
    version_str = response.strip().removeprefix("VERSION:").strip()
    major, minor, patch = version_str.split(".")
    return {"major": int(major), "minor": int(minor), "patch": int(patch)}


@firmware_router.get("/binary")
async def get_firmware_binary() -> FileResponse:
    binary_file = FIRMWARE_DIR / "firmware.bin"
    if not binary_file.exists():
        raise HTTPException(status_code=404, detail="No firmware available")
    return FileResponse(
        binary_file, media_type="application/octet-stream", filename="firmware.bin"
    )
