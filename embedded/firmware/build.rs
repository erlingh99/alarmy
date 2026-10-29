use dotenvy::dotenv;
use std::env;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;

fn main() {
    dotenv().ok(); // load .env if present; CI sets vars directly in the environment
    let ssid = env::var("WIFI_SSID").expect("WIFI_SSID must be set (in .env or environment)");
    let password =
        env::var("WIFI_PASSWORD").expect("WIFI_PASSWORD must be set (in .env or environment)");
    println!("cargo::rustc-env=WIFI_SSID={ssid}");
    println!("cargo::rustc-env=WIFI_PASSWORD={password}");

    // Put `memory.x` in our output directory and ensure it's
    // on the linker search path.
    let out = &PathBuf::from(env::var_os("OUT_DIR").unwrap());
    File::create(out.join("memory.x"))
        .unwrap()
        .write_all(include_bytes!("memory.x"))
        .unwrap();
    println!("cargo:rustc-link-search={}", out.display());

    println!("cargo:rerun-if-changed=memory.x");

    println!("cargo:rustc-link-arg-bins=--nmagic");
    println!("cargo:rustc-link-arg-bins=-Tlink.x");
}
