import asyncio
import logging

from fastapi import HTTPException

from ..config import PICO_HOST, PICO_PORT

logger = logging.getLogger(__name__)

# The Pico writes this line as soon as it accepts a connection, before it has
# read any command, so it arrives ahead of the response we actually asked for.
GREETING_PREFIX = "Connected to Pico"


async def _read_command_response(reader: asyncio.StreamReader) -> str | None:
    """Read the device's answer to a command, skipping its connect greeting."""
    while True:
        line = await reader.readline()
        if not line:
            return None
        try:
            text = line.decode()
        except UnicodeDecodeError:
            return None
        if not text.startswith(GREETING_PREFIX):
            return text


async def send_message_to_led_controller(
    message: str, rethrow: bool = True
) -> str | None:
    writer = None
    try:
        reader, writer = await asyncio.wait_for(
            asyncio.open_connection(PICO_HOST, PICO_PORT), timeout=10.0
        )
        writer.write(message.encode())
        await writer.drain()

        return await asyncio.wait_for(_read_command_response(reader), timeout=10.0)
    except (asyncio.TimeoutError, OSError) as e:
        logger.error("LED controller connection failed: %s", e)
        if rethrow:
            raise HTTPException(
                status_code=500,
                detail="Service unavailable: could not reach LED controller",
            )
        return None
    finally:
        if writer is not None:
            try:
                writer.close()
                await writer.wait_closed()
            except (OSError, asyncio.CancelledError):
                pass
