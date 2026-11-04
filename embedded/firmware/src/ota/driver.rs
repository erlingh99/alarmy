use core::cell::RefCell;

use embassy_boot_rp::{AlignedBuffer, BlockingFirmwareUpdater, FirmwareUpdaterConfig, State};
use embassy_executor::Spawner;
use embassy_rp::flash::{Blocking, Flash};
use embassy_rp::gpio::{AnyPin, Level, Output};
use embassy_rp::peripherals::FLASH;
use embassy_rp::Peri;
use embassy_sync::blocking_mutex::raw::{CriticalSectionRawMutex, NoopRawMutex};
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Instant, Timer};
use serde::Deserialize;

use crate::config::api_config::{FIRMWARE_BINARY_ROUTE, RETRY_DELAY_S};
use crate::crash;
use crate::net::{HttpClientManager, NetworkError};
use crate::watchdog;

use super::record::{self, Version};

const FLASH_SIZE: usize = 2 * 1024 * 1024;
const BOOT_CONFIRM_DEADLINE: Duration = Duration::from_secs(10 * 60);

// RP2040 flash write alignment — pad partial final chunks to this boundary.
const WRITE_SIZE: usize = 256;

const fn parse_u32(s: &[u8]) -> u32 {
    let mut val: u32 = 0;
    let mut i = 0;
    while i < s.len() {
        val = val * 10 + (s[i] - b'0') as u32;
        i += 1;
    }
    val
}

const CURRENT_MAJOR: u32 = parse_u32(env!("CARGO_PKG_VERSION_MAJOR").as_bytes());
const CURRENT_MINOR: u32 = parse_u32(env!("CARGO_PKG_VERSION_MINOR").as_bytes());
const CURRENT_PATCH: u32 = parse_u32(env!("CARGO_PKG_VERSION_PATCH").as_bytes());

#[derive(Debug)]
pub enum OtaError {
    Network,
    Flash,
    /// A version part over 255, which the 8 bit OTA record can't hold
    UnsupportedVersion,
}

#[derive(Deserialize)]
pub struct VersionResponse {
    major: u32,
    minor: u32,
    patch: u32,
}

pub(super) type FlashMutex =
    Mutex<NoopRawMutex, RefCell<Flash<'static, FLASH, Blocking, FLASH_SIZE>>>;

pub struct OtaPinning {
    pub flash: Peri<'static, FLASH>,
    pub download_pin: Peri<'static, AnyPin>,
}

pub struct OtaDriver {
    http_client: &'static HttpClientManager,
    flash: FlashMutex,
    download_pin: Output<'static>,
}

pub struct OtaBootState {
    flash: FlashMutex,
    download_pin: Output<'static>,
    needs_confirmation: bool,
}

pub static OTA_TRIGGER: Signal<CriticalSectionRawMutex, ()> = Signal::new();

impl OtaDriver {
    /// Check the bootloader state at boot. A boot after a swap, arms the watchdog's confirmation deadline
    pub fn check_boot(pinning: OtaPinning) -> OtaBootState {
        let flash = Flash::<_, _, FLASH_SIZE>::new_blocking(pinning.flash);
        let flash = Mutex::new(RefCell::new(flash));
        let download_pin = Output::new(pinning.download_pin, Level::Low);

        // An unreadable state is treated as a swap
        let needs_confirmation = match Self::updater_state(&flash) {
            Ok(State::Swap) => {
                log::info!(
                    "OTA: Running freshly swapped firmware, confirming once the device is up"
                );
                watchdog::require_boot_confirmation(Instant::now() + BOOT_CONFIRM_DEADLINE);
                true
            }
            Ok(State::Revert) => {
                Self::mark_booted(&flash).ok();
                match Self::record_failed_update() {
                    Some((major, minor, patch)) => log::warn!(
                        "OTA: Bootloader reverted v{major}.{minor}.{patch}, skipping it until a power cycle"
                    ),
                    None => log::warn!(
                        "OTA: Bootloader reverted an update whose version was lost to a power cycle"
                    ),
                }
                false
            }
            Ok(_) => {
                //acknowledge the boot directly, so the bootloader doesn't revert
                Self::mark_booted(&flash).ok();
                false
            }
            Err(e) => {
                log::error!(
                    "OTA: Could not read bootloader state ({:?}), requiring confirmation",
                    e
                );
                watchdog::require_boot_confirmation(Instant::now() + BOOT_CONFIRM_DEADLINE);
                true
            }
        };

        OtaBootState {
            flash,
            download_pin,
            needs_confirmation,
        }
    }

    pub fn initialize(
        spawner: Spawner,
        http_client: &'static HttpClientManager,
        boot_state: OtaBootState,
    ) {
        let driver = OtaDriver {
            http_client,
            flash: boot_state.flash,
            download_pin: boot_state.download_pin,
        };
        //TODO this function does both the confirmation and the starting
        spawner.spawn(ota_task(driver, boot_state.needs_confirmation).unwrap());
    }

    fn updater_state(flash: &FlashMutex) -> Result<State, OtaError> {
        let config = FirmwareUpdaterConfig::from_linkerfile_blocking(flash, flash);
        let mut aligned = AlignedBuffer([0u8; 1]);
        BlockingFirmwareUpdater::new(config, &mut aligned.0)
            .get_state()
            .map_err(|_| OtaError::Flash)
    }

