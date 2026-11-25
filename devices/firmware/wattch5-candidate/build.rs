//! Bind explicit test-only input and preserve the existing native memory map.
mod profile;
use std::{env, fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    for key in ["SYD881X_FLASH_BACKUP", "WATTCH5_CANDIDATE_PROFILE"] {
        println!("cargo:rerun-if-env-changed={key}");
    }
    println!("cargo:rerun-if-changed=profile.rs");
    let out = PathBuf::from(env::var_os("OUT_DIR").ok_or("OUT_DIR missing")?);
    let mut source = String::new();
    if let Some(path) = env::var_os("SYD881X_FLASH_BACKUP") {
        let path = fs::canonicalize(path)?;
        println!("cargo:rerun-if-changed={}", path.display());
        let flash = fs::read(path)?;
        if flash.len() != 0x80000 {
            return Err("full physical512KiB backup required".into());
        }
        let start = u32::from_le_bytes(flash[6..10].try_into()?) as usize;
        if start < 49 || start + 6 > 4096 {
            return Err("table offset outside boot page".into());
        }
        let count = u16::from_le_bytes([flash[start + 4], flash[start + 5]]) as usize;
        let end = start + 6 + count * 3;
        if count == 0 || end > 4096 || flash[start + 1] & 15 != 0 {
            return Err("bounded original64MHz table required".into());
        }
        source.push_str(&format!(
            "#[used] #[unsafe(link_section=\".board_configuration\")] static TABLE_BYTES:[u8;{}]={:?};\nconst BOARD_TABLE:Option<&[u8]>=Some(&TABLE_BYTES);\n",
            end-start, &flash[start..end]
        ));
    } else {
        source.push_str("const BOARD_TABLE:Option<&[u8]>=None;\n");
    }
    if let Some(path) = env::var_os("WATTCH5_CANDIDATE_PROFILE") {
        let path = fs::canonicalize(path)?;
        println!("cargo:rerun-if-changed={}", path.display());
        let profile_text = fs::read_to_string(path)?;
        let p = profile::parse(&profile_text)?;
        source.push_str(&format!("#[used] #[unsafe(link_section=\".bringup_profile\")] static PROFILE_BYTES:[u8;{}]={:?};\n",profile_text.len(),profile_text.as_bytes()));
        let mut adv = vec![2, 1, 6, 3, 3];
        adv.extend(p.service.to_le_bytes());
        adv.push((p.name.len() + 1).try_into()?);
        adv.push(9);
        adv.extend(p.name.as_bytes());
        if adv.len() > 31 {
            return Err("legacy advertisement overflow".into());
        }
        source.push_str(&format!("const TEST_PROFILE:Option<TestProfile>=Some(TestProfile{{address:{:?},service:{},rx:{},tx:{},name:&{:?},adv:&{:?},product_id:{},build_id:{}}});\n",p.address,p.service,p.rx,p.tx,p.name.as_bytes(),adv,p.product_id,p.build_id));
    } else {
        source.push_str("const TEST_PROFILE:Option<TestProfile>=None;\n");
    }
    for key in ["WATTCH5_CANDIDATE_CONFIG", "WATTCH5_CANDIDATE_POLICY"] {
        println!("cargo:rerun-if-env-changed={key}");
    }
    if env::var("TARGET")? == "thumbv6m-none-eabi"
        && env::var_os("CARGO_FEATURE_ARM_IMAGE").is_some()
    {
        let config_path = fs::canonicalize(
            env::var_os("WATTCH5_CANDIDATE_CONFIG").ok_or("explicit candidate config required")?,
        )?;
        let policy_path = fs::canonicalize(
            env::var_os("WATTCH5_CANDIDATE_POLICY")
                .ok_or("explicit experimental native policy required")?,
        )?;
        println!("cargo:rerun-if-changed={}", config_path.display());
        println!("cargo:rerun-if-changed={}", policy_path.display());
        let config = fs::read_to_string(config_path)?;
        let policy = fs::read_to_string(policy_path)?;
        if (json_value(&config, "format")? == "W5UIF001")
            != env::var_os("CARGO_FEATURE_FACTORY_UI").is_some()
        {
            return Err("candidate UI reader/resource selection mismatch".into());
        }
        for (key, want) in [
            ("selected_schema_max_body", 474),
            ("transport_body", 512),
            ("capacity", 4194304),
            ("write_size", 256),
            ("erase_size", 4096),
            ("image_capacity", 128),
        ] {
            if number(&config, key)? != want {
                return Err("candidate capacity/schema input mismatch".into());
            }
        }
        for (text, key, want) in [
            (&config, "physical_grant", "false"),
            (&config, "hardware_qualified", "false"),
            (&policy, "hardware_qualified", "false"),
            (&policy, "automatic_unprotect", "false"),
            (&policy, "automatic_format", "false"),
            (&policy, "laboratory_access_only", "true"),
            (&policy, "profile_kind", "experimental-offline-candidate"),
            (&policy, "expected_jedec", "5e4016"),
            (&config, "expected_jedec", "5e4016"),
        ] {
            if json_value(text, key)? != want {
                return Err("explicit experimental-only candidate policy required".into());
            }
        }
        for (name, key) in [
            ("NAMESPACE", "namespace"),
            ("DATASET", "dataset"),
            ("PROVISIONING", "provisioning_identity"),
        ] {
            let bytes = hex::<16>(json_value(&config, key)?)?;
            source.push_str(&format!("const {name}:[u8;16]={bytes:?};\n"));
        }
        let mut epochs = [0u32; 6];
        for (i, key) in ["0", "1", "2", "5", "6", "7"].iter().enumerate() {
            epochs[i] = number(&config, key)?.try_into()?;
            if epochs[i] == 0 {
                return Err("zero domain epoch".into());
            }
        }
        source.push_str(&format!("const DOMAIN_EPOCHS:[u32;6]={epochs:?};\n"));
        if env::var_os("CARGO_FEATURE_NATIVE_HEALTH").is_some() {
            // An old six-domain profile is never authority for the new writer handles.
            if json_value(&policy, "allow_native_health")? != "true" {
                return Err("explicit native health writer policy required".into());
            }
            for (key, want) in [
                ("health_metadata_start", 0x39b000),
                ("health_metadata_length", 0x2000),
                ("health_log_start", 0x39d000),
                ("health_log_length", 0x62000),
            ] {
                if number(&config, key)? != want || number(&policy, key)? != want {
                    return Err("native health writer partition mismatch".into());
                }
            }
            let mut epochs = [0u32; 3];
            for (i, key) in ["health_epoch_3", "health_epoch_4", "health_epoch_8"]
                .iter()
                .enumerate()
            {
                epochs[i] = number(&config, key)?.try_into()?;
                if epochs[i] == 0 {
                    return Err("zero health domain epoch".into());
                }
            }
            source.push_str(&format!("const HEALTH_DOMAIN_EPOCHS:[u32;3]={epochs:?};\n"));
        }
        let jedec = hex::<3>(json_value(&policy, "expected_jedec")?)?;
        source.push_str(&format!(
            "const DATA_UUID:u16={};\n",
            number(&policy, "data_uuid16")?
        ));
        source.push_str(&format!("const NOR_POLICY:device_spi_nor::writable::ProfileConfig=device_spi_nor::writable::ProfileConfig{{id:{jedec:?},capacity:4194304,write_size:256,protection_mask:{},protection_value:{},poll_interval_ms:{},program_max_ms:{},erase_max_ms:{},sync_max_ms:{},operation_max_ms:{}}};\n",number(&policy,"protection_mask")?,number(&policy,"protection_value")?,number(&policy,"poll_interval_ms")?,number(&policy,"program_max_ms")?,number(&policy,"erase_max_ms")?,number(&policy,"sync_max_ms")?,number(&policy,"operation_max_ms")?));
        for (name, section, raw) in [
            ("CANDIDATE_CONFIG", ".candidate_config", config),
            ("CANDIDATE_POLICY", ".candidate_policy", policy),
        ] {
            source.push_str(&format!(
                "#[used] #[unsafe(link_section=\"{section}\")] static {name}:[u8;{}]={:?};\n",
                raw.len(),
                raw.as_bytes()
            ));
        }
    }
    fs::write(out.join("bindings.rs"), source)?;
    if env::var("TARGET")? == "thumbv6m-none-eabi"
        && env::var_os("CARGO_FEATURE_ARM_IMAGE").is_some()
    {
        let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").ok_or("manifest missing")?);
        let script = manifest.join("../../platforms/syd881x/runtime/linker/wattch5-candidate.x");
        println!("cargo:rerun-if-changed={}", script.display());
        fs::copy(script, out.join("link.x"))?;
        println!(
            "cargo:rustc-link-arg-bin=wattch5-candidate=-T{}",
            out.join("link.x").display()
        );
    }
    Ok(())
}

