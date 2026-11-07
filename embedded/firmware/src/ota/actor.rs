use core::cell::RefCell;

use embassy_boot::FirmwareUpdaterError;
use embassy_boot_rp::{AlignedBuffer, BlockingFirmwareUpdater, FirmwareUpdaterConfig, State};
use embassy_executor::Spawner;
use embassy_rp::Peri;
use embassy_rp::flash::{Blocking, Flash};
use embassy_rp::gpio::{AnyPin, Level, Output};
use embassy_rp::peripherals::FLASH;
use embassy_sync::blocking_mutex::Mutex;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_time::{Duration, Instant, Timer};
use heapless::String;
use serde::Deserialize;

use crate::config::api_config::FIRMWARE_BINARY_ROUTE;
use crate::health::{self, Fault};
use crate::mailbox::{Mailbox, MailboxReceiver, MailboxSender};
use crate::net::{HttpClient, NetworkError};
use crate::supervisor::{crash, watchdog};

use super::record::{self, Version};

const FLASH_SIZE: usize = 2 * 1024 * 1024;
const BOOT_CONFIRM_DEADLINE: Duration = Duration::from_secs(10 * 60);

// RP2040 flash write alignment, pad partial final chunks to this boundary.
const WRITE_SIZE: usize = 256;

const fn parse_u32(s: &str) -> u32 {
    match u32::from_str_radix(s, 10) {
        Ok(v) => v,
        Err(_) => panic!("invalid version component"),
    }
}

const CURRENT_MAJOR: u32 = parse_u32(env!("CARGO_PKG_VERSION_MAJOR"));
const CURRENT_MINOR: u32 = parse_u32(env!("CARGO_PKG_VERSION_MINOR"));
const CURRENT_PATCH: u32 = parse_u32(env!("CARGO_PKG_VERSION_PATCH"));

/// Updates must be signed with its private key, see scripts/signing_key.py
const SIGNING_PUBLIC_KEY: &[u8; 32] = include_bytes!("../../signing_key.pub");

#[derive(Debug)]
pub enum OtaError {
    #[allow(dead_code)] // Read through Debug when reported
    Network(NetworkError),
    Flash,
    /// A version part over 255, which the 8 bit OTA record can't hold
    UnsupportedVersion,
    /// The backend has no signature for the image, or it is malformed
    Unsigned,
    /// The downloaded image doesn't match its signature
    BadSignature,
}

#[derive(Deserialize)]
pub struct VersionResponse {
    major: u32,
    minor: u32,
    patch: u32,
}

#[derive(Deserialize)]
pub struct SignatureResponse {
    /// Ed25519 signature of the image's SHA-512 digest, in hex
    signature: String<128>,
}

impl SignatureResponse {
    fn bytes(&self) -> Option<[u8; 64]> {
        let hex = self.signature.as_bytes();
        let mut signature = [0; 64];
        if hex.len() != 2 * signature.len() {
            return None;
        }
        for (byte, pair) in signature.iter_mut().zip(hex.chunks(2)) {
            *byte = u8::from_str_radix(core::str::from_utf8(pair).ok()?, 16).ok()?;
        }
        Some(signature)
    }
}

pub(super) type FlashMutex =
    Mutex<NoopRawMutex, RefCell<Flash<'static, FLASH, Blocking, FLASH_SIZE>>>;

pub struct OtaPinning {
    pub flash: Peri<'static, FLASH>,
    pub download_pin: Peri<'static, AnyPin>,
}

pub struct OtaActor {
    flash: FlashMutex,
    download_pin: Output<'static>,
    needs_confirmation: bool,
    mailbox: MailboxReceiver<OtaMsg>,
}

static MAILBOX: Mailbox<OtaMsg> = Mailbox::new();

#[derive(Debug)]
enum OtaMsg {
    CheckForUpdate,
}

#[derive(Clone, Copy)]
pub struct OtaHandle {
    mailbox: MailboxSender<OtaMsg>,
}

pub fn handle() -> OtaHandle {
    OtaHandle {
        mailbox: MAILBOX.sender(),
    }
}

impl OtaHandle {
    pub fn check_for_update(&self) {
        self.mailbox.post_or_warn(OtaMsg::CheckForUpdate);
    }
}

