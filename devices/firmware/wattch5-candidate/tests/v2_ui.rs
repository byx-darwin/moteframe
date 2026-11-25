//! Native V2 adapter: real renderer, bounded row writes and asynchronous settings.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use core::cell::{Cell, RefCell};
use device_core::{Dimensions, Display, Rect, Rgb565};
use device_runtime::{PortError, Work};
use wattch5_bringup::{Event, InputReport, InputSource, Readiness, ResetGate, TouchStatus};
use wattch5_candidate::{ui::SettingsPort, v2_ui::V2Ui};
use wattch5_parity_ui_demo::{Appearance, Field, Locale, Screen};

struct Panel<'a> {
    frame: &'a RefCell<Vec<Rgb565>>,
    writes: &'a Cell<usize>,
    fail: &'a Cell<bool>,
}
impl Display for Panel<'_> {
    type Error = ();
    fn dimensions(&self) -> Dimensions {
        Dimensions::new(128, 128)
    }
    fn write(&mut self, r: Rect, p: &[Rgb565]) -> Result<(), ()> {
        assert!((1..=8).contains(&r.width));
        assert_eq!((r.height, p.len()), (1, usize::from(r.width)));
        assert!(r.fits(Dimensions::new(128, 128)));
        if self.fail.replace(false) {
            return Err(());
        }
        let offset = usize::from(r.y) * 128 + usize::from(r.x);
        self.frame.borrow_mut()[offset..offset + p.len()].copy_from_slice(p);
        self.writes.set(self.writes.get() + 1);
        Ok(())
    }
}
struct Input<'a>(&'a Cell<Option<Event>>);
impl InputSource for Input<'_> {
    fn sample(&mut self, _: u64) -> InputReport {
        InputReport {
            event: self.0.take(),
            touch: TouchStatus::Ready,
            fault: None,
        }
    }
}
struct Settings {
    actual: Cell<Appearance>,
    pending: Cell<Option<(Field, u8)>>,
    complete: Cell<bool>,
    rejected: Cell<bool>,
    finishes: Cell<usize>,
    loads: Cell<usize>,
}
impl Settings {
    fn new() -> Self {
        Self {
            actual: Cell::new(Appearance::default()),
            pending: Cell::new(None),
            complete: Cell::new(false),
            rejected: Cell::new(false),
            finishes: Cell::new(0),
            loads: Cell::new(0),
        }
    }
}
impl SettingsPort for Settings {
    fn load(&self) -> Result<Appearance, PortError> {
        self.loads.set(self.loads.get() + 1);
        Ok(self.actual.get())
    }
    fn begin(&self, field: Field, value: u8) -> Result<(), PortError> {
        if self.rejected.get() || self.pending.get().is_some() {
            return Err(PortError::Unavailable);
        }
        self.pending.set(Some((field, value)));
        Ok(())
    }
    fn poll(&self) -> Result<bool, PortError> {
        Ok(self.complete.get())
    }
    fn finish(&self) -> Result<(), PortError> {
        assert!(self.complete.get());
        self.pending.set(None);
        self.finishes.set(self.finishes.get() + 1);
        Ok(())
    }
}
fn gate() -> ResetGate {
    let gate = ResetGate::new();
    gate.completed();
    gate.arm_ui().unwrap();
    gate
}
fn tap(x: u8, y: u8) -> Event {
    Event::Tap { x, y }
}
fn language(ui: &mut impl Work, input: &Cell<Option<Event>>) {
    // Original V2 menus: dial ↑ tools, page2 settings; settings page2 language.
    for e in [
        Event::Up,
        Event::Up,
        tap(20, 20),
        Event::Up,
        tap(20, 60),
        Event::Up,
    ] {
        input.set(Some(e));
        ui.step(0).unwrap();
    }
}
#[test]
fn test_should_show_full_v2_default_face_and_reach_original_tools_and_language() {
    let input = Cell::new(None);
    let status = Cell::new(Readiness::new());
    let gate = gate();
    let frame = RefCell::new(vec![Rgb565(0); 16384]);
    let writes = Cell::new(0);
    let fail = Cell::new(false);
    let settings = Settings::new();
    let mut ui = V2Ui::new(
        Panel {
            frame: &frame,
            writes: &writes,
            fail: &fail,
        },
        Input(&input),
        |_| Ok(()),
        &status,
        &gate,
    )
    .unwrap();
    ui.bind(&settings).unwrap();
    assert_eq!(ui.model().screen(), Screen::Dial);
    for n in 0..4096 {
        let before = writes.get();
        ui.step(n).unwrap();
        assert!(
            writes.get() - before <= 4,
            "yield after one native32-pixel chunk (four legacy compatibility writes)"
        );
    }
    let completed = writes.get();
    assert!(frame.borrow().iter().any(|p| p.0 != 0));
    for n in 4096..4800 {
        ui.step(n).unwrap();
    }
    assert_eq!(
        writes.get(),
        completed,
        "unchanged scene must stop sending rows"
    );
    language(&mut ui, &input);
    assert_eq!(ui.model().screen(), Screen::Language);
    assert_eq!(ui.model().draft(), 1);
    assert_eq!(ui.model().appearance().locale, Locale::Zh);
}
#[test]
fn test_should_publish_locale_only_after_durable_readback_and_finish_while_navigating() {
    let input = Cell::new(None);
    let status = Cell::new(Readiness::new());
    let gate = gate();
    let frame = RefCell::new(vec![Rgb565(0); 16384]);
    let writes = Cell::new(0);
    let fail = Cell::new(false);
    let settings = Settings::new();
    let mut ui = V2Ui::new(
        Panel {
            frame: &frame,
            writes: &writes,
            fail: &fail,
        },
        Input(&input),
        |_| Ok(()),
        &status,
        &gate,
    )
    .unwrap();
    ui.bind(&settings).unwrap();
    language(&mut ui, &input);
    input.set(Some(tap(100, 110)));
    ui.step(1).unwrap();
    assert_eq!(settings.pending.get(), Some((Field::Language, 1)));
    assert_eq!(ui.model().appearance().locale, Locale::Zh);
    input.set(Some(Event::KeyRelease));
    ui.step(2).unwrap();
    settings.actual.set(Appearance {
        locale: Locale::En,
        ..Appearance::default()
    });
    settings.complete.set(true);
    ui.step(3).unwrap();
    assert_eq!(ui.model().appearance().locale, Locale::En);
    assert_eq!(settings.finishes.get(), 1);
    for n in 64..1600 {
        ui.step(n).unwrap();
    }
    assert_eq!(
        settings.finishes.get(),
        1,
        "accepted write cannot be reissued or acknowledged twice"
    );
}
#[test]
fn test_should_retry_failed_panel_row_and_keep_input_and_settings_progress() {
    let input = Cell::new(None);
    let status = Cell::new(Readiness::new());
    let gate = gate();
    let frame = RefCell::new(vec![Rgb565(0); 16384]);
    let writes = Cell::new(0);
    let fail = Cell::new(true);
    let mut ui = V2Ui::new(
        Panel {
            frame: &frame,
            writes: &writes,
            fail: &fail,
        },
        Input(&input),
        |_| Ok(()),
        &status,
        &gate,
    )
    .unwrap();
    assert_eq!(ui.step(0), Err(PortError::Fault));
    assert_eq!(writes.get(), 0);
    input.set(Some(Event::Up));
    ui.step(1).unwrap();
    assert_eq!(ui.model().screen(), Screen::Tools);
    for n in 32..4800 {
        ui.step(n).unwrap();
    }
    assert!(frame.borrow().iter().any(|p| p.0 != 0));
}

