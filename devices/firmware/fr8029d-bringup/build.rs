//! Select the FR8029D memory layout and build the opt-in `FreeRTOS` kernel.
#[cfg(all(feature = "mic1-acquisition", feature = "ble-gatt-ota"))]
compile_error!("Select either mic1-acquisition or ble-gatt-ota");

use std::{
    env,
    error::Error,
    path::{Path, PathBuf},
    process::Command,
};

const FREERTOS_REVISION: &str = "0adc196d4bd52a2d91102b525b0aafc1e14a2386";

fn main() -> Result<(), Box<dyn Error>> {
    let startup_probe = env::var_os("CARGO_FEATURE_PROBE").is_some();
    let advertising_probe = env::var_os("CARGO_FEATURE_BLE_ADV_PROBE").is_some();
    if !startup_probe && !advertising_probe {
        return Ok(());
    }
    if env::var("TARGET")? != "thumbv8m.main-none-eabi" {
        return Err("the probe requires --target thumbv8m.main-none-eabi".into());
    }
    validate_touch_profile()?;

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR")?);
    #[cfg(feature = "ble-gatt-ota")]
    pin_ota_image()?;
    let script = manifest_dir.join("link.x");
    println!("cargo:rerun-if-changed=link.x");
    if startup_probe {
        link_probe(
            "fr8029d-bringup",
            &script,
            env::var_os("CARGO_FEATURE_RAM_PROBE").is_some(),
        );
    }
    if advertising_probe {
        let connect_probe = env::var_os("CARGO_FEATURE_BLE_CONNECT_PROBE").is_some();
        let gatt_board = env::var_os("CARGO_FEATURE_BLE_GATT_BOARD").is_some();
        let gatt_xip = env::var_os("CARGO_FEATURE_BLE_GATT_XIP").is_some();
        let advertising_in_sram =
            !gatt_xip && (connect_probe || env::var_os("CARGO_FEATURE_BLE_ADV_COLD_XIP").is_none());
        if gatt_board {
            if env::var_os("CARGO_FEATURE_MIC1_ACQUISITION").is_some() {
                println!("cargo:rustc-link-arg-bin=fr8029d-ble-adv=--defsym=__gatt_mic1=1");
            }
            if env::var_os("CARGO_FEATURE_BLE_GATT_DLE").is_some() {
                println!("cargo:rustc-link-arg-bin=fr8029d-ble-adv=--defsym=__gatt_dle=1");
            }
            println!("cargo:rustc-link-arg-bin=fr8029d-ble-adv=--defsym=__gatt_board_probe=1");
            if env::var_os("CARGO_FEATURE_BLE_GATT_EVENT_DRIVEN").is_some() {
                println!("cargo:rustc-link-arg-bin=fr8029d-ble-adv=--defsym=__gatt_event_wake=1");
            }
        } else if connect_probe {
            println!("cargo:rustc-link-arg-bin=fr8029d-ble-adv=--defsym=__wide_sram_probe=1");
        }
        if env::var_os("CARGO_FEATURE_BLE_GATT_FREERTOS").is_some() {
            build_freertos(&manifest_dir)?;
            println!("cargo:rustc-link-arg-bin=fr8029d-ble-adv=--defsym=__gatt_freertos=1");
        }
        link_probe("fr8029d-ble-adv", &script, advertising_in_sram);
    }
    Ok(())
}

fn validate_touch_profile() -> Result<(), &'static str> {
    if !cfg!(feature = "touchkey-feedback") {
        return Ok(());
    }
    if !cfg!(feature = "ble-gatt-board") {
        return Err("touchkey-feedback requires the ble-gatt-board firmware entry");
    }
    if cfg!(feature = "ble-gatt-ota") {
        return Err("touchkey-feedback and ble-gatt-ota require separate firmware images");
    }
    if cfg!(feature = "mic1-acquisition")
        && !cfg!(all(
            feature = "ble-gatt-freertos",
            feature = "ble-gatt-xip",
            not(feature = "ble-gatt-dle")
        ))
    {
        return Err("touchkey-feedback with MIC1 requires FreeRTOS Flash with short BLE packets");
    }
    if cfg!(all(
        feature = "ble-gatt-freertos",
        feature = "ble-gatt-dle",
        not(feature = "ble-gatt-xip")
    )) {
        return Err("touchkey-feedback with FreeRTOS long packets requires a Flash image");
    }
    Ok(())
}

fn link_probe(name: &str, script: &Path, in_sram: bool) {
    if in_sram {
        println!("cargo:rustc-link-arg-bin={name}=--defsym=__probe_in_sram=1");
    }
    println!("cargo:rustc-link-arg-bin={name}=-T{}", script.display());
    println!("cargo:rustc-link-arg-bin={name}=--nmagic");
}

