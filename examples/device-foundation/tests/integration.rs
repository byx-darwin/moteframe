//! Observable composition behavior, using actual UI/assets/input contracts.
#![allow(clippy::unwrap_used)]
use core::num::NonZeroU32;
use moteframe_core::{Dimensions, Display, Rect};
use moteframe_device_foundation::{
    Catalog, Demo, Sample, Services, UiMemory,
    model::{Error, LinkModel, Sink, SourceModel},
};
use moteframe_input::{ButtonConfig, Point, TouchConfig, TouchSample};
use moteframe_ui::{
    DrawOutcome, FrameInterval, RefreshSettings, SessionError,
    graphics::pixelcolor::{Rgb565, RgbColor},
    graphics::prelude::IntoStorage,
};

#[derive(Debug)]
struct Screen {
    pixels: [Rgb565; 512],
    sink: Sink,
}
impl Screen {
    fn new() -> Self {
        Self {
            pixels: [Rgb565::BLACK; 512],
            sink: Sink::new(),
        }
    }
    fn pixel(&self, x: usize, y: usize) -> Rgb565 {
        self.pixels[y * 32 + x]
    }
}
impl Display for Screen {
    type Color = Rgb565;
    type Error = Error;
    fn dimensions(&self) -> Dimensions {
        self.sink.dimensions()
    }
    fn write(&mut self, area: Rect, pixels: &[Rgb565]) -> Result<(), Error> {
        self.sink.write(area, pixels)?;
        for (i, p) in pixels.iter().enumerate() {
            self.pixels[usize::from(area.y) * 32 + usize::from(area.x) + i] = *p;
        }
        Ok(())
    }
}
fn services() -> Services<LinkModel> {
    Services::new(
        core::num::NonZeroU64::MIN,
        LinkModel::new(),
        ButtonConfig::new(0, Some(100)).unwrap(),
        TouchConfig::new(32, 16, 1, 4, 100).unwrap(),
    )
}

