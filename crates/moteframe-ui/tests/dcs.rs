// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! End-to-end borrowed font to real DCS stream, replacing physical transport only.
#![cfg(feature = "refresh")]
use graphics::{
    image::ImageRaw,
    mono_font::{DecorationDimensions, MonoFont, MonoTextStyle},
    pixelcolor::Rgb565,
    prelude::*,
    primitives::Rectangle,
    text::{Baseline, Text},
};
use moteframe_core::{DelayMs, Dimensions, DisplayBus};
use moteframe_display::DcsDisplay;
use moteframe_ui::graphics::pixelcolor::{Rgb565 as Packed, raw::RawU16};
use moteframe_ui::{DirtyRegion, graphics, repaint};

#[derive(Debug, Default)]
struct Bus {
    commands: Vec<(u8, Vec<u8>)>,
    pixels: Vec<Vec<u8>>,
}
impl DisplayBus for Bus {
    type Error = &'static str;
    fn command(&mut self, command: u8, data: &[u8]) -> Result<(), Self::Error> {
        self.commands.push((command, data.to_vec()));
        Ok(())
    }
    fn pixels(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        assert!(bytes.len() <= 64);
        self.pixels.push(bytes.to_vec());
        Ok(())
    }
}
#[derive(Debug)]
struct Delay;
impl DelayMs for Delay {
    fn delay_ms(&mut self, _: u16) {}
}

#[test]
fn test_should_draw_borrowed_font_through_tiles_to_exact_dcs_windows_and_bytes() {
    // Original, hand-authored three-row A glyph; each row has MSB-first padding.
    let glyph = [0b0100_0000, 0b1010_0000, 0b1110_0000];
    let font = MonoFont {
        image: ImageRaw::new(&glyph, 3),
        character_size: Size::new(3, 3),
        character_spacing: 0,
        baseline: 2,
        strikethrough: DecorationDimensions::new(1, 1),
        underline: DecorationDimensions::new(3, 1),
        glyph_mapping: &|_| 0,
    };
    let mut display = DcsDisplay::new(Bus::default(), Dimensions::new(5, 5), (0, 0));
    assert!(display.initialize(&[], &mut Delay).is_ok());
    let mut dirty = DirtyRegion::new(Dimensions::new(5, 5));
    dirty.invalidate(Rectangle::new(Point::new(1, 1), Size::new(3, 3)));
    assert_eq!(
        repaint(
            &mut display,
            &mut dirty,
            &mut [Packed::from(RawU16::new(0)); 2],
            Rgb565::BLACK,
            |tile| {
                Text::with_baseline(
                    "A",
                    Point::new(1, 1),
                    MonoTextStyle::new(&font, Rgb565::RED),
                    Baseline::Top,
                )
                .draw(tile)
                .map(|_| ())
            }
        ),
        Ok(())
    );
    let bus = display.into_bus();
    assert_eq!(
        bus.commands,
        vec![
            (0x2a, vec![0, 1, 0, 2]),
            (0x2b, vec![0, 1, 0, 1]),
            (0x2c, vec![]),
            (0x2a, vec![0, 3, 0, 3]),
            (0x2c, vec![]),
            (0x2a, vec![0, 1, 0, 2]),
            (0x2b, vec![0, 2, 0, 2]),
            (0x2c, vec![]),
            (0x2a, vec![0, 3, 0, 3]),
            (0x2c, vec![]),
            (0x2a, vec![0, 1, 0, 2]),
            (0x2b, vec![0, 3, 0, 3]),
            (0x2c, vec![]),
            (0x2a, vec![0, 3, 0, 3]),
            (0x2c, vec![]),
        ]
    );
    assert_eq!(
        bus.pixels,
        vec![
            vec![0, 0, 0xf8, 0],
            vec![0, 0],
            vec![0xf8, 0, 0, 0],
            vec![0xf8, 0],
            vec![0xf8, 0, 0xf8, 0],
            vec![0xf8, 0],
        ]
    );
    assert_eq!(dirty.pending(), None);
}