impl OtaActor {
    /// Check the bootloader state at boot. A boot after a swap, arms the watchdog's confirmation deadline
    pub fn check_boot(pinning: OtaPinning) -> Self {
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

        Self {
            flash,
            download_pin,
            needs_confirmation,
            mailbox: MAILBOX.receiver(),
        }
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
    async fn confirm_boot(&self, http_client: &HttpClient) {
        log::info!("OTA: Waiting for the backend before confirming the firmware");
        http_client.wait_for_backend().await;
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

    async fn run_update(&self, http_client: &HttpClient) -> Result<(), OtaError> {
        let resp = http_client
            .get_firmware_version()
            .await
            .map_err(OtaError::Network)?;

        let available = (resp.major, resp.minor, resp.patch);
        if available <= (CURRENT_MAJOR, CURRENT_MINOR, CURRENT_PATCH) {
            return Ok(());
        }
        if !record::is_supported(available) {
            return Err(OtaError::UnsupportedVersion);
        }
        if record::failed() == Some(available) {
            log::warn!(
                "OTA: v{}.{}.{} failed before, skipping it until a power cycle",
                resp.major,
                resp.minor,
                resp.patch
            );
            return Ok(());
        }
        let signature = http_client
            .get_firmware_signature()
            .await
            .map_err(|e| match e {
                NetworkError::HttpStatus(404) => OtaError::Unsigned,
                e => OtaError::Network(e),
            })?
            .bytes()
            .ok_or(OtaError::Unsigned)?;

        log::info!(
            "OTA: v{}.{}.{} available (current v{}.{}.{}), downloading...",
            resp.major,
            resp.minor,
            resp.patch,
            CURRENT_MAJOR,
            CURRENT_MINOR,
            CURRENT_PATCH
        );

        self.download_flash_and_reset(http_client, available, &signature)
            .await
    }

    /// Streams the firmware binary from the backend into the DFU flash partition, marks it for
    /// swap if it matches `signature`, and triggers a soft reset. Does not return on success.
    async fn download_flash_and_reset(
        &self,
        http_client: &HttpClient,
        version: Version,
        signature: &[u8; 64],
    ) -> Result<(), OtaError> {
        let config = FirmwareUpdaterConfig::from_linkerfile_blocking(&self.flash, &self.flash);
        let mut aligned = AlignedBuffer([0; 1]);
        let mut updater = BlockingFirmwareUpdater::new(config, &mut aligned.0);

        let mut buf: AlignedBuffer<4096> = AlignedBuffer([0; 4096]);
        let mut fill = 0usize;
        let mut flash_offset = 0usize;

        let image_len = http_client
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
                e => OtaError::Network(e),
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

        log::info!("OTA: {flash_offset} total bytes written, verifying signature");
        // Over the image as served, not the padding
        match updater.verify_and_mark_updated(SIGNING_PUBLIC_KEY, signature, image_len as u32) {
            Ok(()) => log::info!("OTA: Signature valid, DFU marked for swap"),
            Err(FirmwareUpdaterError::Signature(_)) => {
                // So the same image isn't downloaded again on every check
                Self::record_failed_update();
                return Err(OtaError::BadSignature);
            }
            Err(_) => {
                // No swap is scheduled, so don't leave the version recorded as being installed
                record::clear_pending();
                return Err(OtaError::Flash);
            }
        }

        log::info!("OTA: Resetting...");
        Timer::after_secs(1).await;
        crash::reset();
    }

    async fn run(&mut self, http_client: &HttpClient) -> ! {
        if self.needs_confirmation {
            self.confirm_boot(http_client).await;
        }

        loop {
            log::info!("OTA: Checking for firmware update");
            self.download_pin.set_high();
            match self.run_update(http_client).await {
                Ok(()) => {
                    log::info!("OTA: Firmware is up to date");
                    health::clear(Fault::Ota);
                }
                Err(e) => health::report(Fault::Ota, e),
            }
            self.download_pin.set_low();
            // Requests that arrived during the check were answered by it
            self.mailbox.clear();
            match self.mailbox.receive().await {
                OtaMsg::CheckForUpdate => {}
            }
        }
    }
}

pub fn spawn(spawner: Spawner, actor: OtaActor, http_client: &'static HttpClient) {
    spawner.spawn(ota_task(actor, http_client).unwrap());
}

#[embassy_executor::task]
async fn ota_task(mut actor: OtaActor, http_client: &'static HttpClient) -> ! {
    actor.run(http_client).await
}