#[test]
fn test_should_keep_shared_health_result_and_stop_pending_work_on_navigation() {
    use wattch5_candidate::ui::DeviceHealth;
    use wattch5_parity_ui_demo::{MeasureKind, MeasureState, MeasurementSnapshot, Reading};
    struct Health {
        actual: Cell<MeasurementSnapshot>,
        requests: RefCell<Vec<bool>>,
    }
    impl DeviceHealth for Health {
        fn page(&self, k: MeasureKind) -> Result<MeasurementSnapshot, PortError> {
            Ok(if k == MeasureKind::Pressure {
                self.actual.get()
            } else {
                MeasurementSnapshot::default()
            })
        }
        fn request(&self, k: MeasureKind, start: bool) -> Result<(), PortError> {
            assert_eq!(k, MeasureKind::Pressure);
            self.requests.borrow_mut().push(start);
            Ok(())
        }
    }
    let input = Cell::new(None);
    let status = Cell::new(Readiness::new());
    let gate = gate();
    let frame = RefCell::new(vec![Rgb565(0); 16384]);
    let writes = Cell::new(0);
    let fail = Cell::new(false);
    let health = Health {
        actual: Cell::new(MeasurementSnapshot {
            state: MeasureState::Result,
            reading: Reading::Pressure {
                upper: 120,
                lower: 80,
            },
        }),
        requests: RefCell::new(vec![]),
    };
    let mut ui = V2Ui::new(
        Panel {
            frame: &frame,
            writes: &writes,
            fail: &fail,
        },
        Input(&input),
        |_| Ok(()),
        &status,
        &gate,
    )
    .unwrap();
    ui.bind_health(&health).unwrap();
    for _ in 0..5 {
        input.set(Some(Event::Left));
        ui.step(0).unwrap();
    }
    assert_eq!(ui.model().screen(), Screen::Pressure);
    for n in 16..6400 {
        ui.step(n).unwrap();
    }
    let result = frame.borrow().clone();
    // Independently supply the known provider result to the full V2 renderer.
    // A missing/wrong measurement mapping in the adapter must change final pixels.
    {
        use device_ui::graphics::{pixelcolor::Rgb565 as Color, prelude::*};
        struct Raster(Vec<Rgb565>);
        impl OriginDimensions for Raster {
            fn size(&self) -> Size {
                Size::new(128, 128)
            }
        }
        impl DrawTarget for Raster {
            type Color = Color;
            type Error = ();
            fn draw_iter<T: IntoIterator<Item = Pixel<Color>>>(
                &mut self,
                pixels: T,
            ) -> Result<(), ()> {
                for Pixel(p, c) in pixels {
                    assert!((0..128).contains(&p.x) && (0..128).contains(&p.y));
                    self.0[p.y as usize * 128 + p.x as usize] = Rgb565(c.into_storage());
                }
                Ok(())
            }
        }
        let mut expected_data = wattch5_parity_ui_demo::Snapshot::unavailable();
        expected_data.measurements[MeasureKind::Pressure as usize] = MeasurementSnapshot {
            state: MeasureState::Result,
            reading: Reading::Pressure {
                upper: 120,
                lower: 80,
            },
        };
        let mut expected_model = wattch5_parity_ui_demo::Model::default();
        expected_model
            .preview(Screen::Pressure, &expected_data)
            .unwrap();
        let mut expected = Raster(vec![Rgb565(0); 16384]);
        expected_model
            .product_scene(&expected_data)
            .unwrap()
            .draw(&mut expected)
            .unwrap();
        assert_eq!(
            result, expected.0,
            "native frame must show actual shared result with full V2 geometry"
        );
    }
    input.set(Some(tap(80, 110)));
    ui.step(401).unwrap();
    assert_eq!(&*health.requests.borrow(), &[true]);
    health.actual.set(MeasurementSnapshot {
        state: MeasureState::Pending,
        reading: Reading::Missing,
    });
    // In-flight row snapshot can be older; action admission must consult the actual owner.
    input.set(Some(tap(80, 110)));
    ui.step(402).unwrap();
    assert_eq!(&*health.requests.borrow(), &[true, false]);
    input.set(Some(Event::Left));
    ui.step(403).unwrap();
    assert_eq!(health.requests.borrow().last(), Some(&false));
    assert_eq!(ui.model().screen(), Screen::Oxygen);
    assert!(result.iter().any(|p| p.0 != 0));
}

