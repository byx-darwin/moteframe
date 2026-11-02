// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

use core::{convert::Infallible, num::NonZeroU16};
use moteframe_core::{Dimensions, Display, Rect};
use moteframe_ui::{
    self as ui,
    graphics::pixelcolor::{Rgb565, raw::RawU16},
    graphics::prelude::*,
};
use moteframe_ui_low_memory::{
    App, FrameReport, RenderError, UI_PIXELS, draw_scene, render_once, resources, stream_row,
};

const DIMENSIONS: Dimensions = Dimensions::new(64, 8);
const FULL: Rect = Rect::new(0, 0, 64, 8);

#[derive(Debug, PartialEq, Eq)]
struct TransportFailure(u8);

#[derive(Debug)]
struct Screen {
    pixels: [Rgb565; 512],
    writes: usize,
    accepted: usize,
    fail_at: Option<usize>,
    dimensions: Dimensions,
}
impl Screen {
    fn new() -> Self {
        Self {
            pixels: [Rgb565::BLACK; 512],
            writes: 0,
            accepted: 0,
            fail_at: None,
            dimensions: DIMENSIONS,
        }
    }
    fn reset_counts(&mut self) {
        self.writes = 0;
        self.accepted = 0;
    }
}
impl Display for Screen {
    type Color = Rgb565;
    type Error = TransportFailure;
    fn dimensions(&self) -> Dimensions {
        self.dimensions
    }
    fn write(&mut self, area: Rect, pixels: &[Rgb565]) -> Result<(), Self::Error> {
        assert!(area.fits(self.dimensions));
        assert_eq!(area.height, 1);
        assert_eq!(usize::from(area.width), pixels.len());
        assert!(pixels.len() <= UI_PIXELS);
        self.writes += 1;
        if self.fail_at == Some(self.writes) {
            self.fail_at = None;
            return Err(TransportFailure(73));
        }
        let start = usize::from(area.y) * 64 + usize::from(area.x);
        self.pixels[start..start + pixels.len()].copy_from_slice(pixels);
        self.accepted += pixels.len();
        Ok(())
    }
}

// Independently specify the output in raw RGB565 channel positions. Calling the
// sample's scene/color helper here would miss a shared rendering regression.
fn expected(seed: u32) -> [Rgb565; 512] {
    core::array::from_fn(|i| {
        let x = (i % 64) as u32;
        let y = (i / 64) as u32;
        let raw = (((x ^ seed) % 32) << 11) | (((3 * y + seed) % 64) << 5) | ((x + y + seed) % 32);
        RawU16::new(raw as u16).into()
    })
}
fn fixed(pixels: usize) -> ui::RefreshSettings {
    ui::RefreshSettings::new().with_tile_pixels(ui::TilePixels::Fixed(
        NonZeroU16::new(pixels as u16).unwrap(),
    ))
}
fn draw(
    session: &mut ui::RefreshSession<'_, Rgb565>,
    screen: &mut Screen,
    seed: u32,
    service: impl FnMut(&mut ui::DeferredInvalidation) -> Result<(), Infallible>,
) -> Result<ui::DrawOutcome, ui::SessionError<TransportFailure, Infallible, Infallible>> {
    session.draw(
        ui::DisplayBinding::new(screen, 1),
        resources(),
        |tile, context| draw_scene(seed, tile, context),
        service,
    )
}

