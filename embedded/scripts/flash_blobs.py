# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""
Flash the preflashed (non-OTA) regions of a Pico W with picotool.

These live outside the firmware binary at fixed addresses (see memory_layout.toml):
the CYW43 firmware/CLM/NVRAM blobs, and the per-device WiFi credentials sector, which is
kept out of the publicly served OTA image. Only the WPA2 PSK (PBKDF2 of the passphrase) is
flashed, never the passphrase itself. The credentials format must match
`firmware/src/config/wifi_config.rs`.

Usage (Pico in BOOTSEL mode, run from embedded/):
    uv run scripts/flash_blobs.py --ssid MyNetwork    # CYW43 blobs + credentials
    uv run scripts/flash_blobs.py --only cyw43        # CYW43 blobs only
    uv run scripts/flash_blobs.py --only creds        # credentials only (e.g. new network)
    uv run scripts/flash_blobs.py --only creds --out creds.bin  # write image, don't flash
"""
import argparse
import getpass
import hashlib
import os
import string
import subprocess
import sys
import tempfile
import tomllib
from enum import StrEnum
from pathlib import Path
from typing import NamedTuple

SECTOR_SIZE = 4096
EMBEDDED_DIR = Path(__file__).resolve().parent.parent
BLOB_DIR = EMBEDDED_DIR / "cyw43-firmware"


class Region(NamedTuple):
    origin: int
    length: int


class RegionName(StrEnum):
    """The memory_layout.toml regions this script flashes"""
    WIFI_FW = "WIFI_FW"
    WIFI_CLM = "WIFI_CLM"
    WIFI_NVRAM = "WIFI_NVRAM"
    WIFI_CREDS = "WIFI_CREDS"


# The flash layout shared with the bootloader and firmware builds
with open(EMBEDDED_DIR / "memory_layout.toml", "rb") as f:
    _layout = tomllib.load(f)["regions"]
REGIONS = {name: Region(**_layout[name]) for name in RegionName}

# (region, blob)
CYW43_BLOBS = [
    (RegionName.WIFI_FW, "43439A0.bin"),
    (RegionName.WIFI_CLM, "43439A0_clm.bin"),
    (RegionName.WIFI_NVRAM, "nvram_rp2040.bin"),
]

WIFI_CREDS_ADDR = REGIONS[RegionName.WIFI_CREDS].origin
_creds_format = tomllib.loads((EMBEDDED_DIR / "memory_layout.toml").read_text())["wifi_creds"]
WIFI_CREDS_MAGIC = _creds_format["magic"].encode()
MAX_SSID_LEN = _creds_format["max_ssid_len"]
PSK_LEN = _creds_format["psk_len"]


def pad_to_sector(data: bytes) -> bytes:
    # Fill with erased-flash bytes so every touched sector is rewritten in full
    return data.ljust(-(-len(data) // SECTOR_SIZE) * SECTOR_SIZE, b"\xff")


def wpa2_psk(ssid: bytes, password: str) -> bytes:
    """The 32-byte WPA2 PSK: a 64-hex-digit password is already one, otherwise PBKDF2 it"""
    if len(password) == 64 and all(c in string.hexdigits for c in password):
        return bytes.fromhex(password)
    if not (8 <= len(password) <= 63 and all(32 <= ord(c) <= 126 for c in password)):
        sys.exit("Password must be 8-63 printable ASCII characters, 64 hex digits, or empty for an open network")
    return hashlib.pbkdf2_hmac("sha1", password.encode(), ssid, 4096, 32)


def build_creds_image(ssid: str, password: str) -> bytes:
    ssid_bytes = ssid.encode()
    if not 1 <= len(ssid_bytes) <= MAX_SSID_LEN:
        sys.exit(f"SSID must be 1-{MAX_SSID_LEN} bytes")
    psk = wpa2_psk(ssid_bytes, password) if password else b""
    assert len(psk) in (0, PSK_LEN)

    header = WIFI_CREDS_MAGIC + bytes([len(ssid_bytes), len(psk)])
    return pad_to_sector(header + ssid_bytes + psk)


def picotool_load(addr: int, image: bytes, name: str) -> None:
    fd, path = tempfile.mkstemp(suffix=".bin")
    try:
        with os.fdopen(fd, "wb") as f:
            f.write(image)
        print(f"Flashing {name} at {addr:#x} ({len(image)} bytes)")
        subprocess.run(["picotool", "load", "-v", "-t", "bin", path, "-o", hex(addr)], check=True)
    finally:
        os.remove(path)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--only", choices=["cyw43", "creds"], help="flash only one group of blobs")
    parser.add_argument("--ssid", help="WiFi network name (prompted if omitted)")
    parser.add_argument("--out", help="with --only creds: write the credentials image here instead of flashing")
    parser.add_argument("--no-reboot", action="store_true", help="stay in BOOTSEL after flashing")
    args = parser.parse_args()
    if args.out and args.only != "creds":
        parser.error("--out requires --only creds")

    images: list[tuple[int, bytes, str]] = []

    if args.only != "creds":
        for region, name in CYW43_BLOBS:
            addr, region_size = REGIONS[region]
            blob = (BLOB_DIR / name).read_bytes()
            if len(blob) > region_size:
                sys.exit(f"{name} ({len(blob)} bytes) does not fit its {region_size} byte region")
            images.append((addr, pad_to_sector(blob), name))

    if args.only != "cyw43":
        ssid = args.ssid or input("SSID: ")
        # Prompted rather than passed as an argument to keep it out of shell history
        password = getpass.getpass("Password (empty for open network): ")
        creds = build_creds_image(ssid, password)
        if args.out:
            Path(args.out).write_bytes(creds)
            print(f"Wrote {args.out}; flash with: picotool load -v -t bin {args.out} -o {WIFI_CREDS_ADDR:#x}")
            return
        images.append((WIFI_CREDS_ADDR, creds, "WiFi credentials"))

    for addr, image, name in images:
        picotool_load(addr, image, name)
    if not args.no_reboot:
        subprocess.run(["picotool", "reboot"], check=True)


if __name__ == "__main__":
    main()
