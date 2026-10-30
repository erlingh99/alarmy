# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""
Build the firmware and flash it into the ACTIVE partition with picotool.

For first-time setup or recovery; normally the firmware is updated over OTA. Requires the
bootloader and blobs to be flashed already (flash_bootloader.py, flash_blobs.py).
Run from anywhere, with the Pico in BOOTSEL mode:
    uv run scripts/flash_firmware.py
"""

import argparse
import subprocess
import tomllib
from enum import StrEnum
from pathlib import Path
from typing import NamedTuple

EMBEDDED_DIR = Path(__file__).resolve().parent.parent
BIN = "firmware"
ELF = EMBEDDED_DIR / "target" / "thumbv6m-none-eabi" / "release" / BIN


class Region(NamedTuple):
    origin: int
    length: int


class RegionName(StrEnum):
    """The memory_layout.toml regions this script erases"""
    BOOTLOADER_STATE = "BOOTLOADER_STATE"
    OTA_RECORD = "OTA_RECORD"


with open(EMBEDDED_DIR / "memory_layout.toml", "rb") as f:
    _layout = tomllib.load(f)["regions"]
REGIONS = {name: Region(**_layout[name]) for name in RegionName}

# Erased before flashing: BOOTLOADER_STATE so the freshly flashed firmware isn't reverted by a
# leftover swap state, OTA_RECORD so a version blacklisted by an earlier failed update can be
# installed again
ERASED_REGIONS = [RegionName.BOOTLOADER_STATE, RegionName.OTA_RECORD]


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--no-build", action="store_true", help="flash the existing release ELF without rebuilding")
    parser.add_argument("--no-reboot", action="store_true", help="stay in BOOTSEL after flashing")
    args = parser.parse_args()

    if not args.no_build:
        subprocess.run(["cargo", "build", "-r", "-p", BIN], cwd=EMBEDDED_DIR, check=True)

    for name in ERASED_REGIONS:
        region = REGIONS[name]
        print(f"Erasing {name}")
        subprocess.run(
            ["picotool", "erase", "-r", hex(region.origin), hex(region.origin + region.length)],
            check=True,
        )

    print(f"Flashing {ELF}")
    subprocess.run(["picotool", "load", "--update", "--verify", "-t", "elf", str(ELF)], check=True)
    if not args.no_reboot:
        subprocess.run(["picotool", "reboot"], check=True)


if __name__ == "__main__":
    main()
