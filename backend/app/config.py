from pathlib import Path

PICO_HOST = "alarmy-pico.lan"
PICO_PORT = 8080

DATABASE_PATH = "sqlite+aiosqlite:////app/data/alarms.db"

FRONTEND_DIST = Path(__file__).parent.parent.parent / "frontend" / "dist"
MIGRATIONS_PATH = Path(__file__).parent.parent / "alembic.ini"
FIRMWARE_DIR = Path("/app/firmware")
