// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Host-only exact final-image replay of the `no_std` composition.
use moteframe_core::{Dimensions, Display, Rect};
use moteframe_device_foundation::{
    ASSET_BYTES, INPUT_EVENTS, RAW_INPUTS, UI_PIXELS, VALUE_BYTES,
    model::{Error, Sink},
    probe,
};
use moteframe_ui::graphics::pixelcolor::{Rgb565, RgbColor};
use std::{error::Error as StdError, fs, path::PathBuf};
#[derive(Debug)]
struct Screen {
    pixels: [Rgb565; 512],
    sink: Sink,
}
impl Display for Screen {
    type Color = Rgb565;
    type Error = Error;
    fn dimensions(&self) -> Dimensions {
        self.sink.dimensions()
    }
    fn write(&mut self, area: Rect, pixels: &[Rgb565]) -> Result<(), Error> {
        self.sink.write(area, pixels)?;
        for (i, p) in pixels.iter().enumerate() {
            self.pixels[usize::from(area.y) * 32 + usize::from(area.x) + i] = *p;
        }
        Ok(())
    }
}
fn main() -> Result<(), Box<dyn StdError>> {
    let destination = PathBuf::from(std::env::args().nth(1).ok_or("provide output PPM path")?);
    let mut screen = Screen {
        pixels: [Rgb565::BLACK; 512],
        sink: Sink::new(),
    };
    let o = probe::exercise(&mut screen, 0).map_err(|e| format!("replay failed: {e:?}"))?;
    let mut image = b"P6\n32 16\n255\n".to_vec();
    for p in screen.pixels {
        image.extend_from_slice(&[scale(p.r(), 31), scale(p.g(), 63), scale(p.b(), 31)]);
    }
    fs::write(destination, image)?;
    println!(
        "{{\"capacities\":[{UI_PIXELS},{RAW_INPUTS},{INPUT_EVENTS},{VALUE_BYTES},{ASSET_BYTES}],\"samples\":{},\"events\":{},\"accepted\":{},\"received\":{},\"recoveries\":{},\"selection\":{},\"tx_fingerprint\":{}}}",
        o.samples, o.events, o.accepted, o.received, o.recoveries, o.selection, o.tx_fingerprint
    );
    Ok(())
}

fn scale(component: u8, maximum: u16) -> u8 {
    u8::try_from(u16::from(component) * 255 / maximum).unwrap_or(255)
}
