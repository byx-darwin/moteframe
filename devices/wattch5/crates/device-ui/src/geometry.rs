use device_core::Dimensions;
use embedded_graphics::{
    geometry::Point, pixelcolor::Rgb565, prelude::Pixel, primitives::Rectangle,
};

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