#[test]
fn test_should_retry_backlight_after_external_setting_readback_fails_once() {
    let input = Cell::new(None);
    let status = Cell::new(Readiness::new());
    let gate = gate();
    let frame = RefCell::new(vec![Rgb565(0); 16384]);
    let writes = Cell::new(0);
    let fail = Cell::new(false);
    let light_fail = Cell::new(false);
    let duty = Cell::new(0);
    let settings = Settings::new();
    let mut ui = V2Ui::new(
        Panel {
            frame: &frame,
            writes: &writes,
            fail: &fail,
        },
        Input(&input),
        |n| {
            if light_fail.replace(false) {
                Err(PortError::Fault)
            } else {
                duty.set(n);
                Ok(())
            }
        },
        &status,
        &gate,
    )
    .unwrap();
    ui.bind(&settings).unwrap();
    assert_eq!(duty.get(), 40);
    settings.actual.set(Appearance {
        brightness: 4,
        ..Appearance::default()
    });
    light_fail.set(true);
    assert_eq!(ui.step(0), Err(PortError::Fault));
    assert_eq!(duty.get(), 40);
    ui.step(1).unwrap();
    assert_eq!(duty.get(), 100);
    assert_eq!(ui.model().appearance().brightness, 4);
}

#[test]
fn test_should_repaint_health_after_unrelated_read_failure_and_allow_exiting_failed_provider() {
    use wattch5_candidate::ui::DeviceHealth;
    use wattch5_parity_ui_demo::{MeasureKind, MeasureState, MeasurementSnapshot, Reading};
    struct Health {
        upper: Cell<u16>,
        bad: Cell<Option<MeasureKind>>,
    }
    impl DeviceHealth for Health {
        fn page(&self, k: MeasureKind) -> Result<MeasurementSnapshot, PortError> {
            if self.bad.get() == Some(k) {
                return Err(PortError::Unavailable);
            }
            Ok(if k == MeasureKind::Pressure {
                MeasurementSnapshot {
                    state: MeasureState::Result,
                    reading: Reading::Pressure {
                        upper: self.upper.get(),
                        lower: 80,
                    },
                }
            } else {
                MeasurementSnapshot::default()
            })
        }
        fn request(&self, _: MeasureKind, _: bool) -> Result<(), PortError> {
            Err(PortError::Unavailable)
        }
    }
    let input = Cell::new(None);
    let status = Cell::new(Readiness::new());
    let gate = gate();
    let frame = RefCell::new(vec![Rgb565(0); 16384]);
    let writes = Cell::new(0);
    let fail = Cell::new(false);
    let health = Health {
        upper: Cell::new(120),
        bad: Cell::new(None),
    };
    let mut ui = V2Ui::new(
        Panel {
            frame: &frame,
            writes: &writes,
            fail: &fail,
        },
        Input(&input),
        |_| Ok(()),
        &status,
        &gate,
    )
    .unwrap();
    ui.bind_health(&health).unwrap();
    for _ in 0..5 {
        input.set(Some(Event::Left));
        ui.step(0).unwrap();
    }
    for n in 0..6400 {
        ui.step(n).unwrap();
    }
    let before = frame.borrow().clone();
    let before_writes = writes.get();
    health.upper.set(135);
    health.bad.set(Some(MeasureKind::Oxygen));
    for n in 6400..11200 {
        let _ = ui.step(n);
    }
    health.bad.set(None);
    for n in 11200..16000 {
        ui.step(n).unwrap();
    }
    assert!(
        writes.get() > before_writes,
        "changed visible result must be repainted after source recovers"
    );
    assert_ne!(*frame.borrow(), before);
    health.bad.set(Some(MeasureKind::Pressure));
    input.set(Some(Event::KeyRelease));
    let _ = ui.step(1001);
    assert_eq!(
        ui.model().screen(),
        Screen::Dial,
        "health failure cannot trap navigation"
    );
    let before = writes.get();
    for n in 16032..22400 {
        let _ = ui.step(n);
    }
    assert!(
        writes.get() > before,
        "failed health provider must not block unrelated dial drawing"
    );
}