#[test]
fn input_changes_real_pixels_and_assets_are_read_through_a_window() {
    let c = Catalog::new(0).unwrap();
    let mut memory = UiMemory::new(&c).unwrap();
    let mut d = Demo::new(&mut memory, services(), c, RefreshSettings::new()).unwrap();
    let mut s = Screen::new();
    let mut r = SourceModel::new();
    assert_eq!(d.step(&mut s, 1, &mut r, 0).draw, Ok(DrawOutcome::Drawn));
    assert_eq!(s.pixel(1, 1), Rgb565::WHITE);
    assert_eq!(s.pixel(2, 1), Rgb565::BLACK);
    assert_eq!(s.pixel(8, 1), Rgb565::RED);
    assert_eq!(s.pixel(9, 1), Rgb565::GREEN);
    assert_eq!(s.pixel(10, 1), Rgb565::BLUE);
    assert_eq!(s.pixel(1, 8), Rgb565::RED);
    assert!(r.reads > 0);
    assert_eq!(r.lowest, 4);
    d.services_mut()
        .enqueue_input(Sample::Button {
            pressed: true,
            at_us: 0,
        })
        .unwrap();
    for n in 1..4 {
        d.step(&mut s, 1, &mut r, n);
    }
    assert_eq!(s.pixel(1, 8), Rgb565::GREEN);
    assert_eq!(d.services().processed_events(), 1);
}
#[test]
fn mid_frame_touch_changes_only_the_next_frame() {
    let c = Catalog::new(0).unwrap();
    let mut memory = UiMemory::new(&c).unwrap();
    let mut d = Demo::new(&mut memory, services(), c, RefreshSettings::new()).unwrap();
    let mut s = Screen::new();
    let mut r = SourceModel::new();
    for (sample, at_us) in [
        (TouchSample::Down(Point { x: 1, y: 1 }), 0),
        (TouchSample::Up(Point { x: 1, y: 1 }), 1),
    ] {
        d.services_mut()
            .enqueue_input(Sample::Touch { sample, at_us })
            .unwrap();
    }
    let report = d.step(&mut s, 1, &mut r, 0);
    assert_eq!(report.services.events, 1);
    assert_eq!(d.services().snapshot().selection, 1);
    assert_eq!(s.pixel(1, 8), Rgb565::RED);
    assert!(d.pending().is_some());
    d.step(&mut s, 1, &mut r, 1);
    assert_eq!(s.pixel(1, 8), Rgb565::GREEN);
    assert!(d.pending().is_none());
}
#[test]
fn idle_and_deferred_frames_still_service_rx_tx_and_input() {
    let c = Catalog::new(0).unwrap();
    let mut memory = UiMemory::new(&c).unwrap();
    let mut d = Demo::new(
        &mut memory,
        services(),
        c,
        RefreshSettings::new().with_frame_interval(FrameInterval::MinimumInterval(
            NonZeroU32::new(100).unwrap(),
        )),
    )
    .unwrap();
    let mut s = Screen::new();
    let mut r = SourceModel::new();
    d.step(&mut s, 1, &mut r, 0);
    let writes = s.sink.writes;
    let idle = d.step(&mut s, 1, &mut r, 1);
    assert_eq!(idle.draw, Ok(DrawOutcome::Idle));
    assert_eq!(idle.services.rounds, 2);
    let target = d.services().link().target;
    d.services_mut().link_mut().inject(target, &[]).unwrap();
    d.services_mut().enqueue_notification(&[3]).unwrap();
    d.services_mut()
        .enqueue_input(Sample::Button {
            pressed: true,
            at_us: 0,
        })
        .unwrap();
    let deferred = d.step(&mut s, 1, &mut r, 2);
    assert!(matches!(deferred.draw, Ok(DrawOutcome::Deferred { .. })));
    assert_eq!(deferred.services.received, 1);
    assert_eq!(deferred.services.accepted, 1);
    assert_eq!(deferred.services.rounds, 2);
    assert_eq!(deferred.services.samples, 1);
    assert_eq!(s.sink.writes, writes);
    assert_eq!(d.step(&mut s, 1, &mut r, 100).draw, Ok(DrawOutcome::Drawn));
    assert_eq!(s.pixel(1, 14), Rgb565::GREEN);
    assert_eq!(s.pixel(5, 14), Rgb565::BLUE);
}
#[test]
fn source_decode_and_display_failures_retain_full_redraw_and_service_lanes() {
    let c = Catalog::new(0).unwrap();
    let mut memory = UiMemory::new(&c).unwrap();
    let mut d = Demo::new(&mut memory, services(), c, RefreshSettings::new()).unwrap();
    let mut s = Screen::new();
    let mut r = SourceModel::new();
    r.fail_at = Some(1);
    let failed = d.step(&mut s, 1, &mut r, 0);
    assert!(failed.draw.is_err());
    assert_eq!(failed.services.rounds, 2 + s.sink.writes);
    assert_eq!(d.pending(), Some(Rect::new(0, 0, 32, 16)));
    r.fail_at = None;
    r.corrupt = true;
    assert!(d.step(&mut s, 1, &mut r, 1).draw.is_err());
    assert!(d.pending().is_some());
    r.corrupt = false;
    d.update_resources(1).unwrap();
    s.sink.fail_at = Some(s.sink.writes + 1);
    let failed = d.step(&mut s, 1, &mut r, 2);
    assert!(failed.draw.is_err());
    assert_eq!(failed.services.rounds, 2);
    assert!(d.pending().is_some());
    s.sink.fail_at = None;
    assert_eq!(d.step(&mut s, 1, &mut r, 3).draw, Ok(DrawOutcome::Drawn));
    assert_eq!(s.pixel(1, 1), Rgb565::WHITE);
    assert!(d.pending().is_none());
}
#[test]
fn changed_display_requires_explicit_fixed_geometry_rebind() {
    let c = Catalog::new(0).unwrap();
    let mut memory = UiMemory::new(&c).unwrap();
    let mut d = Demo::new(&mut memory, services(), c, RefreshSettings::new()).unwrap();
    let mut s = Screen::new();
    let mut r = SourceModel::new();
    d.step(&mut s, 1, &mut r, 0);
    let changed = d.step(&mut s, 2, &mut r, 1);
    assert!(matches!(changed.draw, Err(SessionError::NeedsRebind)));
    assert_eq!(changed.services.rounds, 2);
    s.sink.size = Dimensions::new(16, 32);
    assert!(d.rebind(&mut s, 2).is_err());
    assert!(matches!(
        d.step(&mut s, 2, &mut r, 2).draw,
        Err(SessionError::NeedsRebind)
    ));
    s.sink.size = Dimensions::new(32, 16);
    d.rebind(&mut s, 2).unwrap();
    assert_eq!(d.step(&mut s, 2, &mut r, 3).draw, Ok(DrawOutcome::Drawn));
    assert!(d.pending().is_none());
}
#[test]
fn rx_flood_and_poll_errors_cannot_starve_other_lanes_or_exceed_round_bounds() {
    let c = Catalog::new(0).unwrap();
    let mut memory = UiMemory::new(&c).unwrap();
    let mut d = Demo::new(&mut memory, services(), c, RefreshSettings::new()).unwrap();
    let mut s = Screen::new();
    let mut r = SourceModel::new();
    d.step(&mut s, 1, &mut r, 0);
    let mut received = 0;
    let mut accepted = 0;
    for n in 0..20 {
        let target = d.services().link().target;
        d.services_mut().link_mut().inject(target, &[2]).unwrap();
        d.services_mut().enqueue_notification(&[4]).unwrap();
        d.services_mut()
            .enqueue_input(Sample::Button {
                pressed: n % 2 == 0,
                at_us: n,
            })
            .unwrap();
        let report = d.step(&mut s, 1, &mut r, n + 1);
        assert!(report.draw.is_ok());
        assert_eq!(report.services.polls, report.services.rounds);
        assert!(report.services.received <= report.services.rounds);
        assert!(report.services.attempts <= report.services.rounds);
        assert!(report.services.samples <= report.services.rounds);
        received += report.services.received;
        accepted += report.services.accepted;
    }
    assert_eq!(received, 20);
    assert_eq!(accepted, 20);
    assert_eq!(d.services().processed_samples(), 20);
    assert_eq!(d.services().processed_events(), 20);
    d.services_mut().link_mut().poll_faults = 2;
    d.services_mut()
        .enqueue_input(Sample::Button {
            pressed: true,
            at_us: 20,
        })
        .unwrap();
    let report = d.step(&mut s, 1, &mut r, 21);
    assert_eq!(report.services.poll_error, Some(Error::Poll));
    assert_eq!(report.services.samples, 1);
    assert_eq!(report.services.attempts, 0);
}
#[test]
fn marker_damage_reads_no_assets_and_update_versions_are_atomic() {
    let c = Catalog::new(0).unwrap();
    let mut memory = UiMemory::new(&c).unwrap();
    let mut d = Demo::new(&mut memory, services(), c, RefreshSettings::new()).unwrap();
    let mut s = Screen::new();
    let mut r = SourceModel::new();
    d.step(&mut s, 1, &mut r, 0);
    let reads = r.reads;
    let writes = s.sink.writes;
    let target = d.services().link().target;
    d.services_mut().link_mut().inject(target, &[1]).unwrap();
    d.step(&mut s, 1, &mut r, 1);
    assert_eq!(r.reads, reads);
    assert_eq!(s.sink.writes, writes + 1);
    assert_eq!(s.pixel(1, 14).into_storage(), Rgb565::GREEN.into_storage());
    assert!(d.update_resources(0).is_err());
    assert_eq!(d.resource_revision(), 0);
    assert!(d.pending().is_none());
    d.update_resources(1).unwrap();
    assert_eq!(d.resource_revision(), 1);
    assert_eq!(d.pending(), Some(Rect::new(0, 0, 32, 16)));
}

#[test]
fn resource_scratch_debug_does_not_expose_loaded_bytes() {
    let scratch = moteframe_device_foundation::scene::RenderScratch::new();
    let debug = format!("{scratch:?}");
    assert!(!debug.contains("mono"));
    assert!(!debug.contains("rgb"));
    assert!(!debug.contains("bytes:"));
}
