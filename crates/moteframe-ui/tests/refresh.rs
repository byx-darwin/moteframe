// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Partial redraw behavior using actual rasterization into caller storage.
use core::convert::Infallible;
use graphics::{
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
};
use moteframe_core::{Dimensions, Display, Rect, Rgb565 as Packed};
use moteframe_ui::{DirtyRegion, RefreshError, graphics, repaint};

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
            return Err("display");
        }
        self.writes.push((rect, pixels.to_vec()));
        Ok(())
    }
}

#[test]
fn test_should_union_only_visible_invalidations() {
    let mut dirty = DirtyRegion::new(Dimensions::new(8, 6));
    dirty.invalidate(Rectangle::new(Point::new(-2, -2), Size::new(4, 4)));
    dirty.invalidate(Rectangle::new(Point::new(6, 4), Size::new(9, 9)));
    assert_eq!(dirty.pending(), Some(Rect::new(0, 0, 8, 6)));
    let mut empty = DirtyRegion::new(Dimensions::new(8, 6));
    empty.invalidate(Rectangle::new(Point::new(8, 0), Size::new(2, 2)));
    assert_eq!(empty.pending(), None);
    empty.invalidate_all();
    assert_eq!(empty.pending(), Some(Rect::new(0, 0, 8, 6)));
}

#[test]
fn test_should_repaint_only_dirty_pixels_with_scratch_narrower_than_region() {
    let mut screen = Screen::new(8, 6);
    let mut dirty = DirtyRegion::new(screen.size);
    dirty.invalidate(Rectangle::new(Point::new(2, 3), Size::new(3, 2)));
    let result = repaint(
        &mut screen,
        &mut dirty,
        &mut [Packed(0xffff); 2],
        Rgb565::BLACK,
        |tile| {
            Rectangle::new(Point::new(3, 3), Size::new(1, 2))
                .into_styled(PrimitiveStyle::with_fill(Rgb565::RED))
                .draw(tile)
        },
    );
    assert_eq!(result, Ok(()));
    assert_eq!(
        screen.writes,
        vec![
            (Rect::new(2, 3, 2, 1), vec![Packed(0), Packed(0xf800)]),
            (Rect::new(4, 3, 1, 1), vec![Packed(0)]),
            (Rect::new(2, 4, 2, 1), vec![Packed(0), Packed(0xf800)]),
            (Rect::new(4, 4, 1, 1), vec![Packed(0)]),
        ]
    );
    assert_eq!(dirty.pending(), None);
}

#[test]
fn test_should_keep_entire_dirty_region_on_partial_display_failure_and_retry() {
    let mut screen = Screen::new(3, 2);
    screen.fail_at = Some(1);
    let mut dirty = DirtyRegion::new(screen.size);
    dirty.invalidate_all();
    let result = repaint(
        &mut screen,
        &mut dirty,
        &mut [Packed(0); 3],
        Rgb565::GREEN,
        |_| Ok::<_, Infallible>(()),
    );
    assert_eq!(result, Err(RefreshError::Display("display")));
    assert_eq!(screen.writes.len(), 1);
    assert_eq!(dirty.pending(), Some(Rect::new(0, 0, 3, 2)));
    screen.fail_at = None;
    assert_eq!(
        repaint(
            &mut screen,
            &mut dirty,
            &mut [Packed(0); 3],
            Rgb565::GREEN,
            |_| Ok::<_, Infallible>(())
        ),
        Ok(())
    );
    assert_eq!(
        screen.writes[2],
        (Rect::new(0, 1, 3, 1), vec![Packed(0x07e0); 3])
    );
    assert_eq!(dirty.pending(), None);
}

#[test]
fn test_should_keep_dirty_region_on_scene_failure() {
    let mut screen = Screen::new(2, 1);
    let mut dirty = DirtyRegion::new(screen.size);
    dirty.invalidate_all();
    assert_eq!(
        repaint(
            &mut screen,
            &mut dirty,
            &mut [Packed(0); 2],
            Rgb565::BLACK,
            |_| Err("scene")
        ),
        Err(RefreshError::Scene("scene"))
    );
    assert!(screen.writes.is_empty());
    assert_eq!(dirty.pending(), Some(Rect::new(0, 0, 2, 1)));
}

#[test]
fn test_should_reject_empty_scratch_and_geometry_changes_before_drawing() {
    let mut screen = Screen::new(2, 1);
    let mut dirty = DirtyRegion::new(screen.size);
    dirty.invalidate_all();
    let result = repaint(&mut screen, &mut dirty, &mut [], Rgb565::BLACK, |_| {
        Ok::<_, Infallible>(())
    });
    assert_eq!(result, Err(RefreshError::EmptyScratch));
    screen.size = Dimensions::new(3, 1);
    assert_eq!(
        repaint(
            &mut screen,
            &mut dirty,
            &mut [Packed(0); 2],
            Rgb565::BLACK,
            |_| Ok::<_, Infallible>(())
        ),
        Err(RefreshError::DimensionsChanged)
    );
    assert!(screen.writes.is_empty());
    assert_eq!(dirty.pending(), Some(Rect::new(0, 0, 2, 1)));
}

#[test]
fn test_should_skip_scene_and_io_when_nothing_is_dirty() {
    let mut screen = Screen::new(2, 1);
    let mut dirty = DirtyRegion::new(screen.size);
    let mut called = false;
    let result = repaint(&mut screen, &mut dirty, &mut [], Rgb565::BLACK, |_| {
        called = true;
        Ok::<_, Infallible>(())
    });
    assert_eq!(result, Ok(()));
    assert!(!called);
    assert!(screen.writes.is_empty());
}

#[test]
fn test_should_preserve_contiguous_color_positions_within_global_tile_clip() {
    let mut screen = Screen::new(4, 1);
    let mut dirty = DirtyRegion::new(screen.size);
    dirty.invalidate(Rectangle::new(Point::new(1, 0), Size::new(2, 1)));
    assert_eq!(
        repaint(
            &mut screen,
            &mut dirty,
            &mut [Packed(0); 1],
            Rgb565::BLACK,
            |tile| {
                tile.fill_contiguous(
                    &Rectangle::new(Point::new(-1, 0), Size::new(5, 1)),
                    [
                        Rgb565::BLACK,
                        Rgb565::RED,
                        Rgb565::GREEN,
                        Rgb565::BLUE,
                        Rgb565::WHITE,
                    ],
                )
            }
        ),
        Ok(())
    );
    assert_eq!(
        screen.writes,
        vec![
            (Rect::new(1, 0, 1, 1), vec![Packed(0x07e0)]),
            (Rect::new(2, 0, 1, 1), vec![Packed(0x001f)]),
        ]
    );
}

#[test]
fn test_should_clip_extreme_tile_fill_inputs_without_panicking() {
    let mut screen = Screen::new(2, 1);
    let mut dirty = DirtyRegion::new(screen.size);
    dirty.invalidate_all();
    let result = repaint(
        &mut screen,
        &mut dirty,
        &mut [Packed(0); 2],
        Rgb565::BLACK,
        |tile| {
            tile.fill_contiguous(
                &Rectangle::new(Point::new(i32::MAX, 0), Size::new(u32::MAX, 1)),
                [Rgb565::RED],
            )?;
            tile.fill_solid(
                &Rectangle::new(Point::new(i32::MIN, 0), Size::new(u32::MAX, 1)),
                Rgb565::GREEN,
            )
        },
    );
    assert_eq!(result, Ok(()));
    assert_eq!(
        screen.writes,
        vec![(Rect::new(0, 0, 2, 1), vec![Packed(0x07e0); 2])]
    );
}
