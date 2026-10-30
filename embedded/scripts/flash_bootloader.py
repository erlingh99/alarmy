# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""
Build the bootloader and flash it with picotool.

Only needed once per device (or when the bootloader itself changes); the firmware is
updated over OTA after that. Run from anywhere, with the Pico in BOOTSEL mode:
    uv run scripts/flash_bootloader.py
"""

import argparse
import subprocess
from pathlib import Path

EMBEDDED_DIR = Path(__file__).resolve().parent.parent
BIN = "bootloader"
ELF = EMBEDDED_DIR / "target" / "thumbv6m-none-eabi" / "release" / BIN


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--no-build", action="store_true", help="flash the existing release ELF without rebuilding")
    parser.add_argument("--no-reboot", action="store_true", help="stay in BOOTSEL after flashing")
    args = parser.parse_args()

    if not args.no_build:
        subprocess.run(["cargo", "build", "-r", "-p", BIN], cwd=EMBEDDED_DIR, check=True)

    print(f"Flashing {ELF}")
    subprocess.run(["picotool", "load", "--update", "--verify", "-t", "elf", str(ELF)], check=True)
    if not args.no_reboot:
        subprocess.run(["picotool", "reboot"], check=True)


if __name__ == "__main__":
    main()
