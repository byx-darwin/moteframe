// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Stream chunking uses the compile-time capacity and preserves pixel order.
use moteframe_core::{Dimensions, Display, Rect};
use moteframe_ui::{
    StreamTarget,
    graphics::{pixelcolor::Gray8, prelude::*},
};
#[derive(Debug, Default)]
struct Screen {
    writes: Vec<(Rect, Vec<Gray8>)>,
}
impl Display for Screen {
    type Color = Gray8;
    type Error = ();
    fn dimensions(&self) -> Dimensions {
        Dimensions::new(7, 1)
    }
    fn write(&mut self, area: Rect, pixels: &[Gray8]) -> Result<(), ()> {
        self.writes.push((area, pixels.to_vec()));
        Ok(())
    }
}
#[test]
fn explicit_small_and_large_runs_preserve_the_same_image() -> Result<(), ()> {
    let mut a = Screen::default();
    let mut b = Screen::default();
    StreamTarget::<_, 2>::with_run(&mut a)
        .draw_iter((0..7).map(|x| Pixel(Point::new(x, 0), Gray8::new(3))))?;
    StreamTarget::<_, 65535>::with_run(&mut b).fill_solid(
        &moteframe_ui::graphics::primitives::Rectangle::new(Point::zero(), Size::new(7, 1)),
        Gray8::new(3),
    )?;
    assert_eq!(
        a.writes
            .iter()
            .map(|(rect, _)| rect.width)
            .collect::<Vec<_>>(),
        vec![2, 2, 2, 1]
    );
    assert_eq!(b.writes[0].0, Rect::new(0, 0, 7, 1));
    assert_eq!(
        a.writes
            .into_iter()
            .flat_map(|(_, pixels)| pixels)
            .collect::<Vec<_>>(),
        b.writes
            .into_iter()
            .flat_map(|(_, pixels)| pixels)
            .collect::<Vec<_>>()
    );
    Ok(())
}