#[test]
fn test_should_retry_unconfirmed_native_chunk_preserve_confirmed_pixels_and_keep_input_progress() {
    type Native = device_ui::graphics::pixelcolor::Rgb565;
    struct Chunks<'a>(&'a RefCell<Vec<(Rect, Vec<Native>)>>);
    impl Display for Chunks<'_> {
        type Error = ();
        fn dimensions(&self) -> Dimensions {
            Dimensions::new(128, 128)
        }
        fn write(&mut self, _: Rect, _: &[Rgb565]) -> Result<(), ()> {
            Err(())
        }
        fn write_rgb565(
            &mut self,
            r: Rect,
            p: &[Native],
        ) -> Result<(), device_core::NativeWriteError<()>> {
            assert_eq!((r.width, r.height, p.len()), (32, 1, 32));
            let mut attempts = self.0.borrow_mut();
            attempts.push((r, p.to_vec()));
            if attempts.len() == 2 {
                Err(device_core::NativeWriteError::Display(()))
            } else {
                Ok(())
            }
        }
    }
    let attempts = RefCell::new(Vec::new());
    let input = Cell::new(None);
    let status = Cell::new(Readiness::new());
    let gate = gate();
    let mut ui = V2Ui::new(Chunks(&attempts), Input(&input), |_| Ok(()), &status, &gate).unwrap();
    ui.step(0).unwrap();
    assert_eq!(ui.step(1), Err(PortError::Fault));
    input.set(Some(Event::Up));
    ui.step(2).unwrap();
    assert_eq!(ui.model().screen(), Screen::Tools);
    assert_eq!(ui.frames(), 0);
    {
        let a = attempts.borrow();
        assert_eq!(
            a[1], a[2],
            "retry frozen failed chunk even if input arrived"
        );
        assert_eq!(
            a.iter().map(|(r, _)| *r).collect::<Vec<_>>(),
            vec![
                Rect::new(0, 0, 32, 1),
                Rect::new(32, 0, 32, 1),
                Rect::new(32, 0, 32, 1)
            ]
        );
    }
    ui.step(3).unwrap();
    assert_eq!(
        attempts.borrow()[3].0,
        Rect::new(0, 0, 32, 1),
        "then preempt obsolete scene, without finishing its remaining rows"
    );
}

#[test]
fn test_should_reject_invalid_visible_reading_repeatedly_without_poisoning_frozen_snapshot() {
    use wattch5_candidate::ui::DeviceHealth;
    use wattch5_parity_ui_demo::{MeasureKind, MeasureState, MeasurementSnapshot, Reading};
    struct Health(Cell<u16>);
    impl DeviceHealth for Health {
        fn page(&self, kind: MeasureKind) -> Result<MeasurementSnapshot, PortError> {
            Ok(if kind == MeasureKind::Oxygen {
                MeasurementSnapshot {
                    state: MeasureState::Result,
                    reading: Reading::Number(self.0.get()),
                }
            } else {
                MeasurementSnapshot::default()
            })
        }
        fn request(&self, _: MeasureKind, _: bool) -> Result<(), PortError> {
            Err(PortError::Unavailable)
        }
    }
    let input = Cell::new(None);
    let status = Cell::new(Readiness::new());
    let gate = gate();
    let frame = RefCell::new(vec![Rgb565(0); 16384]);
    let writes = Cell::new(0);
    let fail = Cell::new(false);
    let health = Health(Cell::new(98));
    let mut ui = V2Ui::new(
        Panel {
            frame: &frame,
            writes: &writes,
            fail: &fail,
        },
        Input(&input),
        |_| Ok(()),
        &status,
        &gate,
    )
    .unwrap();
    ui.bind_health(&health).unwrap();
    for _ in 0..6 {
        input.set(Some(Event::Left));
        ui.step(0).unwrap();
    }
    assert_eq!(ui.model().screen(), Screen::Oxygen);
    for n in 16..6400 {
        ui.step(n).unwrap();
    }
    let before = frame.borrow().clone();
    let count = writes.get();
    // Formatting accepts a u16, but the V2 oxygen contract is at most999.
    health.0.set(u16::MAX);
    assert_eq!(ui.step(6400), Err(PortError::Fault));
    assert_eq!(
        ui.step(6420),
        Err(PortError::Fault),
        "the next scheduled provider read must reject the bad cell again without poisoning the frozen snapshot"
    );
    assert_eq!(writes.get(), count);
    assert_eq!(*frame.borrow(), before);
    health.0.set(97);
    for n in 6432..11200 {
        ui.step(n).unwrap();
    }
    assert_ne!(
        *frame.borrow(),
        before,
        "a later valid source must recover and repaint"
    );
}

