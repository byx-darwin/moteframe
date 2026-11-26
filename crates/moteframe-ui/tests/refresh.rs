// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Partial redraw behavior using actual rasterization into caller storage.
#![cfg(feature = "refresh")]
use core::{cell::Cell, convert::Infallible};
use graphics::{
    geometry::Dimensions as _,
    image::ImageRaw,
    pixelcolor::Rgb565,
    prelude::*,
    primitives::{PrimitiveStyle, Rectangle},
};
use moteframe_core::{Dimensions, Display, Rect};
use moteframe_ui::graphics::pixelcolor::{
    Rgb565 as Packed,
    raw::{RawU8, RawU16},
};
use moteframe_ui::{DirtyRegion, RefreshError, graphics, repaint};

#[derive(Debug)]
struct Screen<C = Packed> {
    size: Dimensions,
    writes: Vec<(Rect, Vec<C>)>,
    fail_at: Option<usize>,
}
impl<C> Screen<C> {
    fn new(width: u16, height: u16) -> Self {
        Self {
            size: Dimensions::new(width, height),
            writes: Vec::new(),
            fail_at: None,
        }
    }
}
impl<C: Copy> Display for Screen<C> {
    type Color = C;
    type Error = &'static str;
    fn dimensions(&self) -> Dimensions {
        self.size
    }
    fn write(&mut self, rect: Rect, pixels: &[C]) -> Result<(), Self::Error> {
        assert!(rect.fits(self.size));
        assert_eq!(usize::try_from(rect.area()), Ok(pixels.len()));
        if self.fail_at == Some(self.writes.len()) {
            return Err("display");
        }
        self.writes.push((rect, pixels.to_vec()));
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CountedColor(u8);

thread_local! {
    static CONVERSIONS: Cell<u32> = const { Cell::new(0) };
}

impl PixelColor for CountedColor {
    type Raw = RawU8;
}

impl From<RawU8> for CountedColor {
    fn from(raw: RawU8) -> Self {
        CONVERSIONS.set(CONVERSIONS.get() + 1);
        Self(raw.into_inner())
    }
}

#[test]
fn raw_image_decodes_each_visible_pixel_once() -> Result<(), RefreshError<&'static str, Infallible>>
{
    let bytes: Vec<_> = (0..=u8::MAX).cycle().take(240 * 240).collect();
    let image = ImageRaw::<CountedColor>::new(&bytes, 240);
    for capacity in [8, 32] {
        let mut screen = Screen::new(240, 240);
        let mut dirty = DirtyRegion::new(screen.size);
        dirty.invalidate_all();
        CONVERSIONS.set(0);
        repaint(
            &mut screen,
            &mut dirty,
            &mut vec![CountedColor(0); capacity],
            CountedColor(0),
            |tile| tile.draw_image(&image, Point::zero()),
        )?;
        assert_eq!(CONVERSIONS.get(), 57_600, "capacity {capacity}");
        assert!(
            screen
                .writes
                .iter()
                .flat_map(|(_, colors)| colors.iter().map(|c| c.0))
                .eq(bytes.iter().copied())
        );
    }
    Ok(())
}

#[test]
fn raw_image_clips_source_and_preserves_surrounding_tile_pixels()
-> Result<(), RefreshError<&'static str, Infallible>> {
    let image = ImageRaw::<CountedColor>::new(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12], 4);
    let mut screen = Screen::new(8, 2);
    let mut dirty = DirtyRegion::new(screen.size);
    dirty.invalidate(Rectangle::new(Point::new(1, 1), Size::new(7, 1)));
    CONVERSIONS.set(0);
    repaint(
        &mut screen,
        &mut dirty,
        &mut [CountedColor(0); 4],
        CountedColor(99),
        |tile| {
            tile.draw_image(&image, Point::new(-1, -1))?;
            tile.draw_image(&image, Point::new(4, 1))?;
            tile.draw_image(&image, Point::new(i32::MAX, 0))
        },
    )?;
    assert_eq!(CONVERSIONS.get(), 6);
    assert_eq!(
        screen.writes[0].1.iter().map(|c| c.0).collect::<Vec<_>>(),
        [11, 12, 99, 1]
    );
    assert_eq!(
        screen.writes[1].1.iter().map(|c| c.0).collect::<Vec<_>>(),
        [2, 3, 4]
    );
    Ok(())
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
        &mut [Packed::from(RawU16::new(0xffff)); 2],
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
            (
                Rect::new(2, 3, 2, 1),
                vec![
                    Packed::from(RawU16::new(0)),
                    Packed::from(RawU16::new(0xf800))
                ]
            ),
            (Rect::new(4, 3, 1, 1), vec![Packed::from(RawU16::new(0))]),
            (
                Rect::new(2, 4, 2, 1),
                vec![
                    Packed::from(RawU16::new(0)),
                    Packed::from(RawU16::new(0xf800))
                ]
            ),
            (Rect::new(4, 4, 1, 1), vec![Packed::from(RawU16::new(0))]),
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
        &mut [Packed::from(RawU16::new(0)); 3],
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
            &mut [Packed::from(RawU16::new(0)); 3],
            Rgb565::GREEN,
            |_| Ok::<_, Infallible>(())
        ),
        Ok(())
    );
    assert_eq!(
        screen.writes[2],
        (
            Rect::new(0, 1, 3, 1),
            vec![Packed::from(RawU16::new(0x07e0)); 3]
        )
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
            &mut [Packed::from(RawU16::new(0)); 2],
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
            &mut [Packed::from(RawU16::new(0)); 2],
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
            &mut [Packed::from(RawU16::new(0)); 1],
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
            (
                Rect::new(1, 0, 1, 1),
                vec![Packed::from(RawU16::new(0x07e0))]
            ),
            (
                Rect::new(2, 0, 1, 1),
                vec![Packed::from(RawU16::new(0x001f))]
            ),
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
        &mut [Packed::from(RawU16::new(0)); 2],
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
        vec![(
            Rect::new(0, 0, 2, 1),
            vec![Packed::from(RawU16::new(0x07e0)); 2]
        )]
    );
}

