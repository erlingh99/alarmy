use core::cell::RefCell;

use embassy_boot_rp::{AlignedBuffer, BlockingFirmwareUpdater, FirmwareUpdaterConfig};
use embassy_executor::Spawner;
use embassy_rp::flash::{Blocking, Flash};
use embassy_rp::gpio::{AnyPin, Level, Output};
use embassy_rp::peripherals::{FLASH, WATCHDOG};
use embassy_rp::watchdog::Watchdog;
use embassy_rp::Peri;
use embassy_sync::blocking_mutex::raw::{CriticalSectionRawMutex, NoopRawMutex};
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Timer};
use serde::Deserialize;

use crate::config::api_config::FIRMWARE_BINARY_ROUTE;
use crate::net::{HttpClientManager, NetworkError};

const FLASH_SIZE: usize = 2 * 1024 * 1024;
const WATCHDOG_TIMEOUT: Duration = Duration::from_secs(8);

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

type FlashMutex = Mutex<NoopRawMutex, RefCell<Flash<'static, FLASH, Blocking, FLASH_SIZE>>>;

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

        // Confirm this boot so the bootloader won't revert on watchdog reset.
        // Silently ignored on a fresh device with an uninitialized state partition.
        let config = FirmwareUpdaterConfig::from_linkerfile_blocking(&flash, &flash);
        let mut aligned = AlignedBuffer([0u8; 1]);
        BlockingFirmwareUpdater::new(config, &mut aligned.0)
            .mark_booted()
            .ok();

        watchdog.stop();

        let driver = OtaDriver {
            http_client,
            flash,
            download_pin,
        };
        spawner.spawn(ota_task(driver).unwrap());
    }

    pub async fn run_update(&self) -> Result<(), OtaError> {
        let resp = self
            .http_client
            .get_firmware_version()
            .await
            .map_err(|_| OtaError::Network)?;

        if (resp.major, resp.minor, resp.patch) <= (CURRENT_MAJOR, CURRENT_MINOR, CURRENT_PATCH) {
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

        self.download_flash_and_reset().await
    }

    /// Streams the firmware binary from the backend into the DFU flash partition,
    /// marks it for swap, and triggers a soft reset. Does not return on success.
    async fn download_flash_and_reset(&self) -> Result<(), OtaError> {
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

        log::info!("OTA: {flash_offset} total bytes written, marking DFU for swap");
        updater.mark_updated().map_err(|_| OtaError::Flash)?;

        log::info!("OTA: Resetting...");
        Timer::after_secs(1).await;
        cortex_m::peripheral::SCB::sys_reset();
    }
}

#[embassy_executor::task]
pub async fn ota_task(mut driver: OtaDriver) -> ! {
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
