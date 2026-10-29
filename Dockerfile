# Build frontend
FROM oven/bun:alpine AS frontend-builder
WORKDIR /app
COPY frontend/package.json frontend/bun.lock ./
COPY ./frontend/ .
RUN bun install --frozen-lockfile && bun run build

# Build backend
FROM ghcr.io/astral-sh/uv:python3.13-alpine AS backend-builder
WORKDIR /app
COPY ./backend/ .
ENV UV_PROJECT_ENVIRONMENT=.venv
RUN uv sync --frozen --no-dev

# Firmware base: install tools once (cached layer)
FROM rust:alpine AS firmware-base
RUN rustup target add thumbv6m-none-eabi && \
    rustup component add llvm-tools && \
    cargo install flip-link && \
    cargo install cargo-binutils

# Builder: compile only the firmware binary
FROM firmware-base AS firmware-builder
WORKDIR /firmware
COPY embedded/ .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/firmware/target \
    --mount=type=secret,id=wifi_ssid \
    --mount=type=secret,id=wifi_password \
    export WIFI_SSID="$(cat /run/secrets/wifi_ssid)" && \
    export WIFI_PASSWORD="$(cat /run/secrets/wifi_password)" && \
    cargo build --release --bin alarmy-firmware && \
    cargo objcopy --release --bin alarmy-firmware -- -O binary firmware.bin && \
    awk -F'"' '/^version/{print $2; exit}' ./firmware/Cargo.toml > version.txt

# Final stage
FROM python:3.13-alpine AS prod

ARG BACKEND_HOST
ARG BACKEND_PORT

ENV BACKEND_HOST=${BACKEND_HOST} \
    BACKEND_PORT=${BACKEND_PORT} \
    PATH="/app/.venv/bin:$PATH" \
    TZ=Europe/Berlin

COPY --from=backend-builder /app/.venv /app/.venv

WORKDIR /app

COPY ./backend/app ./backend/app
COPY ./backend/migrations ./backend/migrations
COPY ./backend/alembic.ini ./backend/alembic.ini
COPY --from=frontend-builder /app/dist ./frontend/dist
COPY --from=firmware-builder /firmware/firmware.bin ./firmware/firmware.bin
COPY --from=firmware-builder /firmware/version.txt ./firmware/version.txt

RUN apk add --no-cache curl tzdata

HEALTHCHECK --interval=5m --timeout=30s --start-period=5s --retries=3 \
    CMD curl "http://localhost:${BACKEND_PORT}/api/health"

WORKDIR /app/backend
CMD ["sh", "-c", "uvicorn app.main:app --host \"${BACKEND_HOST}\" --port \"${BACKEND_PORT}\""]
