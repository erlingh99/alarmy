use core::cell::RefCell;

use embassy_boot_rp::{AlignedBuffer, BlockingFirmwareUpdater, FirmwareUpdaterConfig, State};
use embassy_executor::Spawner;
use embassy_futures::select::{select, Either};
use embassy_rp::flash::{Blocking, Flash};
use embassy_rp::gpio::{AnyPin, Level, Output};
use embassy_rp::peripherals::{FLASH, WATCHDOG};
use embassy_rp::watchdog::Watchdog;
use embassy_rp::Peri;
use embassy_sync::blocking_mutex::raw::{CriticalSectionRawMutex, NoopRawMutex};
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Instant, Timer};
use serde::Deserialize;

use crate::config::api_config::{FIRMWARE_BINARY_ROUTE, RETRY_DELAY_S};
use crate::net::{HttpClientManager, NetworkError};

use super::record::{OtaRecord, Version};

const FLASH_SIZE: usize = 2 * 1024 * 1024;
const WATCHDOG_TIMEOUT: Duration = Duration::from_secs(8);
const WATCHDOG_FEED_INTERVAL: Duration = Duration::from_secs(2);
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
    pub watchdog: Peri<'static, WATCHDOG>,
    pub download_pin: Peri<'static, AnyPin>,
}

pub struct OtaDriver {
    http_client: &'static HttpClientManager,
    flash: FlashMutex,
    download_pin: Output<'static>,
}

pub static OTA_TRIGGER: Signal<CriticalSectionRawMutex, ()> = Signal::new();
static STOP_WATCHDOG: Signal<CriticalSectionRawMutex, ()> = Signal::new();

impl OtaDriver {
    pub fn initialize(
        spawner: Spawner,
        http_client: &'static HttpClientManager,
        pinning: OtaPinning,
    ) {
        //override bootloader watchdog
        let mut watchdog = Watchdog::new(pinning.watchdog);
        watchdog.start(WATCHDOG_TIMEOUT);

        let download_pin = Output::new(pinning.download_pin, Level::Low);

        let flash = Flash::<_, _, FLASH_SIZE>::new_blocking(pinning.flash);
        let flash = Mutex::new(RefCell::new(flash));

        let driver = OtaDriver {
            http_client,
            flash,
            download_pin,
        };

        // Fresh off an OTA swap the new image must prove it can still reach the backend (and so
        // receive the next update) before it is confirmed. Until then the watchdog keeps running,
        // so a hang or a missed deadline resets into the bootloader, which reverts the swap.
        let state = driver.updater_state();
        // The bootloader reverted the last swap: remember that version so it isn't downloaded again
        if matches!(state, Ok(State::Revert)) {
            driver.record_failed_update();
        }
        // An unreadable state is treated like a swap: better to demand proof than to confirm blindly.
        let swapped = match state {
            Ok(state) => matches!(state, State::Swap),
            Err(e) => {
                log::error!(
                    "OTA: Could not read bootloader state ({:?}), requiring confirmation",
                    e
                );
                true
            }
        };
        if swapped {
            log::info!(
                "OTA: Running freshly swapped firmware, confirming once the backend is reachable"
            );
            spawner.spawn(watchdog_task(watchdog).unwrap());
        } else {
            // Silently ignored on a fresh device with an uninitialized state partition.
            driver.mark_booted().ok();
            watchdog.stop();
        }

        spawner.spawn(ota_task(driver, swapped).unwrap());
    }

    fn updater_state(&self) -> Result<State, OtaError> {
        let config = FirmwareUpdaterConfig::from_linkerfile_blocking(&self.flash, &self.flash);
        let mut aligned = AlignedBuffer([0u8; 1]);
        BlockingFirmwareUpdater::new(config, &mut aligned.0)
            .get_state()
            .map_err(|_| OtaError::Flash)
    }

