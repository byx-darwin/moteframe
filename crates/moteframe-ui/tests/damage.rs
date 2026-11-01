// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Exact damage bounds and cooperative scheduling, independent of a watch skin.
use moteframe_core::{Dimensions, Rect, Rgb565 as Packed};
use moteframe_ui::{
    damage_cooperative,
    graphics::{pixelcolor::Rgb565, prelude::*, primitives::Rectangle},
};
#[test]
fn test_should_find_exact_damage_without_a_framebuffer() {
    let mut scratch = [Packed(0); 16];
    let mut services = 0;
    let result = damage_cooperative(
        Dimensions::new(8, 6),
        &mut scratch,
        Rgb565::new(0, 0, 0),
        |_: &mut moteframe_ui::Tile<'_>| Ok::<(), ()>(()),
        |t| {
            t.fill_solid(
                &Rectangle::new(Point::new(2, 1), Size::new(3, 4)),
                Rgb565::new(31, 0, 0),
            )
            .map_err(|_| ())
        },
        || {
            services += 1;
            Ok::<(), ()>(())
        },
    );
    assert_eq!(result, Ok(Some(Rect::new(2, 1, 3, 4))));
    assert_eq!(services, 6);
}
#[test]
fn test_should_reject_short_scratch_and_stop_on_service_failure() {
    let mut scratch = [Packed(0); 15];
    let mut calls = 0;
    assert!(
        damage_cooperative(
            Dimensions::new(8, 6),
            &mut scratch,
            Rgb565::new(0, 0, 0),
            |_| {
                calls += 1;
                Ok::<(), ()>(())
            },
            |_| Ok::<(), ()>(()),
            || Ok::<(), ()>(())
        )
        .is_err()
    );
    assert_eq!(calls, 0);
    let mut scratch = [Packed(0); 16];
    let mut services = 0;
    let r = damage_cooperative(
        Dimensions::new(8, 6),
        &mut scratch,
        Rgb565::new(0, 0, 0),
        |_| Ok::<(), ()>(()),
        |_| Ok::<(), ()>(()),
        || {
            services += 1;
            Err::<(), _>(7)
        },
    );
    assert!(r.is_err());
    assert_eq!(services, 1);
}
#[test]
fn test_should_report_no_damage_for_identical_overwriting_scenes() {
    let mut scratch = [Packed(0); 16];
    let scene = |t: &mut moteframe_ui::Tile<'_>| {
        t.fill_solid(
            &Rectangle::new(Point::new(-3, -2), Size::new(8, 7)),
            Rgb565::new(0, 63, 0),
        )
        .map_err(|_| ())
    };
    assert_eq!(
        damage_cooperative(
            Dimensions::new(8, 6),
            &mut scratch,
            Rgb565::new(0, 0, 0),
            scene,
            scene,
            || Ok::<(), ()>(())
        ),
        Ok(None)
    );
}

#[test]
fn test_should_include_erased_pixels_when_content_shrinks() {
    let mut scratch = [Packed(0); 16];
    let old = |t: &mut moteframe_ui::Tile<'_>| {
        t.fill_solid(
            &Rectangle::new(Point::new(1, 2), Size::new(6, 2)),
            Rgb565::new(31, 0, 0),
        )
    };
    let new = |t: &mut moteframe_ui::Tile<'_>| {
        t.fill_solid(
            &Rectangle::new(Point::new(1, 2), Size::new(2, 2)),
            Rgb565::new(31, 0, 0),
        )
    };
    assert_eq!(
        damage_cooperative(
            Dimensions::new(8, 6),
            &mut scratch,
            Rgb565::new(0, 0, 0),
            old,
            new,
            || Ok::<(), ()>(())
        ),
        Ok(Some(Rect::new(3, 2, 4, 2)))
    );
}
#[test]
fn test_should_stop_before_service_on_scene_failure_and_ignore_empty_dimensions() {
    let mut scratch = [Packed(0); 16];
    let mut calls = 0;
    assert_eq!(
        damage_cooperative(
            Dimensions::new(8, 6),
            &mut scratch,
            Rgb565::new(0, 0, 0),
            |_| Err(7),
            |_| Ok(()),
            || {
                calls += 1;
                Ok::<(), ()>(())
            }
        ),
        Err(moteframe_ui::DamageError::Scene(7))
    );
    assert_eq!(calls, 0);
    assert_eq!(
        damage_cooperative(
            Dimensions::new(0, 6),
            &mut [],
            Rgb565::new(0, 0, 0),
            |_| Err::<(), _>(7),
            |_| Ok(()),
            || Err::<(), _>(8)
        ),
        Ok(None)
    );
}
