// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Offline layout and fixed-workload compiler probes; never firmware startup.
#![cfg_attr(target_os = "none", no_std)]
use core::{convert::Infallible, hint::black_box, mem::size_of};
use moteframe_core::{DelayMs, Dimensions, DisplayBus};
use moteframe_display::DcsDisplay;
use moteframe_ui::{
    DirtyRegion, StreamTarget, Tile,
    graphics::{
        pixelcolor::{BinaryColor, Gray8, Rgb565, Rgb888},
        prelude::*,
    },
    repaint,
};

type RgbTile<'a> = Tile<'a, Rgb565>;

macro_rules! layout {
    ($($name:ident: $ty:ty),+ $(,)?) => {
        $(#[doc = concat!("Compiler layout marker for ", stringify!($ty), ".")]
          #[used] pub static $name: [u8; size_of::<$ty>()] = [0; size_of::<$ty>()];)+
        #[cfg(not(target_os = "none"))]
        fn main() {
            println!("{{");
            $(println!("  \"{}\": {},", stringify!($name), size_of::<$ty>());)+
            println!("  \"wire_rgb565_bytes\": 2, \"driver_scratch_bytes\": 64\n}}");
        }
    };
}

layout! {
    MF_PIXEL_RGB565: Rgb565,
    MF_PIXEL_RGB888: Rgb888,
    MF_PIXEL_GRAY8: Gray8,
    MF_PIXEL_BINARY: BinaryColor,
    MF_RUN_RGB565: [Rgb565; 32],
    MF_RUN_RGB888: [Rgb888; 32],
    MF_RUN_GRAY8: [Gray8; 32],
    MF_RUN_BINARY: [BinaryColor; 32],
    MF_ROWS_RGB565: [Rgb565; 480],
    MF_ROWS_RGB888: [Rgb888; 480],
    MF_ROWS_GRAY8: [Gray8; 480],
    MF_ROWS_BINARY: [BinaryColor; 480],
    MF_STREAM: StreamTarget<'static, DcsDisplay<ByteBus>>,
    MF_TILE: RgbTile<'static>,
    MF_DIRTY: DirtyRegion,
    MF_DRIVER: DcsDisplay<ByteBus>,
}

/// Synthetic byte consumer with observable output; no pins or hardware.
#[derive(Debug)]
pub struct ByteBus {
    /// Rolling checksum supplied and returned by the probe.
    pub checksum: u32,
}
impl DisplayBus for ByteBus {
    type Error = Infallible;
    fn command(&mut self, command: u8, data: &[u8]) -> Result<(), Infallible> {
        self.checksum = self.checksum.wrapping_add(u32::from(command));
        self.pixels(data)
    }
    fn pixels(&mut self, bytes: &[u8]) -> Result<(), Infallible> {
        for byte in black_box(bytes) {
            self.checksum = self.checksum.rotate_left(3) ^ u32::from(*byte);
        }
        Ok(())
    }
}
struct Delay;
impl DelayMs for Delay {
    fn delay_ms(&mut self, _: u16) {}
}

/// Compile the RGB565 stream/encoder path with a runtime width and color.
#[inline(never)]
pub fn probe_rgb565(bus: ByteBus, width: u16, color: Rgb565) -> u32 {
    let mut display = DcsDisplay::new(bus, Dimensions::new(width, 1), (0, 0));
    if display.initialize(&[], &mut Delay).is_err() {
        return 0;
    }
    let result = StreamTarget::new(&mut display)
        .draw_iter((0..width).map(|x| Pixel(Point::new(i32::from(x), 0), color)));
    if result.is_err() {
        return 0;
    }
    display.into_bus().checksum
}

/// Compile typed partial repaint with sixteen caller-owned scratch pixels.
#[inline(never)]
pub fn probe_repaint(bus: ByteBus, width: u16, color: Rgb565) -> u32 {
    let mut display = DcsDisplay::new(bus, Dimensions::new(width, 1), (0, 0));
    if display.initialize(&[], &mut Delay).is_err() {
        return 0;
    }
    let mut dirty = DirtyRegion::new(Dimensions::new(width, 1));
    dirty.invalidate_all();
    let mut scratch = [Rgb565::BLACK; 16];
    if repaint(&mut display, &mut dirty, &mut scratch, color, |_| {
        Ok::<(), Infallible>(())
    })
    .is_err()
    {
        return 0;
    }
    display.into_bus().checksum
}

#[cfg(not(rgb565_only))]
mod native {
    use super::{Dimensions, Infallible, StreamTarget, black_box};
    use moteframe_core::{Display, Rect};
    use moteframe_ui::graphics::{prelude::*, primitives::Rectangle};
    struct Sink<C> {
        color: C,
        count: u32,
    }
    impl<C: Copy + PartialEq> Display for Sink<C> {
        type Color = C;
        type Error = Infallible;
        fn dimensions(&self) -> Dimensions {
            Dimensions::new(240, 1)
        }
        fn write(&mut self, _: Rect, pixels: &[C]) -> Result<(), Infallible> {
            for pixel in black_box(pixels) {
                self.count = self.count.wrapping_add(u32::from(*pixel == self.color));
            }
            Ok(())
        }
    }
    pub fn probe<C: PixelColor>(color: C) -> u32 {
        let mut display = Sink { color, count: 0 };
        let area = Rectangle::new(Point::zero(), Size::new(240, 1));
        if StreamTarget::new(&mut display)
            .fill_solid(&area, color)
            .is_err()
        {
            return 0;
        }
        display.count
    }
}

/// Additional native RGB888 instantiation; no RGB565 quantization.
#[cfg(not(rgb565_only))]
#[inline(never)]
#[must_use]
pub fn probe_rgb888(color: Rgb888) -> u32 {
    native::probe(color)
}
/// Additional native grayscale instantiation.
#[cfg(not(rgb565_only))]
#[inline(never)]
#[must_use]
pub fn probe_gray8(color: Gray8) -> u32 {
    native::probe(color)
}
/// Additional native monochrome instantiation (logical colors, not bit-packed).
#[cfg(not(rgb565_only))]
#[inline(never)]
#[must_use]
pub fn probe_binary(color: BinaryColor) -> u32 {
    native::probe(color)
}
