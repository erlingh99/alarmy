use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use memory_layout::{Layout, RegionName};

fn main() {
    // Generate `memory.x` from the shared layout into our output directory and ensure it's
    // on the linker search path.
    let out = &PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let layout = Layout::load(Path::new("../memory_layout.toml"));
    fs::write(
        out.join("memory.x"),
        layout.memory_x(RegionName::Bootloader),
    )
    .unwrap();
    println!("cargo:rustc-link-search={}", out.display());

    println!("cargo:rustc-link-arg-bins=--nmagic");
    println!("cargo:rustc-link-arg-bins=-Tlink.x");
    println!("cargo:rustc-link-arg-bins=-Tlink-rp.x");
}