#[test]
fn test_should_keep_unbound_heart_missing_after_caching_native_health_readings() {
    use wattch5_candidate::ui::DeviceHealth;
    use wattch5_parity_ui_demo::{MeasureKind, MeasureState, MeasurementSnapshot, Reading};
    struct Health;
    impl DeviceHealth for Health {
        fn page(&self, kind: MeasureKind) -> Result<MeasurementSnapshot, PortError> {
            let reading = match kind {
                MeasureKind::Pressure => Reading::Pressure {
                    upper: 123,
                    lower: 81,
                },
                MeasureKind::Oxygen => Reading::Number(98),
                MeasureKind::Glucose => Reading::Tenth(54),
                _ => panic!("unbound kinds must not query the native provider"),
            };
            Ok(MeasurementSnapshot {
                state: MeasureState::Result,
                reading,
            })
        }
        fn request(&self, _: MeasureKind, _: bool) -> Result<(), PortError> {
            Err(PortError::Unavailable)
        }
    }
    let input = Cell::new(None);
    let status = Cell::new(Readiness::new());
    let gate = gate();
    let frame = RefCell::new(vec![Rgb565(0); 16384]);
    let writes = Cell::new(0);
    let fail = Cell::new(false);
    let mut ui = V2Ui::new(
        Panel {
            frame: &frame,
            writes: &writes,
            fail: &fail,
        },
        Input(&input),
        |_| Ok(()),
        &status,
        &gate,
    )
    .unwrap();
    ui.bind_health(&Health).unwrap();
    for _ in 0..5 {
        input.set(Some(Event::Left));
        ui.step(0).unwrap();
    }
    for n in 16..6400 {
        ui.step(n).unwrap();
    }
    assert_eq!(ui.model().screen(), Screen::Pressure);
    input.set(Some(Event::KeyRelease));
    ui.step(400).unwrap();
    for _ in 0..2 {
        input.set(Some(Event::Left));
        ui.step(401).unwrap();
    }
    for n in 6432..12800 {
        ui.step(n).unwrap();
    }
    assert_eq!(ui.model().screen(), Screen::Heart);
    use device_ui::graphics::{pixelcolor::Rgb565 as Color, prelude::*};
    struct Raster(Vec<Rgb565>);
    impl OriginDimensions for Raster {
        fn size(&self) -> Size {
            Size::new(128, 128)
        }
    }
    impl DrawTarget for Raster {
        type Color = Color;
        type Error = ();
        fn draw_iter<T: IntoIterator<Item = Pixel<Color>>>(&mut self, pixels: T) -> Result<(), ()> {
            for Pixel(p, c) in pixels {
                let x = usize::try_from(p.x).map_err(|_| ())?;
                let y = usize::try_from(p.y).map_err(|_| ())?;
                self.0[y * 128 + x] = Rgb565(c.into_storage());
            }
            Ok(())
        }
    }
    let data = wattch5_parity_ui_demo::Snapshot::unavailable();
    let mut model = wattch5_parity_ui_demo::Model::default();
    model.preview(Screen::Heart, &data).unwrap();
    let mut expected = Raster(vec![Rgb565(0); 16384]);
    model
        .product_scene(&data)
        .unwrap()
        .draw(&mut expected)
        .unwrap();
    assert_eq!(
        *frame.borrow(),
        expected.0,
        "the compact pressure/oxygen/glucose cache cannot populate an unbound kind"
    );
}

