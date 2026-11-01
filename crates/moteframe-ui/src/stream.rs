// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

use embedded_graphics::{pixelcolor::Rgb565, prelude::*, primitives::Rectangle};
use moteframe_core::{Display, Rect, Rgb565 as Packed};

use crate::geometry::{clip, contiguous_pixels, visible};

const RUN: u16 = 32;

/// Immediate RGB565 drawing adapter with bounded horizontal-run buffering.
///
/// Each drawing call finishes its writes before returning. Arbitrarily ordered
/// pixels retain their overwrite order. There is no full-screen framebuffer.
/// Any implementation of the hardware-independent `Display` contract works.
#[derive(Debug)]
pub struct StreamTarget<'a, D> {
    display: &'a mut D,
}

impl<'a, D: Display> StreamTarget<'a, D> {
    /// Borrows a display that the caller has already prepared for writing.
    pub fn new(display: &'a mut D) -> Self {
        Self { display }
    }

    fn write_run(
        &mut self,
        x: u16,
        y: u16,
        width: u16,
        pixels: &[Packed; 32],
    ) -> Result<(), D::Error> {
        self.display
            .write(Rect::new(x, y, width, 1), &pixels[..usize::from(width)])
    }
}

impl<D: Display> OriginDimensions for StreamTarget<'_, D> {
    fn size(&self) -> Size {
        let dimensions = self.display.dimensions();
        Size::new(u32::from(dimensions.width), u32::from(dimensions.height))
    }
}

impl<D: Display> DrawTarget for StreamTarget<'_, D> {
    type Color = Rgb565;
    type Error = D::Error;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Rgb565>>,
    {
        let dimensions = self.display.dimensions();
        let mut buffer = [Packed(0); RUN as usize];
        let (mut start_x, mut start_y, mut length) = (0, 0, 0_u16);
        for Pixel(point, color) in pixels {
            let Some((x, y)) = visible(point, dimensions) else {
                continue;
            };
            if length > 0 && (length == RUN || y != start_y || x != start_x + length) {
                self.write_run(start_x, start_y, length, &buffer)?;
                length = 0;
            }
            if length == 0 {
                start_x = x;
                start_y = y;
            }
            buffer[usize::from(length)] = Packed(color.into_storage());
            length += 1;
        }
        if length > 0 {
            self.write_run(start_x, start_y, length, &buffer)?;
        }
        Ok(())
    }

    fn fill_contiguous<I>(&mut self, area: &Rectangle, colors: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Rgb565>,
    {
        let dimensions = self.display.dimensions();
        if clip(*area, dimensions).is_none() {
            return Ok(());
        }
        self.draw_iter(contiguous_pixels(*area, colors))
    }

    fn fill_solid(&mut self, area: &Rectangle, color: Rgb565) -> Result<(), Self::Error> {
        let Some(rect) = clip(*area, self.display.dimensions()) else {
            return Ok(());
        };
        let buffer = [Packed(color.into_storage()); RUN as usize];
        for y in rect.y..rect.y + rect.height {
            let mut x = rect.x;
            let end = rect.x + rect.width;
            while x < end {
                let width = (end - x).min(RUN);
                self.write_run(x, y, width, &buffer)?;
                x += width;
            }
        }
        Ok(())
    }
}
