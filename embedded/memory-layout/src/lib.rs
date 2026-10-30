//! Build dependency of the bootloader and firmware: turns `memory_layout.toml` into the
//! `memory.x` linker script, so the flash layout is defined in one place.

use std::collections::BTreeMap;
use std::fmt::Write;
use std::fs;
use std::path::Path;

/// Flash page size; embassy-boot needs one extra page in DFU for the swap
const PAGE_SIZE: usize = 4096;

pub struct Region {
    pub origin: usize,
    pub length: usize,
}

/// The `memory_layout.toml` regions the build scripts refer to by name
#[derive(Clone, Copy)]
pub enum RegionName {
    Boot2,
    Bootloader,
    BootloaderState,
    Active,
    Dfu,
}

impl RegionName {
    const ALL: [Self; 5] = [
        Self::Boot2,
        Self::Bootloader,
        Self::BootloaderState,
        Self::Active,
        Self::Dfu,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Boot2 => "BOOT2",
            Self::Bootloader => "BOOTLOADER",
            Self::BootloaderState => "BOOTLOADER_STATE",
            Self::Active => "ACTIVE",
            Self::Dfu => "DFU",
        }
    }
}

/// The `[wifi_creds]` constants of the WIFI_CREDS sector format
pub struct WifiCreds {
    pub magic: String,
    pub max_ssid_len: usize,
    pub psk_len: usize,
}

pub struct Layout {
    /// Every non-empty region in the file, by name
    pub regions: BTreeMap<String, Region>,
    pub wifi_creds: WifiCreds,
}

impl Layout {
    /// Load the layout, failing the build if a region named by `RegionName` is missing or the
    /// regions overlap, or if DFU isn't ACTIVE plus one page (embassy-boot's swap scratch).
    /// Zero-length marker regions are dropped.
    pub fn load(path: &Path) -> Self {
        println!("cargo:rerun-if-changed={}", path.display());
        let file: toml::Table = fs::read_to_string(path).unwrap().parse().unwrap();
        let regions = file["regions"]
            .as_table()
            .unwrap()
            .iter()
            .map(|(name, region)| {
                let field = |key| {
                    region
                        .get(key)
                        .and_then(|value| value.as_integer())
                        .unwrap_or_else(|| {
                            panic!("memory_layout.toml: {name} has no integer {key}")
                        }) as usize
                };
                let region = Region {
                    origin: field("origin"),
                    length: field("length"),
                };
                (name.clone(), region)
            })
            .filter(|(_, region)| region.length > 0)
            .collect();

        let creds = &file["wifi_creds"];
        let creds_field = |key: &str| {
            creds[key]
                .as_integer()
                .unwrap_or_else(|| panic!("memory_layout.toml: wifi_creds has no integer {key}"))
                as usize
        };
        let wifi_creds = WifiCreds {
            magic: creds["magic"]
                .as_str()
                .expect("memory_layout.toml: wifi_creds has no string magic")
                .to_owned(),
            max_ssid_len: creds_field("max_ssid_len"),
            psk_len: creds_field("psk_len"),
        };

        let layout = Self {
            regions,
            wifi_creds,
        };
        for name in RegionName::ALL {
            layout.region(name);
        }
        layout.validate();
        layout
    }

    fn validate(&self) {
        let mut sorted: Vec<_> = self.regions.iter().collect();
        sorted.sort_by_key(|(_, region)| region.origin);
        for pair in sorted.windows(2) {
            let ((name, region), (next_name, next)) = (pair[0], pair[1]);
            assert!(
                region.origin + region.length <= next.origin,
                "memory_layout.toml: {name} overlaps {next_name}"
            );
        }

        let active = self.region(RegionName::Active).length;
        let dfu = self.region(RegionName::Dfu).length;
        assert!(
            dfu == active + PAGE_SIZE,
            "memory_layout.toml: DFU must be ACTIVE + {PAGE_SIZE:#X} bytes, got {dfu:#X} for ACTIVE {active:#X}"
        );
    }

    pub fn region(&self, name: RegionName) -> &Region {
        self.regions
            .get(name.as_str())
            .unwrap_or_else(|| panic!("memory_layout.toml has no {} region", name.as_str()))
    }

    /// `memory.x` with every region, `flash` aliased as cortex-m-rt's `FLASH`, plus the
    /// `__bootloader_*` partition offsets embassy-boot reads
    pub fn memory_x(&self, flash: RegionName) -> String {
        let mut sorted: Vec<_> = self.regions.iter().collect();
        sorted.sort_by_key(|(_, region)| region.origin);

        let mut out = String::from("MEMORY\n{\n");
        for (name, region) in sorted {
            writeln!(
                out,
                "  {name:<18} : ORIGIN = {:#010X}, LENGTH = {:#X}",
                region.origin, region.length
            )
            .unwrap();
        }
        out.push_str("}\n\n");
        writeln!(out, "REGION_ALIAS(\"FLASH\", {});\n", flash.as_str()).unwrap();

        let base = self.region(RegionName::Boot2).origin;
        for (symbol, name) in [
            ("state", RegionName::BootloaderState),
            ("active", RegionName::Active),
            ("dfu", RegionName::Dfu),
        ] {
            let region = self.region(name);
            writeln!(
                out,
                "__bootloader_{symbol}_start = {:#X};",
                region.origin - base
            )
            .unwrap();
            writeln!(
                out,
                "__bootloader_{symbol}_end = {:#X};",
                region.origin + region.length - base
            )
            .unwrap();
        }
        out
    }
}