#[test]
fn test_should_render_full_product_v2_through_native_moteframe_color_path() {
    struct NativeOnly<'a>(&'a Cell<usize>);
    impl Display for NativeOnly<'_> {
        type Error = ();
        fn dimensions(&self) -> Dimensions {
            Dimensions::new(128, 128)
        }
        fn write(&mut self, _: Rect, _: &[Rgb565]) -> Result<(), ()> {
            Err(())
        }
        fn write_rgb565(
            &mut self,
            r: Rect,
            p: &[device_ui::graphics::pixelcolor::Rgb565],
        ) -> Result<(), device_core::NativeWriteError<()>> {
            assert_eq!(r.height, 1);
            assert_eq!(p.len(), usize::from(r.width));
            assert_eq!(r.width, 32);
            self.0.set(self.0.get() + 1);
            Ok(())
        }
    }
    let writes = Cell::new(0);
    let input = Cell::new(None);
    let status = Cell::new(Readiness::new());
    let gate = gate();
    let mut ui = V2Ui::new(
        NativeOnly(&writes),
        Input(&input),
        |_| Ok(()),
        &status,
        &gate,
    )
    .unwrap();
    for _ in 0..512 {
        assert_eq!(ui.step(0), Ok(()));
    }
    assert_eq!(ui.frames(), 1);
    assert_eq!(writes.get(), 512);
}

#[test]
fn test_should_bound_settings_validation_to_row_boundaries_while_painting() {
    let input = Cell::new(None);
    let status = Cell::new(Readiness::new());
    let gate = gate();
    let frame = RefCell::new(vec![Rgb565(0); 16384]);
    let writes = Cell::new(0);
    let fail = Cell::new(false);
    let settings = Settings::new();
    let mut ui = V2Ui::new(
        Panel {
            frame: &frame,
            writes: &writes,
            fail: &fail,
        },
        Input(&input),
        |_| Ok(()),
        &status,
        &gate,
    )
    .unwrap();
    ui.bind(&settings).unwrap();
    for now in 0..2048 {
        if ui.frames() == 1 {
            break;
        }
        ui.step(now).unwrap();
    }
    assert_eq!(ui.frames(), 1);
    assert!(
        settings.loads.get() <= 129,
        "one full frame must not revalidate the durable settings for each eight pixels: {} loads",
        settings.loads.get()
    );
    settings.actual.set(Appearance {
        locale: Locale::En,
        ..settings.actual.get()
    });
    ui.step(2048).unwrap();
    assert_eq!(
        ui.model().appearance().locale,
        Locale::En,
        "idle UI must still notice a remote settings change immediately"
    );
}

#[test]
fn test_should_finish_a_full_frame_in_512_bounded_native_chunk_turns() {
    let input = Cell::new(None);
    let status = Cell::new(Readiness::new());
    let gate = gate();
    let frame = RefCell::new(vec![Rgb565(0); 16384]);
    let writes = Cell::new(0);
    let fail = Cell::new(false);
    let mut ui = V2Ui::new(
        Panel {
            frame: &frame,
            writes: &writes,
            fail: &fail,
        },
        Input(&input),
        |_| Ok(()),
        &status,
        &gate,
    )
    .unwrap();
    for now in 0..512 {
        let before = writes.get();
        ui.step(now).unwrap();
        assert!(
            writes.get() - before <= 4,
            "a turn must yield after32 pixels"
        );
    }
    assert_eq!(ui.frames(), 1, "a frame must not need2048 scheduler turns");
    assert_eq!(
        writes.get(),
        2048,
        "all pixels must still be written exactly once"
    );
}

#[test]
fn test_should_start_the_latest_page_on_the_next_turn_without_finishing_obsolete_frame() {
    struct Coordinates<'a>(&'a RefCell<Vec<Rect>>);
    impl Display for Coordinates<'_> {
        type Error = ();
        fn dimensions(&self) -> Dimensions {
            Dimensions::new(128, 128)
        }
        fn write(&mut self, r: Rect, _: &[Rgb565]) -> Result<(), ()> {
            self.0.borrow_mut().push(r);
            Ok(())
        }
    }
    let writes = RefCell::new(Vec::new());
    let input = Cell::new(None);
    let status = Cell::new(Readiness::new());
    let gate = gate();
    let mut ui = V2Ui::new(
        Coordinates(&writes),
        Input(&input),
        |_| Ok(()),
        &status,
        &gate,
    )
    .unwrap();
    ui.step(0).unwrap();
    input.set(Some(Event::Left));
    ui.step(1).unwrap();
    assert_eq!(ui.model().screen(), Screen::Activity);
    assert_eq!(
        writes.borrow()[4],
        Rect::new(0, 0, 8, 1),
        "navigation must redraw the new page immediately, not continue the old dial"
    );
    assert_eq!(
        ui.frames(),
        0,
        "an interrupted frame is not a completed frame"
    );
    input.set(Some(Event::Left));
    ui.step(2).unwrap();
    assert_eq!(ui.model().screen(), Screen::Heart);
    assert_eq!(writes.borrow()[8], Rect::new(0, 0, 8, 1));
}

