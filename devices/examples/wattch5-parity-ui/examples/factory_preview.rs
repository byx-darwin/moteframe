//! Offline frames from the same factory compositor used by the product adapter.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use device_ui::graphics::{pixelcolor::Rgb565, prelude::*};
use std::{fs, path::PathBuf};
use wattch5_parity_ui_demo::{FactoryAssets, Locale, Model, Screen, Snapshot};
#[derive(Debug)]
struct Bytes(Vec<u8>);
impl moteframe_core::ReadAt for Bytes {
    type Error = ();
    fn capacity(&self) -> u32 {
        u32::try_from(self.0.len()).unwrap()
    }
    fn read_exact(&mut self, offset: u32, out: &mut [u8]) -> Result<(), ()> {
        assert!(out.len() <= 256);
        out.copy_from_slice(
            self.0
                .get(offset as usize..offset as usize + out.len())
                .ok_or(())?,
        );
        Ok(())
    }
}
fn main() {
    let path = PathBuf::from(std::env::args().nth(1).expect("private image pack"));
    let output = PathBuf::from(std::env::args().nth(2).expect("output"));
    fs::create_dir_all(&output).unwrap();
    let assets = FactoryAssets::open(Bytes(fs::read(path).unwrap())).unwrap();
    let data = Snapshot::unavailable();
    let mut count = 0;
    for page in Screen::ALL {
        for locale in [Locale::Zh, Locale::En] {
            for variant in 0..Model::background_variants(page) {
                let m = Model::background_model(page, locale, variant).unwrap();
                let mut row = [Rgb565::new(0, 0, 0); 128];
                let mut bytes = Vec::new();
                for y in 0..128 {
                    assets.render(&m, &data, y, &mut row).unwrap();
                    for p in row {
                        bytes.extend(p.into_storage().to_be_bytes());
                    }
                }
                fs::write(
                    output.join(format!(
                        "{}-{}-{variant}.rgb565",
                        page.id(),
                        if locale == Locale::Zh { "zh" } else { "en" }
                    )),
                    bytes,
                )
                .unwrap();
                count += 1;
            }
        }
    }
    println!("Rendered {count} actual factory product frames; hardware_execution=false");
}
