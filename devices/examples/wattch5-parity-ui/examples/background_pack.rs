//! Offline private V2 image generation; never reads vendor resources or device data.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use device_ui::graphics::{pixelcolor::Rgb565, prelude::*};
use std::{collections::BTreeMap, fs, path::PathBuf};
use wattch5_parity_ui_demo::{Locale, Model, Screen, Snapshot};
#[derive(Debug)]
struct Frame(Vec<u16>);
impl OriginDimensions for Frame {
    fn size(&self) -> Size {
        Size::new(128, 128)
    }
}
impl DrawTarget for Frame {
    type Color = Rgb565;
    type Error = ();
    fn draw_iter<I: IntoIterator<Item = Pixel<Rgb565>>>(&mut self, i: I) -> Result<(), ()> {
        for Pixel(p, c) in i {
            self.0[usize::try_from(p.y).map_err(|_| ())? * 128
                + usize::try_from(p.x).map_err(|_| ())?] = c.into_storage();
        }
        Ok(())
    }
}
fn main() {
    let output = PathBuf::from(std::env::args().nth(1).expect("output"));
    let packed = std::env::args().nth(2).as_deref() == Some("--packed");
    let mut directory = vec![0u32; 62 * 2 * 18];
    let mut maps = Vec::new();
    let mut tiles = Vec::<u8>::new();
    let mut tile_ids = BTreeMap::new();
    let mut scenes = 0;
    let data_start = 32 + directory.len() * 4;
    for page in Screen::ALL {
        for locale in [Locale::Zh, Locale::En] {
            for v in 0..Model::background_variants(page) {
                let m = Model::background_model(page, locale, v).unwrap();
                let mut f = Frame(vec![0; 16384]);
                m.draw_background(&mut f).unwrap();
                directory[usize::from(m.background_key())] =
                    u32::try_from(data_start + maps.len()).unwrap();
                // Canonicalize the Theme preview palette as well as normal Phosphor scenes.
                let data = Snapshot::unavailable();
                let palette = m
                    .product_scene(&data)
                    .unwrap()
                    .palette()
                    .map(IntoStorage::into_storage);
                if packed {
                    for chunk in f.0.chunks_exact(4) {
                        let mut byte = 0u8;
                        for pixel in chunk {
                            byte = (byte << 2)
                                | u8::try_from(palette.iter().position(|v| v == pixel).unwrap())
                                    .unwrap();
                        }
                        maps.push(byte);
                    }
                    scenes += 1;
                    continue;
                }
                for ty in 0..16 {
                    for tx in 0..4 {
                        let mut tile = Vec::new();
                        for y in ty * 8..ty * 8 + 8 {
                            for x in tx * 32..tx * 32 + 32 {
                                let index =
                                    palette.iter().position(|v| *v == f.0[y * 128 + x]).unwrap();
                                tile.extend([129u16, 57177, 48941, 36111][index].to_be_bytes());
                            }
                        }
                        let id = if let Some(id) = tile_ids.get(&tile) {
                            *id
                        } else {
                            let id = u16::try_from(tiles.len() / 512).unwrap();
                            tiles.extend_from_slice(&tile);
                            tile_ids.insert(tile, id);
                            id
                        };
                        maps.extend(id.to_le_bytes());
                    }
                }
                scenes += 1;
            }
        }
    }
    let tile_start = if packed {
        data_start
    } else {
        data_start + maps.len()
    };
    let total = data_start + maps.len() + tiles.len();
    assert!(total <= 0x0020_0000);
    let mut bytes = Vec::new();
    bytes.extend(if packed { b"W5UIV202" } else { b"W5UIV201" });
    for v in [
        if packed { 2u32 } else { 1u32 },
        u32::try_from(directory.len()).unwrap(),
        u32::try_from(data_start).unwrap(),
        u32::try_from(tile_start).unwrap(),
        u32::try_from(total).unwrap(),
        scenes,
    ] {
        bytes.extend(v.to_le_bytes());
    }
    for offset in directory {
        bytes.extend(offset.to_le_bytes());
    }
    bytes.extend(maps);
    bytes.extend(tiles);
    assert_eq!(bytes.len(), total);
    fs::create_dir_all(&output).unwrap();
    fs::write(output.join("v2-backgrounds.bin"), bytes).unwrap();
    println!(
        "scenes={scenes} unique_tiles={} bytes={total}",
        tile_ids.len()
    );
}
