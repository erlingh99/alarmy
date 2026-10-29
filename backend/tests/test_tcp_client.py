import asyncio

import pytest

from app.communication import tcp_client

GREETING = b"Connected to Pico TCP Server\n"


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


async def test_returns_command_response_not_greeting(pico):
    await pico(b"VERSION: 1.0.0\n")

    response = await tcp_client.send_message_to_led_controller('"Version"')

    assert response is not None
    assert response.strip() == "VERSION: 1.0.0"


async def test_returns_response_when_device_sends_no_greeting(pico):
    await pico(b"OK: Command queued\n", greeting=b"")

    response = await tcp_client.send_message_to_led_controller('"AlarmsUpdated"')

    assert response is not None
    assert response.strip() == "OK: Command queued"


async def test_returns_none_when_device_unreachable(monkeypatch):
    monkeypatch.setattr(tcp_client, "PICO_HOST", "127.0.0.1")
    monkeypatch.setattr(tcp_client, "PICO_PORT", 1)

    assert await tcp_client.send_message_to_led_controller("x", rethrow=False) is None