fn json_value<'a>(text: &'a str, key: &str) -> Result<&'a str, Box<dyn std::error::Error>> {
    let needle = format!("\"{key}\"");
    let mut matches = text.match_indices(&needle);
    let (_, tail) = matches
        .next()
        .map(|(n, _)| (n, &text[n + needle.len()..]))
        .ok_or("missing config key")?;
    if matches.next().is_some() {
        return Err("duplicate config key".into());
    }
    let tail = tail
        .trim_start()
        .strip_prefix(':')
        .ok_or("config colon")?
        .trim_start();
    let value = if let Some(t) = tail.strip_prefix('"') {
        t.split('"').next().ok_or("config string")?
    } else {
        tail.split([',', '}', '\n'])
            .next()
            .ok_or("config number")?
            .trim()
    };
    Ok(value)
}
fn number(text: &str, key: &str) -> Result<u64, Box<dyn std::error::Error>> {
    Ok(json_value(text, key)?.parse()?)
}
fn hex<const N: usize>(value: &str) -> Result<[u8; N], Box<dyn std::error::Error>> {
    if value.len() != N * 2 || !value.is_ascii() {
        return Err("hex length".into());
    }
    let mut out = [0; N];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&value[i * 2..i * 2 + 2], 16)?;
    }
    Ok(out)
}
