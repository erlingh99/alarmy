from contextlib import asynccontextmanager

from fastapi import FastAPI, HTTPException
from fastapi.middleware.gzip import GZipMiddleware
from fastapi.responses import FileResponse
from fastapi.staticfiles import StaticFiles

from .communication import send_message_to_led_controller
from .config import FRONTEND_DIST
from .database import init_db
from .routes import (
    alarm_api,
    firmware_api,
    health_api,
    logging_api,
    mode_api,
    time_api,
)


@asynccontextmanager
async def lifespan(app: FastAPI):
    await init_db()
    await send_message_to_led_controller('"CheckUpdate"', rethrow=False)
    yield


app = FastAPI(lifespan=lifespan)
app.add_middleware(GZipMiddleware, minimum_size=1000)

# Mount static files for assets
app.mount(
    "/assets", StaticFiles(directory=str(FRONTEND_DIST / "assets")), name="assets"
)

# Include API routers
app.include_router(alarm_api.alarm_router)
app.include_router(time_api.time_router)
app.include_router(mode_api.mode_router)
app.include_router(firmware_api.firmware_router)
app.include_router(health_api.health_router)
app.include_router(logging_api.logging_router)


# Catch-all route for SPA - must be last
@app.get("/{full_path:path}", include_in_schema=False)
async def catch_all(full_path: str):
    if full_path.startswith("api/"):
        raise HTTPException(status_code=404, detail="Not found")
    candidate = FRONTEND_DIST / full_path
    if candidate.is_file():
        return FileResponse(candidate)
    return FileResponse(FRONTEND_DIST / "index.html", media_type="text/html")
