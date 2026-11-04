//! OTA versions in watchdog scratch 2 (`pending`) and 3 (`failed`), so a reverted version isn't
//! downloaded again. Cleared by power loss.

use embassy_rp::pac;

use super::driver::OtaError;

pub type Version = (u32, u32, u32);

const TAG: u32 = 0xBD;

fn encode(version: Version) -> Option<u32> {
    let (major, minor, patch) = version;
    (major <= 0xFF && minor <= 0xFF && patch <= 0xFF)
        .then_some(TAG << 24 | major << 16 | minor << 8 | patch)
}

fn decode(value: u32) -> Option<Version> {
    (value >> 24 == TAG).then_some(((value >> 16) & 0xFF, (value >> 8) & 0xFF, value & 0xFF))
}

pub fn is_supported(version: Version) -> bool {
    encode(version).is_some()
}

// Scratch 0-1 are the crash handler's (see crash.rs); the bootrom uses 4-7.
pub fn pending() -> Option<Version> {
    decode(pac::WATCHDOG.scratch2().read())
}

/// Fails for a part over 255
pub fn set_pending(version: Version) -> Result<(), OtaError> {
    let value = encode(version).ok_or(OtaError::UnsupportedVersion)?;
    pac::WATCHDOG.scratch2().write_value(value);
    Ok(())
}

pub fn clear_pending() {
    pac::WATCHDOG.scratch2().write_value(0);
}

pub fn failed() -> Option<Version> {
    decode(pac::WATCHDOG.scratch3().read())
}

/// `version` came from `pending`, so it fits
pub fn set_failed(version: Version) {
    pac::WATCHDOG
        .scratch3()
        .write_value(encode(version).unwrap_or(0));
}
