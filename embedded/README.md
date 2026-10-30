# Raspberry Pi Pico W Alarm Controller

This is the embedded component of the alarm project, implemented in Rust using the Embassy async runtime. It controls a PWM-capable LED strip and optional buzzer, fetching alarm schedules from the backend.

## Provisioning a device

The CYW43 WiFi chip blobs and the WiFi credentials are not part of the firmware binary. They are
flashed once per device at fixed addresses (see `memory_layout.toml`), so the firmware served
publicly for OTA contains no secrets. All scripts use picotool and need the Pico in BOOTSEL mode.

A new device, from `embedded/`:

```bash
uv run scripts/flash_blobs.py --no-reboot          # CYW43 blobs + credentials (prompts for SSID and password)
uv run scripts/flash_bootloader.py --no-reboot     # builds and flashes the bootloader
uv run scripts/flash_firmware.py                   # builds and flashes the firmware, then reboots
```

After that the firmware updates itself over OTA. Other useful variants:

```bash
uv run scripts/flash_blobs.py --only creds --ssid MyNetwork   # change network
uv run scripts/flash_blobs.py --only cyw43                    # chip blobs only
uv run scripts/flash_firmware.py --no-build                   # flash the existing release ELF
```

Without credentials the firmware boots but logs `No usable WiFi credentials in flash` over USB
serial every minute.