    /// Confirm this boot so the bootloader won't revert on watchdog reset
    fn mark_booted(flash: &FlashMutex) -> Result<(), OtaError> {
        let config = FirmwareUpdaterConfig::from_linkerfile_blocking(flash, flash);
        let mut aligned = AlignedBuffer([0u8; 1]);
        BlockingFirmwareUpdater::new(config, &mut aligned.0)
            .mark_booted()
            .map_err(|_| OtaError::Flash)
    }

    /// Moves `pending` to `failed`. None if a power cycle lost it
    fn record_failed_update() -> Option<Version> {
        let version = record::pending()?;
        record::clear_pending();
        record::set_failed(version);
        Some(version)
    }

    /// Retries until the watchdog deadline ends it
    async fn confirm_boot(&self) {
        while let Err(e) = self.http_client.get_health().await {
            log::warn!(
                "OTA: Backend unreachable, firmware stays unconfirmed ({:?})",
                e
            );
            Timer::after_secs(RETRY_DELAY_S).await;
        }
        match Self::mark_booted(&self.flash) {
            Ok(()) => {
                log::info!("OTA: Backend reachable, firmware confirmed");
                record::clear_pending();
                watchdog::boot_confirmed();
            }
            Err(e) => {
                log::error!(
                    "OTA: Failed to confirm firmware, waiting for the watchdog to revert: {:?}",
                    e
                );
                core::future::pending::<()>().await;
            }
        }
    }

    async fn run_update(&self) -> Result<(), OtaError> {
        let resp = self
            .http_client
            .get_firmware_version()
            .await
            .map_err(|_| OtaError::Network)?;

        let available = (resp.major, resp.minor, resp.patch);
        if available <= (CURRENT_MAJOR, CURRENT_MINOR, CURRENT_PATCH) {
            return Ok(());
        }
        if !record::is_supported(available) {
            return Err(OtaError::UnsupportedVersion);
        }
        if record::failed() == Some(available) {
            log::warn!(
                "OTA: v{}.{}.{} failed to boot before, skipping it until a power cycle",
                resp.major,
                resp.minor,
                resp.patch
            );
            return Ok(());
        }

        log::info!(
            "OTA: v{}.{}.{} available (current v{}.{}.{}), downloading...",
            resp.major,
            resp.minor,
            resp.patch,
            CURRENT_MAJOR,
            CURRENT_MINOR,
            CURRENT_PATCH
        );

        self.download_flash_and_reset(available).await
    }

    /// Streams the firmware binary from the backend into the DFU flash partition,
    /// marks it for swap, and triggers a soft reset. Does not return on success.
    async fn download_flash_and_reset(&self, version: Version) -> Result<(), OtaError> {
        let config = FirmwareUpdaterConfig::from_linkerfile_blocking(&self.flash, &self.flash);
        let mut aligned = AlignedBuffer([0; 1]);
        let mut updater = BlockingFirmwareUpdater::new(config, &mut aligned.0);

        let mut buf: AlignedBuffer<4096> = AlignedBuffer([0; 4096]);
        let mut fill = 0usize;
        let mut flash_offset = 0usize;

        self.http_client
            .stream_response(FIRMWARE_BINARY_ROUTE, |chunk| {
                let mut pos = 0;
                while pos < chunk.len() {
                    let to_copy = (chunk.len() - pos).min(buf.0.len() - fill);
                    buf.0[fill..fill + to_copy].copy_from_slice(&chunk[pos..pos + to_copy]);
                    fill += to_copy;
                    pos += to_copy;

                    if fill == buf.0.len() {
                        updater
                            .write_firmware(flash_offset, &buf.0)
                            .map_err(|_| NetworkError::FlashError)?;
                        flash_offset += fill;
                        fill = 0;
                        log::debug!("OTA: {flash_offset} bytes written");
                    }
                }
                Ok(())
            })
            .await
            .map_err(|e| match e {
                NetworkError::FlashError => OtaError::Flash,
                _ => OtaError::Network,
            })?;

        // Pad and flush the final partial chunk.
        // Erased flash reads as 0xFF, so padding with 0xFF is safe.
        if fill > 0 {
            let padded = (fill + WRITE_SIZE - 1) & !(WRITE_SIZE - 1);
            buf.0[fill..padded].fill(0xFF);
            updater
                .write_firmware(flash_offset, &buf.0[..padded])
                .map_err(|_| OtaError::Flash)?;
            flash_offset += padded;
        }

        // Before the swap, so a revert knows which version failed
        record::set_pending(version)?;

        log::info!("OTA: {flash_offset} total bytes written, marking DFU for swap");
        if updater.mark_updated().is_err() {
            // No swap is scheduled, so don't leave the version recorded as being installed
            record::clear_pending();
            return Err(OtaError::Flash);
        }

        log::info!("OTA: Resetting...");
        Timer::after_secs(1).await;
        crash::reset();
    }
}

#[embassy_executor::task]
pub async fn ota_task(mut driver: OtaDriver, confirm_boot: bool) -> ! {
    if confirm_boot {
        driver.confirm_boot().await;
    }

    loop {
        log::info!("OTA: Checking for firmware update");
        driver.download_pin.set_high();
        match driver.run_update().await {
            Ok(()) => log::info!("OTA: Firmware is up to date"),
            Err(e) => log::error!("OTA: Update failed: {:?}", e),
        }
        driver.download_pin.set_low();
        OTA_TRIGGER.wait().await;
    }
}
