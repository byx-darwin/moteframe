use std::{env, fs, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=../../platforms/syd881x/runtime/linker/wattch5-product.x");
    println!("cargo:rerun-if-changed=capabilities.json");
    let features: Vec<_> = [
        "arm-image",
        "runtime",
        "runtime-trouble",
        "legacy-health",
        "blood-pressure",
        "oxygen",
        "glucose",
        "commercial",
        "development",
        "factory",
        "raw-data",
        "self-test",
        "engineering",
        "factory-config",
        "wattch5-source",
        "service-routing",
        "objects",
        "device-jobs",
        "time",
        "device",
        "system",
        "profile",
        "settings",
        "reminders",
        "find",
        "history",
        "heart-rate",
        "sleep",
        "sport",
        "activity",
        "temperature",
        "notifications",
        "watchface",
        "weather",
        "media",
        "limits",
    ]
    .into_iter()
    .filter(|name| {
        env::var_os(format!(
            "CARGO_FEATURE_{}",
            name.replace('-', "_").to_uppercase()
        ))
        .is_some()
    })
    .collect();
    let profile = if features.contains(&"development") {
        "development"
    } else if features.contains(&"factory") {
        "factory"
    } else {
        "commercial"
    };
    let source = fs::read_to_string("capabilities.json")
        .unwrap_or_else(|error| panic!("capability source: {error}"));
    let source = source
        .trim()
        .strip_prefix('{')
        .unwrap_or_else(|| panic!("capability object required"));
    let generated =
        format!("{{\"build_profile\":{profile:?},\"compiled_features\":{features:?},{source}");
    let output =
        PathBuf::from(env::var_os("OUT_DIR").unwrap_or_else(|| panic!("Cargo OUT_DIR missing")));
    fs::write(output.join("embedded_capabilities.json"), generated)
        .unwrap_or_else(|error| panic!("capability output: {error}"));
    if env::var("TARGET").as_deref() == Ok("thumbv6m-none-eabi") {
        let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../platforms/syd881x/runtime/linker/wattch5-product.x");
        println!(
            "cargo:rustc-link-arg-bin=wattch5-product=-T{}",
            script.display()
        );
    }
}
