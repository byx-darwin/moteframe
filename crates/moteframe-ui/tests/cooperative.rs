// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Display writes and fallible service calls stay ordered at tile boundaries.
use core::convert::Infallible;
use moteframe_core::{Dimensions, Display, Rect, Rgb565 as Packed};
use moteframe_ui::graphics::prelude::RgbColor;
use moteframe_ui::{
    CooperativeError, DirtyRegion, graphics::pixelcolor::Rgb565, repaint_cooperative,
};
use std::{cell::RefCell, rc::Rc};
#[derive(Debug)]
struct Screen(Rc<RefCell<Vec<&'static str>>>);
impl Display for Screen {
    type Error = Infallible;
    fn dimensions(&self) -> Dimensions {
        Dimensions::new(5, 2)
    }
    fn write(&mut self, rect: Rect, pixels: &[Packed]) -> Result<(), Infallible> {
        assert!(rect.area() <= 2);
        assert_eq!(rect.area() as usize, pixels.len());
        self.0.borrow_mut().push("write");
        Ok(())
    }
}
#[test]
fn test_should_service_after_every_tile_and_clear_dirty_only_on_complete_success() {
    let trace = Rc::new(RefCell::new(Vec::new()));
    let mut screen = Screen(trace.clone());
    let mut dirty = DirtyRegion::new(screen.dimensions());
    dirty.invalidate_all();
    let result = repaint_cooperative(
        &mut screen,
        &mut dirty,
        &mut [Packed(0); 2],
        Rgb565::BLACK,
        |_| Ok::<(), Infallible>(()),
        || {
            trace.borrow_mut().push("service");
            Ok::<(), Infallible>(())
        },
    );
    assert_eq!(result, Ok(()));
    assert!(dirty.pending().is_none());
    assert_eq!(trace.borrow().as_slice(), ["write", "service"].repeat(6));
}
#[test]
fn test_should_preserve_entire_dirty_area_on_service_failure_and_retry() {
    let trace = Rc::new(RefCell::new(Vec::new()));
    let mut screen = Screen(trace.clone());
    let mut dirty = DirtyRegion::new(screen.dimensions());
    dirty.invalidate_all();
    let original = dirty.pending();
    let result = repaint_cooperative(
        &mut screen,
        &mut dirty,
        &mut [Packed(0); 2],
        Rgb565::BLACK,
        |_| Ok::<(), Infallible>(()),
        || Err("ble"),
    );
    assert_eq!(result, Err(CooperativeError::Service("ble")));
    assert_eq!(dirty.pending(), original);
    assert_eq!(trace.borrow().as_slice(), ["write"]);
    assert!(
        repaint_cooperative(
            &mut screen,
            &mut dirty,
            &mut [Packed(0); 2],
            Rgb565::BLACK,
            |_| Ok::<(), Infallible>(()),
            || Ok::<(), Infallible>(())
        )
        .is_ok()
    );
    assert!(dirty.pending().is_none());
}
#[test]
fn test_should_skip_service_when_clean_or_invalid_before_first_write() {
    let trace = Rc::new(RefCell::new(Vec::new()));
    let mut screen = Screen(trace.clone());
    let mut dirty = DirtyRegion::new(screen.dimensions());
    assert!(
        repaint_cooperative(
            &mut screen,
            &mut dirty,
            &mut [],
            Rgb565::BLACK,
            |_| Ok::<(), Infallible>(()),
            || Err("unexpected")
        )
        .is_ok()
    );
    dirty.invalidate_all();
    assert!(matches!(
        repaint_cooperative(
            &mut screen,
            &mut dirty,
            &mut [],
            Rgb565::BLACK,
            |_| Ok::<(), Infallible>(()),
            || Err("unexpected")
        ),
        Err(CooperativeError::Refresh(
            moteframe_ui::RefreshError::EmptyScratch
        ))
    ));
    assert!(trace.borrow().is_empty());
}
