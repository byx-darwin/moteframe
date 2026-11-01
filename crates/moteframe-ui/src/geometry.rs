// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

use embedded_graphics::{
    geometry::Point, pixelcolor::Rgb565, prelude::Pixel, primitives::Rectangle,
};
use moteframe_core::{Dimensions, Rect};

/// Intersects signed graphics geometry with unsigned display bounds.
///
/// Returns `None` for an empty intersection, including zero-sized displays.
/// Wide arithmetic supports all `i32` origins and `u32` extents without overflow.
#[must_use]
pub fn clip(area: Rectangle, dimensions: Dimensions) -> Option<Rect> {
    let left = i64::from(area.top_left.x).max(0);
    let top = i64::from(area.top_left.y).max(0);
    let right =
        (i64::from(area.top_left.x) + i64::from(area.size.width)).min(i64::from(dimensions.width));
    let bottom = (i64::from(area.top_left.y) + i64::from(area.size.height))
        .min(i64::from(dimensions.height));
    if left >= right || top >= bottom {
        return None;
    }
    // The nonempty intersection is bounded by u16 display dimensions.
    Some(Rect::new(
        u16::try_from(left).ok()?,
        u16::try_from(top).ok()?,
        u16::try_from(right - left).ok()?,
        u16::try_from(bottom - top).ok()?,
    ))
}

pub(crate) fn visible(point: Point, dimensions: Dimensions) -> Option<(u16, u16)> {
    let x = u16::try_from(point.x).ok()?;
    let y = u16::try_from(point.y).ok()?;
    (x < dimensions.width && y < dimensions.height).then_some((x, y))
}

pub(crate) fn contiguous_pixels<I>(
    area: Rectangle,
    colors: I,
) -> impl Iterator<Item = Pixel<Rgb565>>
where
    I: IntoIterator<Item = Rgb565>,
{
    let width = u64::from(area.size.width);
    let count = width * u64::from(area.size.height);
    (0..count).zip(colors).filter_map(move |(index, color)| {
        // The closure is never called for zero-width areas (count is zero).
        let x = i64::from(area.top_left.x) + i64::try_from(index % width).ok()?;
        let y = i64::from(area.top_left.y) + i64::try_from(index / width).ok()?;
        Some(Pixel(
            Point::new(i32::try_from(x).ok()?, i32::try_from(y).ok()?),
            color,
        ))
    })
}