#[test]
fn test_should_submit_one_native_chunk_per_turn_and_finish_in_512_turns() {
    struct Chunks<'a>(&'a RefCell<Vec<Rect>>);
    impl Display for Chunks<'_> {
        type Error = ();
        fn dimensions(&self) -> Dimensions {
            Dimensions::new(128, 128)
        }
        fn write(&mut self, _: Rect, _: &[Rgb565]) -> Result<(), ()> {
            Err(())
        }
        fn write_rgb565(
            &mut self,
            r: Rect,
            p: &[device_ui::graphics::pixelcolor::Rgb565],
        ) -> Result<(), device_core::NativeWriteError<()>> {
            assert_eq!((r.width, r.height, p.len()), (32, 1, 32));
            self.0.borrow_mut().push(r);
            Ok(())
        }
    }
    let chunks = RefCell::new(Vec::new());
    let input = Cell::new(None);
    let status = Cell::new(Readiness::new());
    let gate = gate();
    let mut ui = V2Ui::new(Chunks(&chunks), Input(&input), |_| Ok(()), &status, &gate).unwrap();
    for n in 0..512 {
        let before = chunks.borrow().len();
        ui.step(n).unwrap();
        assert_eq!(
            chunks.borrow().len() - before,
            1,
            "yield to Controller after one native32-pixel scene render/write"
        );
    }
    assert_eq!(ui.frames(), 1);
    assert_eq!(
        *chunks.borrow(),
        (0..512)
            .map(|n| Rect::new((n % 4) * 32, n / 4, 32, 1))
            .collect::<Vec<_>>()
    );
}

#[test]
fn test_should_bound_idle_provider_reads_while_servicing_inputs_immediately() {
    let input = Cell::new(None);
    let status = Cell::new(Readiness::new());
    let gate = gate();
    let frame = RefCell::new(vec![Rgb565(0); 16384]);
    let writes = Cell::new(0);
    let fail = Cell::new(false);
    let settings = Settings::new();
    let mut ui = V2Ui::new(
        Panel {
            frame: &frame,
            writes: &writes,
            fail: &fail,
        },
        Input(&input),
        |_| Ok(()),
        &status,
        &gate,
    )
    .unwrap();
    ui.bind(&settings).unwrap();
    for n in 0..512 {
        ui.step(n).unwrap();
    }
    assert_eq!(ui.frames(), 1);
    let before = settings.loads.get();
    for n in 512..572 {
        ui.step(n).unwrap();
    }
    assert!(
        settings.loads.get() - before <= 3,
        "idle foreground turns must not re-read durable providers continuously"
    );
    input.set(Some(Event::Left));
    ui.step(572).unwrap();
    assert_eq!(
        ui.model().screen(),
        Screen::Activity,
        "input navigation does not wait for provider cadence"
    );
}

#[test]
fn test_should_bound_idle_health_reads_and_keep_the_last_confirmed_pixels() {
    use wattch5_candidate::ui::DeviceHealth;
    use wattch5_parity_ui_demo::{MeasureKind, MeasurementSnapshot};
    struct Health(Cell<usize>);
    impl DeviceHealth for Health {
        fn page(&self, _: MeasureKind) -> Result<MeasurementSnapshot, PortError> {
            self.0.set(self.0.get() + 1);
            Ok(MeasurementSnapshot::default())
        }
        fn request(&self, _: MeasureKind, _: bool) -> Result<(), PortError> {
            Err(PortError::Unavailable)
        }
    }
    let input = Cell::new(None);
    let status = Cell::new(Readiness::new());
    let gate = gate();
    let frame = RefCell::new(vec![Rgb565(0); 16384]);
    let writes = Cell::new(0);
    let fail = Cell::new(false);
    let health = Health(Cell::new(0));
    let mut ui = V2Ui::new(
        Panel {
            frame: &frame,
            writes: &writes,
            fail: &fail,
        },
        Input(&input),
        |_| Ok(()),
        &status,
        &gate,
    )
    .unwrap();
    ui.bind_health(&health).unwrap();
    for n in 0..6 {
        input.set(Some(Event::Left));
        ui.step(n).unwrap();
    }
    for n in 6..600 {
        ui.step(n).unwrap();
    }
    assert_eq!(ui.model().screen(), Screen::Oxygen);
    assert!(ui.frames() > 0);
    let before = health.0.get();
    let painted = writes.get();
    for n in 600..660 {
        ui.step(n).unwrap();
    }
    assert!(
        health.0.get() - before <= 3,
        "stable health pages cannot reread Flash on every foreground turn"
    );
    assert_eq!(
        writes.get(),
        painted,
        "unchanged readings preserve the confirmed display without redraws"
    );
}
#[test]
fn test_should_observe_remote_appearance_within_provider_cadence() {
    let input = Cell::new(None);
    let status = Cell::new(Readiness::new());
    let gate = gate();
    let frame = RefCell::new(vec![Rgb565(0); 16384]);
    let writes = Cell::new(0);
    let fail = Cell::new(false);
    let settings = Settings::new();
    let mut ui = V2Ui::new(
        Panel {
            frame: &frame,
            writes: &writes,
            fail: &fail,
        },
        Input(&input),
        |_| Ok(()),
        &status,
        &gate,
    )
    .unwrap();
    ui.bind(&settings).unwrap();
    for n in 0..512 {
        ui.step(n).unwrap();
    }
    settings.actual.set(Appearance {
        locale: Locale::En,
        ..settings.actual.get()
    });
    ui.step(519).unwrap();
    assert_eq!(ui.model().appearance().locale, Locale::Zh);
    ui.step(520).unwrap();
    assert_eq!(ui.model().appearance().locale, Locale::En);
}

