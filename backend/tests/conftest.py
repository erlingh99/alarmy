import asyncio

import pytest
from fastapi import FastAPI
from httpx import ASGITransport, AsyncClient
from sqlalchemy.exc import SQLAlchemyError
from sqlalchemy.ext.asyncio import async_sessionmaker, create_async_engine

from app.communication import tcp_client
from app.database.models import Base
from app.database.setup import get_db
from app.routes.alarm_api import alarm_router
from app.routes.health_api import health_router
from app.routes.logging_api import logging_router
from app.routes.mode_api import mode_router
from app.routes.time_api import time_router

TEST_DB_URL = "sqlite+aiosqlite:///:memory:"

GREETING = b"Connected to Pico TCP Server\n"


@pytest.fixture
async def db_engine():
    engine = create_async_engine(TEST_DB_URL)
    async with engine.begin() as conn:
        await conn.run_sync(Base.metadata.create_all)
    yield engine
    await engine.dispose()


@pytest.fixture
async def db_session(db_engine):
    session_maker = async_sessionmaker(db_engine, expire_on_commit=False)
    async with session_maker() as session:
        yield session


async def _serve_like_pico(reply: bytes, *, greeting: bytes = GREETING):
    """Start a server mimicking the Pico's TCP protocol.

    The Pico writes a greeting line as soon as it accepts a connection, before
    it reads any command, then answers each command with its own line.
    """

    async def handle(reader: asyncio.StreamReader, writer: asyncio.StreamWriter):
        writer.write(greeting)
        await writer.drain()
        if await reader.read(1024):
            writer.write(reply)
            await writer.drain()
        writer.close()

    server = await asyncio.start_server(handle, "127.0.0.1", 0)
    return server, server.sockets[0].getsockname()[1]


@pytest.fixture
async def pico(monkeypatch):
    servers: list[asyncio.AbstractServer] = []

    async def _start(reply: bytes, **kwargs):
        server, port = await _serve_like_pico(reply, **kwargs)
        servers.append(server)
        monkeypatch.setattr(tcp_client, "PICO_HOST", "127.0.0.1")
        monkeypatch.setattr(tcp_client, "PICO_PORT", port)

    yield _start

    for server in servers:
        server.close()
        await server.wait_closed()


@pytest.fixture
async def client(db_engine):
    session_maker = async_sessionmaker(db_engine, expire_on_commit=False)

    async def override_get_db():
        async with session_maker() as session:
            try:
                yield session
                await session.commit()
            except SQLAlchemyError:
                await session.rollback()
                raise

    test_app = FastAPI()
    test_app.include_router(alarm_router)
    test_app.include_router(health_router)
    test_app.include_router(logging_router)
    test_app.include_router(time_router)
    test_app.include_router(mode_router)
    test_app.dependency_overrides[get_db] = override_get_db

    async with AsyncClient(
        transport=ASGITransport(app=test_app), base_url="http://test"
    ) as ac:
        yield ac
