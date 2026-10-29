from fastapi import APIRouter

health_router = APIRouter(prefix="/api/health", tags=["health"])


@health_router.get("")
async def get_health() -> dict[str, str]:
    return {"status": "ok"}