    /// Confirm this boot so the bootloader won't revert on watchdog reset
    fn mark_booted(&self) -> Result<(), OtaError> {
        let config = FirmwareUpdaterConfig::from_linkerfile_blocking(&self.flash, &self.flash);
        let mut aligned = AlignedBuffer([0u8; 1]);
        BlockingFirmwareUpdater::new(config, &mut aligned.0)
            .mark_booted()
            .map_err(|_| OtaError::Flash)
    }

    /// Move the version that was being installed to `failed`, after the bootloader reverted it
    fn record_failed_update(&self) {
        let mut record = OtaRecord::load(&self.flash);
        let Some(version) = record.pending.take() else {
            log::warn!("OTA: Update was reverted, but no attempted version was recorded");
            return;
        };
        log::error!(
            "OTA: v{}.{}.{} was reverted, it will not be installed again",
            version.0,
            version.1,
            version.2
        );
        record.failed = Some(version);
        if let Err(e) = record.store(&self.flash) {
            log::error!("OTA: Failed to record the reverted version: {:?}", e);
        }
    }

    /// The swapped-in firmware confirmed itself, so its version is no longer in doubt
    fn clear_pending_update(&self) {
        let mut record = OtaRecord::load(&self.flash);
        if record.pending.take().is_some() {
            if let Err(e) = record.store(&self.flash) {
                log::error!("OTA: Failed to clear the pending version: {:?}", e);
            }
        }
    }

    /// Wait until the backend answers, then confirm the boot and release the watchdog.
    /// Retrying is unbounded because the watchdog deadline ends it
    async fn confirm_boot(&self) {
        while let Err(e) = self.http_client.get_health().await {
            log::warn!(
                "OTA: Backend unreachable, firmware stays unconfirmed ({:?})",
                e
            );
            Timer::after_secs(RETRY_DELAY_S).await;
        }
        match self.mark_booted() {
            Ok(()) => {
                log::info!("OTA: Backend reachable, firmware confirmed");
                self.clear_pending_update();
                STOP_WATCHDOG.signal(());
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

    pub async fn run_update(&self) -> Result<(), OtaError> {
        let resp = self
            .http_client
            .get_firmware_version()
            .await
            .map_err(|_| OtaError::Network)?;

        let available = (resp.major, resp.minor, resp.patch);
        if available <= (CURRENT_MAJOR, CURRENT_MINOR, CURRENT_PATCH) {
            return Ok(());
        }
        if OtaRecord::load(&self.flash).failed == Some(available) {
            log::warn!(
                "OTA: v{}.{}.{} failed to boot before, skipping it",
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

        // Recorded before the swap so the old image can tell what was attempted if it gets
        // reverted back. If this fails, don't swap: a failure then couldn't be remembered.
        let record = OtaRecord {
            pending: Some(version),
            ..OtaRecord::load(&self.flash)
        };
        record.store(&self.flash)?;

        log::info!("OTA: {flash_offset} total bytes written, marking DFU for swap");
        if updater.mark_updated().is_err() {
            // No swap is scheduled, so don't leave the version recorded as being installed
            self.clear_pending_update();
            return Err(OtaError::Flash);
        }

        log::info!("OTA: Resetting...");
        Timer::after_secs(1).await;
        cortex_m::peripheral::SCB::sys_reset();
    }
}

#[embassy_executor::task]
async fn watchdog_task(mut watchdog: Watchdog) {
    let deadline = Instant::now() + BOOT_CONFIRM_DEADLINE;
    loop {
        match select(STOP_WATCHDOG.wait(), Timer::after(WATCHDOG_FEED_INTERVAL)).await {
            Either::First(()) => {
                watchdog.stop();
                return;
            }
            Either::Second(()) if Instant::now() < deadline => watchdog.feed(WATCHDOG_TIMEOUT),
            Either::Second(()) => {
                log::error!("OTA: Firmware not confirmed in time, letting the watchdog revert it");
                return;
            }
        }
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
