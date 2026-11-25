//! Bind explicit test-only input and preserve the existing native memory map.
mod profile;
use std::{env, fs, path::PathBuf};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    for key in ["SYD881X_FLASH_BACKUP", "WATTCH5_BRINGUP_PROFILE"] {
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
    if let Some(path) = env::var_os("WATTCH5_BRINGUP_PROFILE") {
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
    fs::write(out.join("bindings.rs"), source)?;
    if env::var("TARGET")? == "thumbv6m-none-eabi" {
        let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").ok_or("manifest missing")?);
        let script = manifest.join("../../platforms/syd881x/runtime/linker/wattch5-bringup.x");
        println!("cargo:rerun-if-changed={}", script.display());
        fs::copy(script, out.join("link.x"))?;
        println!(
            "cargo:rustc-link-arg-bin=wattch5-bringup=-T{}",
            out.join("link.x").display()
        );
    }
    Ok(())
}