fn build_freertos(manifest_dir: &Path) -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-env-changed=FREERTOS_KERNEL_DIR");
    println!("cargo:rerun-if-env-changed=FREERTOS_CLANG");

    let config = manifest_dir.join("freertos");
    let runtime = manifest_dir.join("../../runtimes/freertos");
    println!(
        "cargo:rerun-if-changed={}",
        runtime.join("moteframe_freertos.h").display()
    );
    let kernel = freertos_kernel(manifest_dir)?;
    let include = kernel.join("include");
    let port = kernel.join("portable/GCC/ARM_CM33_NTZ/non_secure");
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").ok_or("OUT_DIR is missing")?);
    let clang = env::var_os("FREERTOS_CLANG").unwrap_or_else(|| "clang".into());
    let event_driven = env::var_os("CARGO_FEATURE_BLE_GATT_EVENT_DRIVEN").is_some();
    for input in [&config, &include, &port] {
        println!("cargo:rerun-if-changed={}", input.display());
    }
    for source in [
        kernel.join("tasks.c"),
        kernel.join("list.c"),
        port.join("port.c"),
        port.join("portasm.c"),
        config.join("fr29_freertos.c"),
    ] {
        println!("cargo:rerun-if-changed={}", source.display());
        let object = out_dir
            .join(
                source
                    .file_name()
                    .ok_or("FreeRTOS source name is missing")?,
            )
            .with_extension("o");
        let mut command = Command::new(&clang);
        if cfg!(feature = "ble-gatt-ota") {
            command.arg("-DFR29_OTA_XIP=1");
        }
        if event_driven {
            command.arg("-DFR29_EVENT_DRIVEN=1");
            if env::var_os("CARGO_FEATURE_BLE_GATT_XIP").is_none() {
                command.arg("-DFR29_EVENT_SRAM=1");
                if cfg!(feature = "ble-gatt-dle") {
                    command.arg("-DFR29_DLE_SRAM=1");
                }
            }
        }
        let status = command
            .args([
                "--target=thumbv8m.main-none-eabi",
                "-mcpu=cortex-m33",
                "-mthumb",
                "-mfloat-abi=soft",
                "-std=c11",
                // Fit the event-driven SRAM image with the existing task stacks.
                if event_driven { "-Oz" } else { "-Os" },
                "-ffreestanding",
                "-ffunction-sections",
                "-fdata-sections",
                "-fno-unwind-tables",
                "-fno-asynchronous-unwind-tables",
                "-Werror=implicit-function-declaration",
            ])
            .arg("-I")
            .arg(&config)
            .arg("-I")
            .arg(&runtime)
            .arg("-I")
            .arg(&include)
            .arg("-I")
            .arg(&port)
            .arg("-isystem")
            .arg(config.join("freestanding"))
            .arg("-c")
            .arg(&source)
            .arg("-o")
            .arg(&object)
            .status()?;
        if !status.success() {
            return Err(format!("clang could not compile {}", source.display()).into());
        }
        println!(
            "cargo:rustc-link-arg-bin=fr8029d-ble-adv={}",
            object.display()
        );
    }
    Ok(())
}

fn freertos_kernel(manifest_dir: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let kernel = env::var_os("FREERTOS_KERNEL_DIR").map_or_else(
        || manifest_dir.join("../../target/fr8029d-freertos/kernel"),
        PathBuf::from,
    );
    let revision = Command::new("git")
        .arg("-C")
        .arg(&kernel)
        .args(["rev-parse", "HEAD"])
        .output()?;
    if !revision.status.success()
        || String::from_utf8_lossy(&revision.stdout).trim() != FREERTOS_REVISION
    {
        return Err(format!(
            concat!(
                "FreeRTOS-Kernel V11.2.0 ({}) is required at {}. ",
                "Clone https://github.com/FreeRTOS/FreeRTOS-Kernel.git with ",
                "--depth 1 --branch V11.2.0 or set FREERTOS_KERNEL_DIR to the pinned checkout."
            ),
            FREERTOS_REVISION,
            kernel.display(),
        )
        .into());
    }
    Ok(kernel.canonicalize()?)
}

#[cfg(feature = "ble-gatt-ota")]
#[path = "src/ota_policy.rs"]
mod ota_policy;

#[cfg(feature = "ble-gatt-ota")]
fn pin_ota_image() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-env-changed=FR29_OTA_IMAGE");
    let path = std::fs::canonicalize(env::var("FR29_OTA_IMAGE")?)?;
    println!("cargo:rerun-if-changed={}", path.display());
    if !(36..=232 * 1024).contains(&std::fs::metadata(&path)?.len()) {
        return Err("FR29_OTA_IMAGE must be a layout-v1 raw application".into());
    }
    let data = std::fs::read(path)?;
    let policy = ota_policy::manifest(&data).ok_or("unrepresentable image length")?;
    std::fs::write(
        PathBuf::from(env::var("OUT_DIR")?).join("ota-manifest.bin"),
        policy,
    )?;
    println!("cargo:rustc-link-arg-bin=fr8029d-ble-adv=--defsym=__gatt_ota=1");
    Ok(())
}