#[test]
fn full_image_uses_actual_selected_array_and_bounded_writes() {
    let mut hashes = Vec::new();
    for seed in [0, 1, 19, 63] {
        let mut app = App::new(DIMENSIONS).unwrap();
        let actual_bytes = core::mem::size_of_val(app.workspace());
        assert_eq!(
            app.workspace().len(),
            if cfg!(feature = "ui-low-memory") {
                8
            } else {
                32
            }
        );
        assert_eq!(
            actual_bytes,
            if cfg!(feature = "ui-low-memory") {
                16
            } else {
                64
            }
        );
        let mut session = app.session(ui::RefreshSettings::new()).unwrap();
        let mut screen = Screen::new();
        let mut services = 0;
        assert_eq!(
            draw(&mut session, &mut screen, seed, |_| {
                services += 1;
                Ok(())
            }),
            Ok(ui::DrawOutcome::Drawn)
        );
        assert_eq!(screen.pixels, expected(seed));
        assert_eq!(screen.accepted, 512);
        assert_eq!(
            screen.writes,
            if cfg!(feature = "ui-low-memory") {
                64
            } else {
                16
            }
        );
        assert_eq!(services, screen.writes);
        assert_eq!(session.pending(), None);
        hashes.push(screen.pixels.iter().fold(1_u32, |sum, p| {
            sum.wrapping_mul(31)
                .wrapping_add(u32::from(p.into_storage()))
        }));
    }
    // Checker compares this observable image evidence across separate builds.
    println!("image_hashes={hashes:?}");
}

#[test]
fn stream_row_uses_same_run_capacity_and_expected_pixels() {
    let mut screen = Screen::new();
    stream_row(&mut screen, 19).unwrap();
    assert_eq!(
        screen.writes,
        if cfg!(feature = "ui-low-memory") {
            8
        } else {
            2
        }
    );
    assert_eq!(screen.accepted, 64);
    assert_eq!(&screen.pixels[..64], &expected(19)[..64]);
    assert!(screen.pixels[64..].iter().all(|p| *p == Rgb565::BLACK));
}

#[test]
fn fixed_exact_capacity_works_and_oversize_creation_preserves_array() {
    let mut app = App::new(DIMENSIONS).unwrap();
    let before = *app.workspace();
    assert_eq!(
        app.session(fixed(UI_PIXELS + 1)).err(),
        Some(ui::ConfigError::WorkspaceTooSmall)
    );
    assert_eq!(*app.workspace(), before);
    let mut session = app.session(fixed(UI_PIXELS)).unwrap();
    let mut screen = Screen::new();
    assert_eq!(
        draw(&mut session, &mut screen, 1, |_| Ok(())),
        Ok(ui::DrawOutcome::Drawn)
    );
    assert_eq!(screen.pixels, expected(1));
    assert_eq!(screen.writes, 512 / UI_PIXELS);
}

#[test]
fn oversized_policy_update_is_transactional() {
    let mut app = App::new(DIMENSIONS).unwrap();
    let mut session = app.session(fixed(UI_PIXELS)).unwrap();
    let mut screen = Screen::new();
    draw(&mut session, &mut screen, 0, |_| Ok(())).unwrap();
    let settings = *session.settings();
    let policy = session.policy();
    let binding = session.binding();
    assert_eq!(
        session.try_update(settings, fixed(UI_PIXELS + 1), resources()),
        Err(ui::ConfigError::WorkspaceTooSmall)
    );
    assert_eq!(*session.settings(), settings);
    assert_eq!(session.policy(), policy);
    assert_eq!(session.binding(), binding);
    assert_eq!(session.pending(), None);
}

#[test]
fn failed_display_preserves_full_request_and_retry_has_correct_image() {
    let mut app = App::new(DIMENSIONS).unwrap();
    let mut session = app.session(ui::RefreshSettings::new()).unwrap();
    let mut screen = Screen::new();
    screen.fail_at = Some(3);
    let mut services = 0;
    assert_eq!(
        draw(&mut session, &mut screen, 19, |_| {
            services += 1;
            Ok(())
        }),
        Err(ui::SessionError::Refresh(ui::CooperativeError::Refresh(
            ui::RefreshError::Display(TransportFailure(73))
        )))
    );
    assert_eq!(screen.writes, 3, "no silent retry");
    assert_eq!(services, 2);
    assert_eq!(session.pending(), Some(FULL));
    screen.reset_counts();
    assert_eq!(
        draw(&mut session, &mut screen, 19, |_| Ok(())),
        Ok(ui::DrawOutcome::Drawn)
    );
    assert_eq!(screen.accepted, 512);
    assert_eq!(screen.pixels, expected(19));
    assert_eq!(session.pending(), None);
}

