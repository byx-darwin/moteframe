// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! The same renderer preserves native colors, bounds and failure semantics.
#![cfg(feature = "damage")]
use core::fmt::Debug;
use moteframe_core::{Dimensions, Display, Rect};
use moteframe_ui::{
    CooperativeError, DamageError, DirtyRegion, RefreshError, StreamTarget, Tile,
    damage_cooperative,
    graphics::{
        pixelcolor::{BinaryColor, Gray8, Rgb565, Rgb888, raw::RawU8},
        prelude::*,
        primitives::Rectangle,
    },
    repaint, repaint_cooperative,
};

#[derive(Debug)]
struct Screen<C> {
    writes: Vec<(Rect, Vec<C>)>,
    attempts: usize,
    fail_at: Option<usize>,
}
impl<C> Screen<C> {
    fn new() -> Self {
        Self {
            writes: Vec::new(),
            attempts: 0,
            fail_at: None,
        }
    }
}
impl<C: Copy> Display for Screen<C> {
    type Color = C;
    type Error = u8;
    fn dimensions(&self) -> Dimensions {
        Dimensions::new(40, 2)
    }
    fn write(&mut self, area: Rect, pixels: &[C]) -> Result<(), u8> {
        assert!(area.fits(self.dimensions()));
        assert_eq!(usize::try_from(area.area()), Ok(pixels.len()));
        self.attempts += 1;
        if self.fail_at == Some(self.attempts) {
            return Err(7);
        }
        self.writes.push((area, pixels.to_vec()));
        Ok(())
    }
}

fn stream<C: PixelColor + Debug>(background: C, first: C, second: C) -> Result<(), u8> {
    let mut screen = Screen::new();
    let mut target = StreamTarget::new(&mut screen);
    target.draw_iter(core::iter::empty())?;
    target.draw_iter([Pixel(Point::new(-1, 0), background)])?;
    target.draw_iter((0..35).map(|x| Pixel(Point::new(x, 0), first)))?;
    target.draw_iter([
        Pixel(Point::new(2, 1), first),
        Pixel(Point::new(3, 1), second),
        Pixel(Point::new(-1, 1), background),
        Pixel(Point::new(2, 1), second),
    ])?;
    assert_eq!(
        screen.writes,
        vec![
            (Rect::new(0, 0, 32, 1), vec![first; 32]),
            (Rect::new(32, 0, 3, 1), vec![first; 3]),
            (Rect::new(2, 1, 2, 1), vec![first, second]),
            (Rect::new(2, 1, 1, 1), vec![second]),
        ]
    );
    Ok(())
}

fn clipped<C: PixelColor + Debug>(background: C, first: C, second: C) -> Result<(), u8> {
    let mut screen = Screen::new();
    let mut target = StreamTarget::new(&mut screen);
    target.fill_contiguous(
        &Rectangle::new(Point::new(-1, 0), Size::new(4, 1)),
        [background, first, second],
    )?;
    target.fill_solid(&Rectangle::new(Point::new(39, 1), Size::new(5, 2)), second)?;
    assert_eq!(
        screen.writes,
        vec![
            (Rect::new(0, 0, 2, 1), vec![first, second]),
            (Rect::new(39, 1, 1, 1), vec![second]),
        ]
    );
    Ok(())
}

fn refreshed<C: PixelColor + Debug>(background: C, first: C, second: C) -> Result<(), ()> {
    let mut screen = Screen::new();
    let mut dirty = DirtyRegion::new(screen.dimensions());
    let area = Rectangle::new(Point::new(1, 0), Size::new(3, 2));
    dirty.invalidate(area);
    let mut services = 0;
    repaint_cooperative(
        &mut screen,
        &mut dirty,
        &mut [background; 2],
        background,
        |tile| tile.fill_solid(&area, first),
        || {
            services += 1;
            Ok::<(), ()>(())
        },
    )
    .map_err(|_| ())?;
    assert_eq!(services, 4);
    assert_eq!(dirty.pending(), None);
    assert_eq!(
        screen.writes,
        vec![
            (Rect::new(1, 0, 2, 1), vec![first; 2]),
            (Rect::new(3, 0, 1, 1), vec![first]),
            (Rect::new(1, 1, 2, 1), vec![first; 2]),
            (Rect::new(3, 1, 1, 1), vec![first]),
        ]
    );
    let mut calls = 0;
    let changed = damage_cooperative(
        Dimensions::new(4, 2),
        &mut [background; 8],
        background,
        |tile: &mut Tile<'_, C>| {
            tile.fill_solid(&Rectangle::new(Point::new(1, 1), Size::new(2, 1)), first)
        },
        |tile| tile.fill_solid(&Rectangle::new(Point::new(1, 1), Size::new(2, 1)), second),
        || {
            calls += 1;
            Ok::<(), ()>(())
        },
    );
    assert_eq!(changed, Ok(Some(Rect::new(1, 1, 2, 1))));
    assert_eq!(calls, 2);
    Ok(())
}

