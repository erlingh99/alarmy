# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Alarmy is a smart light alarm system with three independently-versioned components in one repo:

- `backend/` — FastAPI service (Python, uv) storing alarms in SQLite and pushing schedules/commands to the Pico.
- `frontend/` — React 19 + Vite + Tailwind + shadcn/radix UI for managing alarms. Built and served as static files by the backend (`FRONTEND_DIST` in `backend/app/config.py`), not run standalone in prod.
- `embedded/` — Rust firmware (Embassy async runtime) for a Raspberry Pi Pico W that drives an LED strip and buzzer, workspace with `bootloader` + `firmware` crates.

The whole stack is built via the root multi-stage `Dockerfile` (frontend build → backend deps → firmware build → single production image) and deployed as one container.

## Common Commands

### Backend (`backend/`)
```bash
uv sync --group dev            # install deps
uv run uvicorn app.main:app --reload   # run dev server
uv run pytest                  # run all tests
uv run pytest tests/test_alarm_api.py -k some_test  # run a single test
uv run ruff check               # lint
uv run ty check                 # type check
```
Migrations use Alembic (`backend/alembic.ini`, `backend/migrations/`); `init_db()` in `app/database/setup.py` runs `alembic upgrade head` automatically on startup — new schema changes need a generated migration, not just model edits.

### Frontend (`frontend/`)
```bash
bun install --frozen-lockfile
bun run dev            # vite dev server
bun run build           # production build
bun run check           # tsc --noEmit (app + node configs)
bun run lint             # eslint
bun run test             # vitest run
bun audit                # dependency vulnerability scan
```

### Embedded (`embedded/firmware/`, `embedded/bootloader/`)
WiFi credentials are not compiled in: like the CYW43 blobs they are preflashed per device at a fixed address (`WIFI_CREDS`, the 4 KB sector after the CYW43 blobs) with `uv run scripts/flash_blobs.py` (picotool), so the public OTA `firmware.bin` holds no secrets.
```bash
cargo check     # from embedded/, checks whole workspace
cargo clippy
cargo build --release --bin firmware
```
Target is `thumbv6m-none-eabi` (RP2040). See `embedded/README.md` for flashing and `embedded/memory_layout.toml` for the flash partition layout, from which both crates' `build.rs` generate `memory.x` (bootloader / CYW43 firmware blobs / active app / DFU slot — this matters for the OTA update mechanism in `firmware/src/ota/`).

### PCB (`embedded/pcb/`)
KiCad 10 carrier board for a socketed Pico W. The schematic and board were originally generated from `scripts/design.py`, but both are now **maintained by hand in KiCad** — the layout is placed and routed manually, so `gen_schematic.py` and `gen_pcb.py --force` are destructive and should not be run casually.
```bash
cd embedded/pcb && uv sync
uv run python scripts/verify_netlist.py   # diffs design.py against the schematic, pad by pad
uv run python scripts/export_fab.py       # gerbers, drill, BOM, PDFs, STEP -> fab/
```
`design.py` survives as the netlist cross-check: keep its parts table in step with the schematic so `verify_netlist.py` keeps catching mis-wired nets and reverted footprints. `scripts/kicad_env.py` puts KiCad's bundled `pcbnew` on the uv environment's path. The board uses a **different GPIO map** from the current firmware (I2C1 GP26/GP27 and strip data GP16 match; buttons are GP14/GP12/GP7, buzzer GP15, status LEDs GP2/GP4/GP3) — `embedded/pcb/README.md` lists the firmware changes that implies.

### Full stack
```bash
docker compose up -d --build
```

## Architecture Notes

**Backend ↔ Pico communication is two-way and asymmetric:**
- Backend → Pico: raw TCP, plaintext command strings (`app/communication/tcp_client.py`, `PICO_HOST`/`PICO_PORT` in `config.py`), e.g. sent on startup (`CheckUpdate`) to trigger firmware update checks.
- Pico → Backend: HTTP, the Pico polls the backend's `time_api`/`alarm_api`/`firmware_api` routes for schedules, time sync, and OTA firmware binaries (`embedded/firmware/src/net/http_client.rs`).

**Backend layering**: `routes/` (FastAPI routers) → Pydantic models in `models/` → SQLAlchemy models in `database/models.py`, with `to_model()`/`from_model()` conversion methods bridging the two. `database/interaction.py` holds the actual CRUD used by routes. `app/main.py` wires routers, serves the built frontend as static files, and has a catch-all SPA route (must stay last) for client-side routing.

**Firmware is dual-core**: `main.rs` spawns two Embassy executors — core0 runs networking (`NetworkManager`), time sync (`TimeManager`), the alarm driver, and OTA (`OtaDriver`), all feeding into a `StateMachine`; core1 runs hardware I/O (`LedDriver`, `BuzzerDriver`, buttons, heartbeat) via a separate `HardwareStateMachine`. The two cores communicate through `embassy_sync` signals/channels (e.g. `InputManager`'s signals, `LOGGER_READY`). Logging (`logging/`) fans out to both USB serial and HTTP (to the backend's `logging_api`) via `MultiLogger`.

**OTA/versioning**: firmware version comes from `embedded/firmware/Cargo.toml` (`CARGO_PKG_VERSION_*`), and the Dockerfile extracts it into `firmware/version.txt` alongside the compiled `firmware.bin`, both served by the backend's `firmware_api` for the Pico to check/download updates against.

## CI (`.github/workflows/`)

- `pr-checks.yml`: runs backend (`ty check`, `ruff check`, `pytest`), frontend (`tsc`, `eslint`, `vitest`), and embedded (`cargo check`, `cargo clippy`) independently — mirror these when validating changes locally.
- `deploy.yml`: on push to `main`, builds the multi-arch (arm64) Docker image, pushes to GHCR, then SSHes over Tailscale to a staging host and runs `scripts/restart.sh` (which does `docker compose down` then a `git pull` + `docker compose up -d --build` deploy loop).

## Backups

The `alarmy-backup` service in `docker-compose.yml` (`offen/docker-volume-backup:v2`) archives the `alarmy_data` volume (`alarms.db`) nightly at 03:00 to `${BACKUP_ARCHIVE_ROOT}/alarmy` on the host, keeping 30 days of `.tar.gz` files (older ones pruned automatically). `BACKUP_ARCHIVE_ROOT` must be set on the host (same mechanism as `${DNS_IP}`) to the mount point of the external backup HDD, e.g. `/mnt/backup-hdd`. During each backup the `alarmy` container is briefly stopped and restarted (via the `docker-volume-backup.stop-during-backup` label) to guarantee a consistent copy of the SQLite file.

To restore: `docker compose stop alarmy`, extract the desired `alarmy-<timestamp>.tar.gz` from the archive directory so `alarms.db` lands back at the `alarmy_data` volume's `/app/data` path, then `docker compose start alarmy`.