#[derive(Debug)]
struct Background(Cell<bool>);
impl wattch5_candidate::v2_ui::Background for Background {
    fn row(
        &self,
        m: &wattch5_parity_ui_demo::Model,
        y: u16,
        out: &mut [moteframe_ui::graphics::pixelcolor::Rgb565; 128],
    ) -> Result<(), PortError> {
        if self.0.replace(false) {
            return Err(PortError::Unavailable);
        }
        use moteframe_ui::graphics::prelude::*;
        #[derive(Debug)]
        struct Row<'a> {
            y: u16,
            out: &'a mut [moteframe_ui::graphics::pixelcolor::Rgb565; 128],
        }
        impl Dimensions for Row<'_> {
            fn bounding_box(&self) -> moteframe_ui::graphics::primitives::Rectangle {
                moteframe_ui::graphics::primitives::Rectangle::new(
                    Point::new(0, i32::from(self.y)),
                    Size::new(128, 1),
                )
            }
        }
        impl DrawTarget for Row<'_> {
            type Color = moteframe_ui::graphics::pixelcolor::Rgb565;
            type Error = ();
            fn draw_iter<T: IntoIterator<Item = Pixel<Self::Color>>>(
                &mut self,
                p: T,
            ) -> Result<(), ()> {
                for Pixel(at, c) in p {
                    assert_eq!(at.y, i32::from(self.y));
                    self.out[at.x as usize] = c;
                }
                Ok(())
            }
        }
        m.draw_background(&mut Row { y, out })
            .map_err(|_| PortError::Fault)
    }
}
#[test]
fn test_should_paint_image_rows_in_128_turns_yield_on_storage_busy_and_preserve_navigation() {
    let input = Cell::new(None);
    let status = Cell::new(Readiness::new());
    let gate = gate();
    let frame = RefCell::new(vec![Rgb565(0); 16384]);
    let writes = Cell::new(0);
    let fail = Cell::new(false);
    let background = Background(Cell::new(true));
    let mut ui = V2Ui::new(
        Panel {
            frame: &frame,
            writes: &writes,
            fail: &fail,
        },
        Input(&input),
        |_| Ok(()),
        &status,
        &gate,
    )
    .unwrap();
    ui.bind_background(&background);
    ui.step(0).unwrap();
    assert_eq!(ui.next_paint_pixel(), Some(0));
    assert_eq!(writes.get(), 0);
    for n in 0..128 {
        ui.step(n).unwrap();
    }
    assert_eq!(ui.frames(), 1);
    assert_eq!(ui.next_paint_pixel(), None);
    input.set(Some(Event::Left));
    ui.step(128).unwrap();
    assert_eq!(ui.model().screen(), Screen::Activity);
    assert_eq!(ui.next_paint_pixel(), Some(128));
}

#[derive(Debug)]
struct BadDial(Background);
impl wattch5_candidate::v2_ui::Background for BadDial {
    fn row(
        &self,
        m: &wattch5_parity_ui_demo::Model,
        y: u16,
        out: &mut [moteframe_ui::graphics::pixelcolor::Rgb565; 128],
    ) -> Result<(), PortError> {
        if m.screen() == Screen::Dial {
            return Err(PortError::Fault);
        }
        wattch5_candidate::v2_ui::Background::row(&self.0, m, y, out)
    }
}
#[test]
fn test_should_leave_a_broken_image_and_paint_a_valid_destination_after_navigation() {
    let input = Cell::new(None);
    let status = Cell::new(Readiness::new());
    let gate = gate();
    let frame = RefCell::new(vec![Rgb565(0); 16384]);
    let writes = Cell::new(0);
    let fail = Cell::new(false);
    let background = BadDial(Background(Cell::new(false)));
    let mut ui = V2Ui::new(
        Panel {
            frame: &frame,
            writes: &writes,
            fail: &fail,
        },
        Input(&input),
        |_| Ok(()),
        &status,
        &gate,
    )
    .unwrap();
    ui.bind_background(&background);
    assert_eq!(ui.step(0), Err(PortError::Fault));
    assert_eq!(writes.get(), 0);
    input.set(Some(Event::Left));
    ui.step(1).unwrap();
    assert_eq!(ui.model().screen(), Screen::Activity);
    assert_eq!(ui.next_paint_pixel(), Some(128));
    for n in 2..129 {
        ui.step(n).unwrap();
    }
    assert_eq!(ui.frames(), 1);
}
