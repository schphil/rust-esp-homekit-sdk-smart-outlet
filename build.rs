use std::{env, path::PathBuf};

// Necessary because of this issue: https://github.com/rust-lang/cargo/issues/9641
fn main() -> anyhow::Result<()> {
    embuild::kconfig::CfgArgs::output_propagated("ESP_HOMEKIT_SDK")?;
    embuild::build::LinkArgs::output_propagated("ESP_HOMEKIT_SDK")?;

    let mcu = std::env::var("DEP_ESP_IDF_MCU").map_err(|s| anyhow::anyhow!(s))?;

    if mcu == "esp32s2" {
        // Future; might be possible once https://github.com/rust-lang/cargo/issues/9096 hits Cargo nightly:
        //let ulp_elf = PathBuf::from(env::var_os("CARGO_BIN_FILE_RUST_ESP32_ULP_HELLO_rust_esp32_ulp_hello").unwrap());

        let ulp_elf = PathBuf::from("ulp").join("rust-esp32-ulp-hello");
        embuild::symgen::run(&ulp_elf, 0x50_000_000)?; // This is where the RTC Slow Mem is mapped within the ESP32-S2 memory space
        embuild::bingen::run(&ulp_elf)?;

        embuild::cargo::track_file(ulp_elf);
    }
    
    ////println!("cargo:rustc-link-arg=/Users/philippscheff/privat/code/rust-esp32-std-homekit-smart-outlet/target/xtensa-esp32-espidf/debug/build/esp-homekit-sdk-sys-17c62df7f0a677c9/out/esp-homekit-sdk/.pio/build/debug/libf3d/libesp-homekit-sdk.a");
    
    println!("cargo:rustc-cfg={}", mcu);

    Ok(())
}