#[test]
fn idle_does_not_call_scene_service_or_display() {
    let mut app = App::new(DIMENSIONS).unwrap();
    let mut session = app.session(ui::RefreshSettings::new()).unwrap();
    let mut screen = Screen::new();
    draw(&mut session, &mut screen, 0, |_| Ok(())).unwrap();
    screen.reset_counts();
    let outcome = session.draw(
        ui::DisplayBinding::new(&mut screen, 1),
        resources(),
        |_, _| -> Result<(), Infallible> { panic!("idle scene") },
        |_| -> Result<(), Infallible> { panic!("idle service") },
    );
    assert_eq!(outcome, Ok(ui::DrawOutcome::Idle));
    assert_eq!(screen.writes, 0);
}

#[test]
fn service_invalidation_is_deferred_until_the_next_frame() {
    let mut app = App::new(DIMENSIONS).unwrap();
    let mut session = app.session(ui::RefreshSettings::new()).unwrap();
    let mut screen = Screen::new();
    let mut posted = false;
    draw(&mut session, &mut screen, 19, |sink| {
        if !posted {
            sink.invalidate(ui::graphics::primitives::Rectangle::new(
                Point::new(63, 7),
                Size::new(1, 1),
            ));
            posted = true;
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(screen.accepted, 512);
    assert_eq!(session.pending(), Some(Rect::new(63, 7, 1, 1)));
    screen.reset_counts();
    draw(&mut session, &mut screen, 19, |_| Ok(())).unwrap();
    assert_eq!(screen.writes, 1);
    assert_eq!(screen.accepted, 1);
    assert_eq!(session.pending(), None);
    assert_eq!(screen.pixels, expected(19));
}

#[test]
fn failed_service_preserves_raw_error_and_deferred_request() {
    let mut app = App::new(DIMENSIONS).unwrap();
    let mut session = app.session(ui::RefreshSettings::new()).unwrap();
    let mut screen = Screen::new();
    let result = session.draw(
        ui::DisplayBinding::new(&mut screen, 1),
        resources(),
        |tile, context| draw_scene(1, tile, context),
        |sink| {
            sink.invalidate_all();
            Err::<(), _>("service paused")
        },
    );
    assert_eq!(
        result,
        Err(ui::SessionError::Refresh(ui::CooperativeError::Service(
            "service paused"
        )))
    );
    assert_eq!(screen.writes, 1);
    assert_eq!(session.pending(), Some(FULL));
    draw(&mut session, &mut screen, 1, |_| Ok(())).unwrap();
    assert_eq!(screen.pixels, expected(1));
}

#[test]
fn render_once_distinguishes_config_stream_and_session_errors() {
    let mut screen = Screen::new();
    let config = render_once(&mut screen, 0, fixed(UI_PIXELS + 1), |_| {
        Ok::<_, Infallible>(())
    })
    .unwrap_err();
    assert_eq!(
        config.error,
        RenderError::Config(ui::ConfigError::WorkspaceTooSmall)
    );
    assert_eq!(config.pending, None);
    assert_eq!(screen.writes, 0);
    screen.fail_at = Some(1);
    let stream = render_once(&mut screen, 0, ui::RefreshSettings::new(), |_| {
        Ok::<_, Infallible>(())
    })
    .unwrap_err();
    assert_eq!(stream.error, RenderError::Stream(TransportFailure(73)));
    assert_eq!(stream.pending, Some(FULL));
    assert_eq!(screen.writes, 1);
    screen.reset_counts();
    screen.fail_at = Some(64 / UI_PIXELS + 1);
    let session = render_once(&mut screen, 0, ui::RefreshSettings::new(), |_| {
        Ok::<_, Infallible>(())
    })
    .unwrap_err();
    assert_eq!(
        session.error,
        RenderError::Session(ui::SessionError::Refresh(ui::CooperativeError::Refresh(
            ui::RefreshError::Display(TransportFailure(73))
        ),))
    );
    assert_eq!(session.pending, Some(FULL));
    assert_eq!(screen.writes, 64 / UI_PIXELS + 1);
}

#[test]
fn render_once_preserves_service_cause_and_renders_successfully() {
    let mut screen = Screen::new();
    let failed =
        render_once(&mut screen, 0, ui::RefreshSettings::new(), |_| Err("pause")).unwrap_err();
    assert_eq!(
        failed.error,
        RenderError::Session(ui::SessionError::Refresh(ui::CooperativeError::Service(
            "pause"
        ),))
    );
    assert_eq!(failed.pending, Some(FULL));
    screen.reset_counts();
    assert_eq!(
        render_once(&mut screen, 63, ui::RefreshSettings::new(), |_| Ok::<
            _,
            Infallible,
        >(
            ()
        )),
        Ok(FrameReport {
            outcome: ui::DrawOutcome::Drawn,
            pending: None
        })
    );
    assert_eq!(screen.pixels, expected(63));
    assert_eq!(screen.accepted, 576);
}

#[test]
fn invalid_dimensions_retain_config_cause_without_writes() {
    assert_eq!(
        App::new(Dimensions::new(0, 8)).err(),
        Some(ui::ConfigError::InvalidDimensions)
    );
    let mut screen = Screen::new();
    screen.dimensions = Dimensions::new(64, 0);
    let failed = render_once(&mut screen, 0, ui::RefreshSettings::new(), |_| {
        Ok::<_, Infallible>(())
    })
    .unwrap_err();
    assert_eq!(
        failed.error,
        RenderError::Config(ui::ConfigError::InvalidDimensions)
    );
    assert_eq!(failed.pending, None);
    assert_eq!(screen.writes, 0);
}

#[test]
fn one_shot_reports_future_work_without_consuming_it() {
    let mut screen = Screen::new();
    let report = render_once(&mut screen, 19, ui::RefreshSettings::new(), |sink| {
        sink.invalidate(ui::graphics::primitives::Rectangle::new(
            Point::new(63, 7),
            Size::new(1, 1),
        ));
        Ok::<_, Infallible>(())
    })
    .unwrap();
    assert_eq!(report.outcome, ui::DrawOutcome::Drawn);
    assert_eq!(report.pending, Some(Rect::new(63, 7, 1, 1)));
    assert_eq!(screen.accepted, 576, "one row plus one frame only");
    assert_eq!(screen.pixels, expected(19));
}

#[test]
fn failed_one_shot_returns_full_retry_request_and_original_cause() {
    let mut screen = Screen::new();
    let failure = render_once(&mut screen, 19, ui::RefreshSettings::new(), |sink| {
        sink.invalidate(ui::graphics::primitives::Rectangle::new(
            Point::new(63, 7),
            Size::new(1, 1),
        ));
        Err("paused")
    })
    .unwrap_err();
    assert_eq!(failure.pending, Some(FULL));
    assert_eq!(
        failure.error,
        RenderError::Session(ui::SessionError::Refresh(ui::CooperativeError::Service(
            "paused"
        ),))
    );
    screen.reset_counts();
    let report = render_once(&mut screen, 19, ui::RefreshSettings::new(), |_| {
        Ok::<_, Infallible>(())
    })
    .unwrap();
    assert_eq!(report.pending, None);
    assert_eq!(screen.pixels, expected(19));
    assert_eq!(screen.accepted, 576);
}

#[test]
fn failed_one_shot_stream_returns_full_unattempted_frame_request() {
    let mut screen = Screen::new();
    screen.fail_at = Some(1);
    let failure = render_once(&mut screen, 1, ui::RefreshSettings::new(), |_| {
        Ok::<_, Infallible>(())
    })
    .unwrap_err();
    assert_eq!(failure.pending, Some(FULL));
    assert_eq!(failure.error, RenderError::Stream(TransportFailure(73)));
    assert_eq!(screen.writes, 1, "no hidden retry or frame attempt");
}
