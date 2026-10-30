//! Persistent OTA bookkeeping in the OTA_RECORD flash sector, so a version that failed to boot
//! isn't downloaded again after the bootloader reverts to the previous image.
//!
//! The sector holds the version being installed (`pending`, written just before the swap) and the
//! last version that was reverted (`failed`). Layout: magic `OTAR`, then the two slots, each a
//! u32 presence flag (1 = set) followed by major, minor, patch as little-endian u32s.

use super::driver::{FlashMutex, OtaError};
use crate::config::flash_config::{ota_record_offset, OTA_RECORD_LENGTH};

pub type Version = (u32, u32, u32);

const MAGIC: &[u8; 4] = b"OTAR";
const SLOT_LEN: usize = 16;
const RECORD_LEN: usize = MAGIC.len() + 2 * SLOT_LEN;
// RP2040 flash write alignment
const WRITE_SIZE: usize = 256;

#[derive(Default, PartialEq, Debug)]
pub struct OtaRecord {
    /// The version whose swap has been requested; cleared once it confirms its boot
    pub pending: Option<Version>,
    /// The version the bootloader last reverted
    pub failed: Option<Version>,
}

fn encode_slot(slot: Option<Version>) -> [u8; SLOT_LEN] {
    let mut out = [0xFF; SLOT_LEN];
    if let Some((major, minor, patch)) = slot {
        out[0..4].copy_from_slice(&1u32.to_le_bytes());
        out[4..8].copy_from_slice(&major.to_le_bytes());
        out[8..12].copy_from_slice(&minor.to_le_bytes());
        out[12..16].copy_from_slice(&patch.to_le_bytes());
    }
    out
}

fn decode_slot(raw: &[u8]) -> Option<Version> {
    let word = |i: usize| u32::from_le_bytes(raw[i * 4..i * 4 + 4].try_into().unwrap());
    (word(0) == 1).then(|| (word(1), word(2), word(3)))
}

impl OtaRecord {
    /// An erased, unreadable or unrecognised sector reads as an empty record
    pub fn load(flash: &FlashMutex) -> Self {
        let offset = ota_record_offset();
        let mut raw = [0u8; RECORD_LEN];
        let read = flash.lock(|f| f.borrow_mut().blocking_read(offset, &mut raw));
        if read.is_err() || &raw[..MAGIC.len()] != MAGIC {
            return Self::default();
        }
        let slots = &raw[MAGIC.len()..];
        Self {
            pending: decode_slot(&slots[..SLOT_LEN]),
            failed: decode_slot(&slots[SLOT_LEN..]),
        }
    }

    pub fn store(&self, flash: &FlashMutex) -> Result<(), OtaError> {
        let offset = ota_record_offset();
        let mut page = [0xFF; WRITE_SIZE];
        page[..MAGIC.len()].copy_from_slice(MAGIC);
        page[MAGIC.len()..MAGIC.len() + SLOT_LEN].copy_from_slice(&encode_slot(self.pending));
        page[MAGIC.len() + SLOT_LEN..RECORD_LEN].copy_from_slice(&encode_slot(self.failed));

        flash
            .lock(|f| {
                let mut f = f.borrow_mut();
                f.blocking_erase(offset, offset + OTA_RECORD_LENGTH as u32)?;
                f.blocking_write(offset, &page)
            })
            .map_err(|_| OtaError::Flash)
    }
}