// An indexed decoder can jump its input cursor before decoding a color.
struct IndexedColors<I, F>(I, F);

impl<I: Iterator, F: FnMut(I::Item) -> Packed> Iterator for IndexedColors<I, F> {
    type Item = Packed;

    fn next(&mut self) -> Option<Packed> {
        self.0.next().map(&mut self.1)
    }

    fn nth(&mut self, n: usize) -> Option<Packed> {
        self.0.nth(n).map(&mut self.1)
    }
}

#[test]
fn seekable_contiguous_image_decodes_each_visible_pixel_once()
-> Result<(), RefreshError<&'static str, Infallible>> {
    const WIDTH: u16 = 240;
    const PIXELS: u16 = WIDTH * WIDTH;
    let area = Rectangle::new(Point::zero(), Size::new(240, 240));
    for (capacity, expected_tiles) in [(8, 7_200), (32, 1_920)] {
        let mut screen = Screen::new(WIDTH, WIDTH);
        let mut dirty = DirtyRegion::new(screen.size);
        dirty.invalidate_all();
        let mut scratch = vec![Rgb565::BLACK; capacity];
        let mut decoded = 0;
        repaint(
            &mut screen,
            &mut dirty,
            &mut scratch,
            Rgb565::BLACK,
            |tile| {
                let before = decoded;
                let colors = IndexedColors(0..PIXELS, |value| {
                    decoded += 1;
                    Packed::from(RawU16::new(value))
                });
                tile.fill_contiguous(&area, colors)?;
                assert_eq!(decoded - before, tile.bounding_box().size.width);
                Ok(())
            },
        )?;
        assert_eq!(decoded, u32::from(PIXELS));
        assert_eq!(screen.writes.len(), expected_tiles);
        let mut expected = (0..PIXELS).map(|value| Packed::from(RawU16::new(value)));
        for (_, pixels) in screen.writes {
            for pixel in pixels {
                assert_eq!(Some(pixel), expected.next());
            }
        }
        assert_eq!(expected.next(), None);
    }
    Ok(())
}

#[test]
fn forward_only_contiguous_source_stops_at_tile_edge_or_end()
-> Result<(), RefreshError<&'static str, Infallible>> {
    let mut screen = Screen::new(4, 1);
    let mut dirty = DirtyRegion::new(screen.size);
    dirty.invalidate_all();
    let mut consumed = 0;
    repaint(
        &mut screen,
        &mut dirty,
        &mut [Rgb565::BLACK; 4],
        Rgb565::BLACK,
        |tile| {
            let area = Rectangle::new(Point::new(-1, -1), Size::new(8, 3));
            let colors = core::iter::from_fn(|| {
                consumed += 1;
                Some(Packed::from(RawU16::new(consumed)))
            });
            tile.fill_contiguous(&area, colors)?;
            tile.fill_contiguous(
                &Rectangle::new(Point::new(1, 0), Size::new(4, 1)),
                [Rgb565::WHITE],
            )?;
            tile.fill_contiguous(
                &Rectangle::new(Point::new(4, 0), Size::new(2, 1)),
                core::iter::from_fn(|| panic!("off-screen source accessed")),
            )
        },
    )?;
    assert_eq!(consumed, 13);
    assert_eq!(
        screen.writes[0].1,
        [
            Packed::from(RawU16::new(10)),
            Rgb565::WHITE,
            Packed::from(RawU16::new(12)),
            Packed::from(RawU16::new(13))
        ]
    );
    Ok(())
}

#[test]
#[cfg(target_pointer_width = "64")]
fn contiguous_image_seeks_to_extreme_signed_origin_without_overflow()
-> Result<(), RefreshError<&'static str, Infallible>> {
    let mut screen = Screen::new(2, 1);
    let mut dirty = DirtyRegion::new(screen.size);
    dirty.invalidate_all();
    let area = Rectangle::new(
        Point::new(i32::MIN, i32::MIN),
        Size::new(u32::MAX, u32::MAX),
    );
    repaint(
        &mut screen,
        &mut dirty,
        &mut [Rgb565::BLACK; 2],
        Rgb565::BLACK,
        |tile| {
            // The first visible pixel is exactly 2^63 positions into this virtual image.
            let source = IndexedColors(0..(1_u64 << 63) + 1, |index| {
                assert_eq!(index, 1 << 63);
                Rgb565::GREEN
            });
            tile.fill_contiguous(&area, source)
        },
    )?;
    assert_eq!(screen.writes[0].1, [Rgb565::GREEN, Rgb565::BLACK]);
    Ok(())
}
