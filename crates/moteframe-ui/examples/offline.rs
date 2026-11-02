// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Offline scene rendering; writes a PPM using only a host memory display.
use core::convert::Infallible;
use moteframe_ui::graphics::pixelcolor::{Rgb565 as Packed, raw::RawU16};
use std::{
    fs::File,
    io::{BufWriter, Write},
};

use graphics::{
    mono_font::{MonoTextStyle, ascii::FONT_6X10},
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{Circle, Line, PrimitiveStyle, Rectangle},
    text::{Baseline, Text},
};
use moteframe_core::{Dimensions, Display, Rect};
use moteframe_ui::{DirtyRegion, graphics, repaint};

const WIDTH: u16 = 64;
const HEIGHT: u16 = 40;

#[derive(Debug)]
struct MemoryDisplay {
    pixels: [Packed; 2560],
    writes: usize,
}
impl Display for MemoryDisplay {
    type Color = Packed;
    type Error = &'static str;
    fn dimensions(&self) -> Dimensions {
        Dimensions::new(WIDTH, HEIGHT)
    }
    fn write(&mut self, area: Rect, pixels: &[Packed]) -> Result<(), Self::Error> {
        if !area.fits(self.dimensions()) || usize::try_from(area.area()).ok() != Some(pixels.len())
        {
            return Err("invalid memory display write");
        }
        for (row, colors) in pixels.chunks(usize::from(area.width)).enumerate() {
            let start = (usize::from(area.y) + row) * usize::from(WIDTH) + usize::from(area.x);
            self.pixels[start..start + colors.len()].copy_from_slice(colors);
        }
        self.writes += 1;
        Ok(())
    }
}

fn scene<T: DrawTarget<Color = Rgb565, Error = Infallible>>(
    target: &mut T,
) -> Result<(), Infallible> {
    Rectangle::new(Point::new(1, 1), Size::new(62, 38))
        .into_styled(PrimitiveStyle::with_stroke(Rgb565::WHITE, 1))
        .draw(target)?;
    Text::with_baseline(
        "UI",
        Point::new(6, 5),
        MonoTextStyle::new(&FONT_6X10, Rgb565::WHITE),
        Baseline::Top,
    )
    .draw(target)?;
    Rectangle::new(Point::new(6, 20), Size::new(18, 13))
        .into_styled(PrimitiveStyle::with_fill(Rgb565::RED))
        .draw(target)?;
    Circle::new(Point::new(32, 18), 16)
        .into_styled(PrimitiveStyle::with_fill(Rgb565::GREEN))
        .draw(target)?;
    Line::new(Point::new(50, 8), Point::new(57, 32))
        .into_styled(PrimitiveStyle::with_stroke(Rgb565::BLUE, 2))
        .draw(target)?;
    Ok(())
}

fn main() -> Result<(), &'static str> {
    let destination = std::env::args().nth(1).ok_or("usage: offline OUTPUT.ppm")?;
    let mut display = MemoryDisplay {
        pixels: [Packed::from(RawU16::new(0)); 2560],
        writes: 0,
    };
    let mut dirty = DirtyRegion::new(display.dimensions());
    dirty.invalidate_all();
    repaint(
        &mut display,
        &mut dirty,
        &mut [Packed::from(RawU16::new(0)); 16],
        Rgb565::BLACK,
        |tile| scene(tile),
    )
    .map_err(|_| "offline repaint failed")?;
    let file = File::create(destination).map_err(|_| "cannot create PPM")?;
    let mut output = BufWriter::new(file);
    write!(output, "P6\n{WIDTH} {HEIGHT}\n255\n").map_err(|_| "cannot write header")?;
    for color in display.pixels {
        let rgb = [
            u16::from(color.r()) * 255 / 31,
            u16::from(color.g()) * 255 / 63,
            u16::from(color.b()) * 255 / 31,
        ];
        for channel in rgb {
            output
                .write_all(&[u8::try_from(channel).map_err(|_| "invalid channel")?])
                .map_err(|_| "cannot write pixels")?;
        }
    }
    output.flush().map_err(|_| "cannot flush PPM")?;
    println!(
        "Rendered {WIDTH}x{HEIGHT}: {} rectangular writes, 16 scratch pixels",
        display.writes
    );
    Ok(())
}
