// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

use embedded_graphics::{pixelcolor::PixelColor, prelude::*, primitives::Rectangle};
use moteframe_core::{Display, Rect};

use crate::geometry::{clip, contiguous_pixels, visible};

/// Immediate typed-color drawing adapter with bounded horizontal-run buffering.
///
/// Each drawing call finishes its writes before returning. Arbitrarily ordered
/// pixels retain their overwrite order. There is no full-screen framebuffer.
/// Any implementation of the hardware-independent `Display` contract works.
#[derive(Debug)]
pub struct StreamTarget<'a, D, const RUN: usize = 32> {
    display: &'a mut D,
}

impl<'a, D: Display> StreamTarget<'a, D> {
    /// Borrows a display using the backward-compatible 32-pixel run capacity.
    pub fn new(display: &'a mut D) -> Self {
        Self::with_run(display)
    }
}

impl<'a, D: Display, const RUN: usize> StreamTarget<'a, D, RUN> {
    const VALID: () = assert!(RUN > 0 && RUN <= u16::MAX as usize, "RUN must be 1..=65535");
    /// Borrows a prepared display with compile-time checked run capacity.
    /// `RUN` must be in 1..=65535; use this constructor for explicit capacities.
    pub fn with_run(display: &'a mut D) -> Self {
        let () = Self::VALID;
        Self { display }
    }

    fn write_run(
        &mut self,
        x: u16,
        y: u16,
        width: u16,
        pixels: &[D::Color; RUN],
    ) -> Result<(), D::Error> {
        self.display
            .write(Rect::new(x, y, width, 1), &pixels[..usize::from(width)])
    }
}

impl<D: Display, const RUN: usize> OriginDimensions for StreamTarget<'_, D, RUN> {
    fn size(&self) -> Size {
        let dimensions = self.display.dimensions();
        Size::new(u32::from(dimensions.width), u32::from(dimensions.height))
    }
}

impl<D: Display, const RUN: usize> DrawTarget for StreamTarget<'_, D, RUN>
where
    D::Color: PixelColor,
{
    type Color = D::Color;
    type Error = D::Error;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<D::Color>>,
    {
        let dimensions = self.display.dimensions();
        let mut pixels = pixels.into_iter();
        let (mut start_x, mut start_y, first) = loop {
            let Some(Pixel(point, color)) = pixels.next() else {
                return Ok(());
            };
            if let Some((x, y)) = visible(point, dimensions) {
                break (x, y, color);
            }
        };
        let mut buffer = [first; RUN];
        let mut length = 1_u16;
        for Pixel(point, color) in pixels {
            let Some((x, y)) = visible(point, dimensions) else {
                continue;
            };
            if length > 0 && (usize::from(length) == RUN || y != start_y || x != start_x + length) {
                self.write_run(start_x, start_y, length, &buffer)?;
                length = 0;
            }
            if length == 0 {
                start_x = x;
                start_y = y;
            }
            buffer[usize::from(length)] = color;
            length += 1;
        }
        if length > 0 {
            self.write_run(start_x, start_y, length, &buffer)?;
        }
        Ok(())
    }

    fn fill_contiguous<I>(&mut self, area: &Rectangle, colors: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = D::Color>,
    {
        let dimensions = self.display.dimensions();
        if clip(*area, dimensions).is_none() {
            return Ok(());
        }
        self.draw_iter(contiguous_pixels(*area, colors))
    }

    fn fill_solid(&mut self, area: &Rectangle, color: D::Color) -> Result<(), Self::Error> {
        let Some(rect) = clip(*area, self.display.dimensions()) else {
            return Ok(());
        };
        let buffer = [color; RUN];
        for y in rect.y..rect.y + rect.height {
            let mut x = rect.x;
            let end = rect.x + rect.width;
            while x < end {
                let width = (end - x).min(u16::try_from(RUN).unwrap_or(u16::MAX));
                self.write_run(x, y, width, &buffer)?;
                x += width;
            }
        }
        Ok(())
    }
}
