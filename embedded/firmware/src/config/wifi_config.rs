//! WiFi credentials are not compiled into the firmware (the binary is served publicly for OTA).
//! They are preflashed per device into the WIFI_CREDS sector by `embedded/scripts/flash_blobs.py`.
//!
//! Only the WPA2 PSK (PBKDF2 of the passphrase) is stored, never the passphrase itself.
//!
//! Sector format: magic `AWC2`, ssid_len: u8, psk_len: u8 (0 or 32), ssid bytes, psk bytes.

use heapless::{String, Vec};

use super::flash_config::{
    WIFI_CREDS_ADDR, WIFI_CREDS_MAGIC as MAGIC, WIFI_CREDS_MAX_SSID_LEN as MAX_SSID_LEN,
    WIFI_CREDS_PSK_LEN as PSK_LEN, WIFI_CREDS_SIZE,
};

const HEADER_LEN: usize = MAGIC.len() + 2;

#[derive(Debug)]
pub enum CredentialsError {
    /// Sector is erased or holds something else
    Missing,
    /// Magic matches but lengths or contents are invalid
    Invalid,
}

pub struct WifiCredentials {
    pub ssid: String<MAX_SSID_LEN>,
    /// Prehashed WPA2 PSK, empty for an open network
    pub psk: Vec<u8, PSK_LEN>,
}

impl WifiCredentials {
    /// Copy the preflashed credentials out of flash into RAM
    pub fn load() -> Result<Self, CredentialsError> {
        let raw = unsafe { core::slice::from_raw_parts(WIFI_CREDS_ADDR, WIFI_CREDS_SIZE) };
        Self::parse(raw)
    }

    fn parse(raw: &[u8]) -> Result<Self, CredentialsError> {
        if &raw[..MAGIC.len()] != MAGIC {
            return Err(CredentialsError::Missing);
        }
        let ssid_len = raw[MAGIC.len()] as usize;
        let psk_len = raw[MAGIC.len() + 1] as usize;
        if ssid_len == 0 || ssid_len > MAX_SSID_LEN || (psk_len != 0 && psk_len != PSK_LEN) {
            return Err(CredentialsError::Invalid);
        }

        let ssid = &raw[HEADER_LEN..HEADER_LEN + ssid_len];
        let psk = &raw[HEADER_LEN + ssid_len..HEADER_LEN + ssid_len + psk_len];
        let ssid = core::str::from_utf8(ssid).map_err(|_| CredentialsError::Invalid)?;

        Ok(Self {
            ssid: String::try_from(ssid).map_err(|_| CredentialsError::Invalid)?,
            psk: Vec::from_slice(psk).map_err(|_| CredentialsError::Invalid)?,
        })
    }
}
