// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Executed consumers with explicit capacities; no device or product content.
#![no_std]
use core::hint::black_box;

#[cfg(any(ai_assets2, ai_assets16))]
mod assets {
    use super::black_box;
    use embedded_graphics_core::pixelcolor::{Rgb565, RgbColor};
    #[cfg(ai_layout)]
    use moteframe_assets::{Glyph, Mono8};
    use moteframe_assets::{Raster, Rgb565Be};
    use moteframe_storage::{ReadWindow, SliceSource};
    #[cfg(ai_assets2)]
    const SCRATCH: usize = 2;
    #[cfg(ai_assets16)]
    const SCRATCH: usize = 16;
    #[cfg(ai_layout)]
    #[unsafe(no_mangle)]
    pub static MODULE_OWNER: [u8; core::mem::size_of::<Raster<Rgb565Be>>()] =
        [0; core::mem::size_of::<Raster<Rgb565Be>>()];
    #[cfg(ai_layout)]
    #[unsafe(no_mangle)]
    pub static MODULE_GLYPH: [u8; core::mem::size_of::<Glyph<Mono8>>()] =
        [0; core::mem::size_of::<Glyph<Mono8>>()];
    #[cfg(ai_layout)]
    #[unsafe(no_mangle)]
    pub static MODULE_READER: [u8; core::mem::size_of::<ReadWindow<SliceSource<'static>>>()] =
        [0; core::mem::size_of::<ReadWindow<SliceSource<'static>>>()];
    #[cfg(ai_layout)]
    #[unsafe(no_mangle)]
    pub static MODULE_SCRATCH: [u8; SCRATCH] = [0; SCRATCH];
    #[cfg(ai_layout)]
    #[unsafe(no_mangle)]
    pub static MODULE_PIXELS: [u8; core::mem::size_of::<[Rgb565; 32]>()] =
        [0; core::mem::size_of::<[Rgb565; 32]>()];

    #[inline(never)]
    pub fn exercise(seed: u32) -> u32 {
        let mut input = [0u8; 64];
        for (i, byte) in input.iter_mut().enumerate() {
            *byte = seed.wrapping_add(i as u32) as u8;
        }
        let Ok(source) = SliceSource::new(black_box(&input)) else {
            return 0;
        };
        let Ok(mut view) = ReadWindow::new(source, black_box(0), black_box(64)) else {
            return 1;
        };
        let Ok(raster) = Raster::<Rgb565Be>::new(0, black_box(32), 1) else {
            return 2;
        };
        let mut output = [Rgb565::BLACK; 32];
        let mut scratch = [0u8; SCRATCH];
        black_box(&mut scratch);
        if raster
            .read_span(&mut view, 0, 0, &mut output, &mut scratch)
            .is_err()
        {
            return 3;
        }
        let mut sum = seed;
        for pixel in output {
            sum = sum.wrapping_mul(31).wrapping_add(
                u32::from(pixel.r()) * 4096 + u32::from(pixel.g()) * 32 + u32::from(pixel.b()),
            );
        }
        black_box(sum)
    }
}
#[cfg(any(ai_assets2, ai_assets16))]
pub use assets::exercise;

#[cfg(any(ai_input1, ai_input4, ai_input16))]
mod input {
    use super::black_box;
    use moteframe_input::{
        Button, ButtonConfig, ButtonEvent, EventQueue, Point, Touch, TouchConfig, TouchEvent,
        TouchSample,
    };
    #[cfg(ai_input1)]
    const N: usize = 1;
    #[cfg(ai_input4)]
    const N: usize = 4;
    #[cfg(ai_input16)]
    const N: usize = 16;
    #[cfg(ai_layout)]
    #[unsafe(no_mangle)]
    pub static MODULE_BUTTON: [u8; core::mem::size_of::<Button>()] =
        [0; core::mem::size_of::<Button>()];
    #[cfg(ai_layout)]
    #[unsafe(no_mangle)]
    pub static MODULE_TOUCH: [u8; core::mem::size_of::<Touch>()] =
        [0; core::mem::size_of::<Touch>()];
    #[cfg(ai_layout)]
    #[unsafe(no_mangle)]
    pub static MODULE_QUEUE: [u8; core::mem::size_of::<EventQueue<ButtonEvent, N>>()] =
        [0; core::mem::size_of::<EventQueue<ButtonEvent, N>>()];
    #[cfg(ai_layout)]
    #[unsafe(no_mangle)]
    pub static MODULE_TOUCH_QUEUE: [u8; core::mem::size_of::<EventQueue<TouchEvent, N>>()] =
        [0; core::mem::size_of::<EventQueue<TouchEvent, N>>()];

    #[inline(never)]
    pub fn exercise(seed: u32) -> u32 {
        let Ok(config) = ButtonConfig::new(black_box(0), Some(black_box(10))) else {
            return 0;
        };
        let mut button = Button::new(config);
        let mut queue = EventQueue::<ButtonEvent, N>::new();
        black_box(&mut queue);
        let now = u64::from(seed);
        if button.sample(true, now, &mut queue).is_err() {
            return 1;
        }
        let mut sum = seed
            .wrapping_mul(31)
            .wrapping_add(u32::from(queue.pop().is_some()));
        if button.sample(true, now + 10, &mut queue).is_err() {
            return 2;
        }
        sum = sum.wrapping_add(u32::from(queue.pop() == Some(ButtonEvent::Held)));
        let Ok(config) = TouchConfig::new(320, 240, 4, 20, 100) else {
            return 3;
        };
        let mut touch = Touch::new(config);
        let mut gestures = EventQueue::<TouchEvent, N>::new();
        black_box(&mut gestures);
        if touch
            .update(
                TouchSample::Down(Point { x: 10, y: 10 }),
                now,
                &mut gestures,
            )
            .is_err()
        {
            return 4;
        }
        let x = black_box(10 + (seed & 63) as u16);
        if touch
            .update(TouchSample::Up(Point { x, y: 10 }), now + 1, &mut gestures)
            .is_err()
        {
            return 5;
        }
        match gestures.pop() {
            Some(TouchEvent::Tap(_)) => sum = sum.wrapping_add(11),
            Some(TouchEvent::Swipe(_)) => sum = sum.wrapping_add(13),
            _ => {}
        }
        black_box(sum)
    }
}
#[cfg(any(ai_input1, ai_input4, ai_input16))]
pub use input::exercise;

#[cfg(ai_off)]
#[inline(never)]
pub fn exercise(seed: u32) -> u32 {
    black_box(seed.wrapping_mul(31).wrapping_add(17))
}
#[cfg(all(ai_off, ai_layout))]
#[unsafe(no_mangle)]
pub static MODULE_OWNER: [u8; 4] = [0; 4];
