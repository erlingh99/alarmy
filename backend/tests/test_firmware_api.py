import pytest

from app.routes import firmware_api


@pytest.fixture
def firmware_dir(tmp_path, monkeypatch):
    monkeypatch.setattr(firmware_api, "FIRMWARE_DIR", tmp_path)
    return tmp_path


async def test_version(client, firmware_dir):
    (firmware_dir / "version.txt").write_text("1.2.3\n")

    body = (await client.get("/api/firmware/version")).json()
    assert body == {"major": 1, "minor": 2, "patch": 3}


async def test_signature(client, firmware_dir):
    (firmware_dir / "firmware.sig").write_text("ab" * 64 + "\n")

    body = (await client.get("/api/firmware/signature")).json()
    assert body == {"signature": "ab" * 64}


async def test_signature_missing(client, firmware_dir):
    assert (await client.get("/api/firmware/signature")).status_code == 404
