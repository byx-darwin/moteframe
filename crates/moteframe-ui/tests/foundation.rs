// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Geometry, layout and stream adapter contracts.
use graphics::{pixelcolor::Rgb565, prelude::*, primitives::Rectangle};
use moteframe_core::{Dimensions, Display, Rect, Rgb565 as Packed};
use moteframe_ui::{Axis, LayoutError, Stack, StreamTarget, clip, graphics};

#[derive(Debug)]
struct Screen {
    size: Dimensions,
    writes: Vec<(Rect, Vec<Packed>)>,
    fail_at: Option<usize>,
}
impl Screen {
    fn new(width: u16, height: u16) -> Self {
        Self {
            size: Dimensions::new(width, height),
            writes: Vec::new(),
            fail_at: None,
        }
    }
}
impl Display for Screen {
    type Error = &'static str;
    fn dimensions(&self) -> Dimensions {
        self.size
    }
    fn write(&mut self, rect: Rect, pixels: &[Packed]) -> Result<(), Self::Error> {
        assert!(rect.fits(self.size));
        assert_eq!(usize::try_from(rect.area()), Ok(pixels.len()));
        if self.fail_at == Some(self.writes.len()) {
            return Err("transport");
        }
        self.writes.push((rect, pixels.to_vec()));
        Ok(())
    }
}

#[test]
fn test_should_clip_negative_and_extreme_rectangles_without_overflow() {
    let size = Dimensions::new(5, 4);
    assert_eq!(
        clip(Rectangle::new(Point::new(-2, -1), Size::new(4, 3)), size),
        Some(Rect::new(0, 0, 2, 2))
    );
    assert_eq!(
        clip(
            Rectangle::new(
                Point::new(i32::MIN, i32::MIN),
                Size::new(u32::MAX, u32::MAX)
            ),
            size
        ),
        Some(Rect::new(0, 0, 5, 4))
    );
    assert_eq!(
        clip(
            Rectangle::new(Point::new(i32::MAX, 0), Size::new(u32::MAX, 1)),
            size
        ),
        None
    );
    assert_eq!(
        clip(Rectangle::new(Point::zero(), Size::new(0, 2)), size),
        None
    );
    assert_eq!(
        clip(Rectangle::new(Point::new(5, 0), Size::new(1, 1)), size),
        None
    );
}

#[test]
fn test_should_layout_with_gap_and_keep_position_after_exhaustion() -> Result<(), LayoutError> {
    let mut stack = Stack::new(Rect::new(2, 3, 10, 4), Axis::Horizontal, 1)?;
    assert_eq!(stack.take(4)?, Rect::new(2, 3, 4, 4));
    assert_eq!(stack.take(6), Err(LayoutError::NoSpace));
    assert_eq!(stack.take(0), Err(LayoutError::ZeroExtent));
    assert_eq!(stack.take(5)?, Rect::new(7, 3, 5, 4));
    assert_eq!(stack.take(1), Err(LayoutError::NoSpace));
    let mut column = Stack::new(Rect::new(1, 2, 3, 9), Axis::Vertical, 2)?;
    assert_eq!(column.take(2)?, Rect::new(1, 2, 3, 2));
    assert_eq!(column.take(5)?, Rect::new(1, 6, 3, 5));
    assert!(Stack::new(Rect::new(u16::MAX, 0, 1, 1), Axis::Vertical, 0).is_err());
    assert!(Stack::new(Rect::new(0, 0, 0, 1), Axis::Vertical, 0).is_err());
    Ok(())
}

#[test]
fn test_should_coalesce_runs_preserve_overwrites_and_clip_pixels() -> Result<(), &'static str> {
    let mut screen = Screen::new(40, 2);
    let mut target = StreamTarget::new(&mut screen);
    target.draw_iter((0..35).map(|x| Pixel(Point::new(x, 0), Rgb565::RED)))?;
    target.draw_iter([
        Pixel(Point::new(-1, 0), Rgb565::BLUE),
        Pixel(Point::new(2, 1), Rgb565::GREEN),
        Pixel(Point::new(3, 1), Rgb565::BLUE),
        Pixel(Point::new(2, 1), Rgb565::WHITE),
        Pixel(Point::new(40, 0), Rgb565::WHITE),
    ])?;
    assert_eq!(
        screen.writes,
        vec![
            (Rect::new(0, 0, 32, 1), vec![Packed(0xf800); 32]),
            (Rect::new(32, 0, 3, 1), vec![Packed(0xf800); 3]),
            (Rect::new(2, 1, 2, 1), vec![Packed(0x07e0), Packed(0x001f)]),
            (Rect::new(2, 1, 1, 1), vec![Packed(0xffff)]),
        ]
    );
    Ok(())
}

#[test]
fn test_should_preserve_source_colors_when_contiguous_area_is_clipped() -> Result<(), &'static str>
{
    let mut screen = Screen::new(2, 2);
    StreamTarget::new(&mut screen).fill_contiguous(
        &Rectangle::new(Point::new(-1, -1), Size::new(3, 3)),
        [
            Rgb565::BLACK,
            Rgb565::BLACK,
            Rgb565::BLACK,
            Rgb565::BLACK,
            Rgb565::RED,
            Rgb565::GREEN,
            Rgb565::BLACK,
            Rgb565::BLUE,
            Rgb565::WHITE,
        ],
    )?;
    assert_eq!(
        screen.writes,
        vec![
            (Rect::new(0, 0, 2, 1), vec![Packed(0xf800), Packed(0x07e0)]),
            (Rect::new(0, 1, 2, 1), vec![Packed(0x001f), Packed(0xffff)]),
        ]
    );
    Ok(())
}

#[test]
fn test_should_not_invent_pixels_for_short_or_empty_streams() -> Result<(), &'static str> {
    let mut screen = Screen::new(3, 1);
    let area = Rectangle::new(Point::zero(), Size::new(3, 1));
    let mut target = StreamTarget::new(&mut screen);
    target.fill_contiguous(&area, [])?;
    target.fill_contiguous(&area, [Rgb565::RED])?;
    assert_eq!(
        screen.writes,
        vec![(Rect::new(0, 0, 1, 1), vec![Packed(0xf800)])]
    );
    Ok(())
}

#[test]
fn test_should_fill_clipped_solid_rows_in_bounded_chunks() -> Result<(), &'static str> {
    let mut screen = Screen::new(33, 2);
    StreamTarget::new(&mut screen).fill_solid(
        &Rectangle::new(Point::new(-1, 1), Size::new(40, 2)),
        Rgb565::BLUE,
    )?;
    assert_eq!(
        screen.writes,
        vec![
            (Rect::new(0, 1, 32, 1), vec![Packed(0x001f); 32]),
            (Rect::new(32, 1, 1, 1), vec![Packed(0x001f)]),
        ]
    );
    Ok(())
}

#[test]
fn test_should_stop_stream_at_first_display_failure() {
    let mut screen = Screen::new(40, 1);
    screen.fail_at = Some(0);
    let result = StreamTarget::new(&mut screen)
        .draw_iter((0..40).map(|x| Pixel(Point::new(x, 0), Rgb565::RED)));
    assert_eq!(result, Err("transport"));
    assert!(screen.writes.is_empty());
}
