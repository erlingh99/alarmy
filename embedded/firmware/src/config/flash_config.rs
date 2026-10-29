pub const FW_ADDR: *const u8 = 0x10007000 as *const u8;
pub const CLM_ADDR: *const u8 = 0x10047000 as *const u8;
pub const NVRAM_ADDR: *const u8 = 0x10048000 as *const u8;

// Sizes of the cyw43-firmware crate's actual blobs:
pub const FW_SIZE: usize = include_bytes!("../../../cyw43-firmware/43439A0.bin").len();
pub const CLM_SIZE: usize = include_bytes!("../../../cyw43-firmware/43439A0_clm.bin").len();
pub const NVRAM_SIZE: usize = include_bytes!("../../../cyw43-firmware/nvram_rp2040.bin").len();
