use std::env;
use std::fmt::Write;
use std::fs;
use std::path::{Path, PathBuf};

use memory_layout::{Layout, RegionName};

fn main() {
    // Generate `memory.x` from the shared layout into our output directory and ensure it's
    // on the linker search path.
    let out = &PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let layout = Layout::load(Path::new("../memory_layout.toml"));
    fs::write(out.join("memory.x"), layout.memory_x(RegionName::Active)).unwrap();
    println!("cargo:rustc-link-search={}", out.display());

    // Expose the regions as `<NAME>_ORIGIN`/`<NAME>_LENGTH` consts (see config/flash_config.rs)
    let mut consts = String::new();
    for (name, region) in &layout.regions {
        writeln!(
            consts,
            "pub const {name}_ORIGIN: usize = {:#X};",
            region.origin
        )
        .unwrap();
        writeln!(
            consts,
            "pub const {name}_LENGTH: usize = {:#X};",
            region.length
        )
        .unwrap();
    }
    let creds = &layout.wifi_creds;
    writeln!(
        consts,
        "pub const WIFI_CREDS_MAGIC: &[u8; {}] = b{:?};",
        creds.magic.len(),
        creds.magic
    )
    .unwrap();
    writeln!(
        consts,
        "pub const WIFI_CREDS_MAX_SSID_LEN: usize = {};",
        creds.max_ssid_len
    )
    .unwrap();
    writeln!(
        consts,
        "pub const WIFI_CREDS_PSK_LEN: usize = {};",
        creds.psk_len
    )
    .unwrap();
    fs::write(out.join("memory_layout.rs"), consts).unwrap();

    println!("cargo:rustc-link-arg-bins=--nmagic");
    println!("cargo:rustc-link-arg-bins=-Tlink.x");
}