fn failures<C: PixelColor + Debug>(background: C, first: C) {
    let mut screen = Screen::new();
    screen.fail_at = Some(2);
    let mut dirty = DirtyRegion::new(screen.dimensions());
    dirty.invalidate(Rectangle::new(Point::zero(), Size::new(3, 1)));
    let pending = dirty.pending();
    let mut services = 0;
    let result = repaint_cooperative(
        &mut screen,
        &mut dirty,
        &mut [background; 2],
        background,
        |tile| tile.fill_solid(&Rectangle::new(Point::zero(), Size::new(3, 1)), first),
        || {
            services += 1;
            Ok::<(), ()>(())
        },
    );
    assert_eq!(
        result,
        Err(CooperativeError::Refresh(RefreshError::Display(7)))
    );
    assert_eq!(dirty.pending(), pending);
    assert_eq!(screen.attempts, 2);
    assert_eq!(services, 1);
    assert_eq!(screen.writes, vec![(Rect::new(0, 0, 2, 1), vec![first; 2])]);
    screen.fail_at = None;
    assert_eq!(
        repaint_cooperative(
            &mut screen,
            &mut dirty,
            &mut [background; 2],
            background,
            |_| Ok::<(), ()>(()),
            || Err(9)
        ),
        Err(CooperativeError::Service(9))
    );
    assert_eq!(dirty.pending(), pending);
    let attempts = screen.attempts;
    assert_eq!(
        repaint(
            &mut screen,
            &mut dirty,
            &mut [background; 2],
            background,
            |_| Err(3)
        ),
        Err(RefreshError::Scene(3))
    );
    assert_eq!(screen.attempts, attempts);
    assert_eq!(dirty.pending(), pending);
    assert_eq!(
        repaint(
            &mut screen,
            &mut dirty,
            &mut [],
            background,
            |_| Ok::<(), ()>(())
        ),
        Err(RefreshError::EmptyScratch)
    );
    assert_eq!(
        damage_cooperative(
            Dimensions::new(4, 2),
            &mut [background; 7],
            background,
            |_| Ok::<(), ()>(()),
            |_| Ok::<(), ()>(()),
            || Ok::<(), ()>(())
        ),
        Err(DamageError::ShortScratch)
    );
    assert!(
        repaint(
            &mut screen,
            &mut dirty,
            &mut [background; 2],
            background,
            |_| Ok::<(), ()>(())
        )
        .is_ok()
    );
    assert_eq!(dirty.pending(), None);
}

// This color deliberately has neither Default nor IntoStorage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Ink(u8);
impl PixelColor for Ink {
    type Raw = RawU8;
}

macro_rules! color_cases {
    ($name:ident, $background:expr, $first:expr, $second:expr) => {
        mod $name {
            use super::*;
            #[test]
            fn preserves_stream_colors() -> Result<(), u8> {
                stream($background, $first, $second)
            }
            #[test]
            fn preserves_clipped_colors() -> Result<(), u8> {
                clipped($background, $first, $second)
            }
            #[test]
            fn refreshes_and_compares_native_colors() -> Result<(), ()> {
                refreshed($background, $first, $second)
            }
            #[test]
            fn retains_dirty_state_on_failures() {
                failures($background, $first);
            }
        }
    };
}
color_cases!(
    rgb565,
    Rgb565::BLACK,
    Rgb565::new(1, 2, 3),
    Rgb565::new(1, 2, 4)
);
color_cases!(
    rgb888,
    Rgb888::BLACK,
    Rgb888::new(1, 2, 3),
    Rgb888::new(1, 2, 4)
);
color_cases!(gray8, Gray8::new(0), Gray8::new(1), Gray8::new(2));
color_cases!(binary, BinaryColor::Off, BinaryColor::On, BinaryColor::Off);
color_cases!(custom, Ink(0), Ink(1), Ink(2));
