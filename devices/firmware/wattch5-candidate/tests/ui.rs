//! Truthful fallback UI uses actual durable settings and licensed existing glyphs.
#![allow(
    clippy::unwrap_used,
    reason = "UI test fixture assertions fail loudly."
)]
use wattch5_candidate::ui::{UiSnapshot, line};
use wattch5_parity_ui_demo::{Appearance, Locale, glyph};
#[test]
fn test_should_render_chinese_default_without_fabricated_measurements() {
    let s = UiSnapshot::new(Appearance::default());
    assert_eq!(s.appearance.locale, Locale::Zh);
    for n in 0..8 {
        let (label, value) = line(s, n);
        for ch in label.chars().chain(value.chars()) {
            assert!(glyph(ch).is_ok(), "{ch}");
        }
    }
    assert!(line(s, 4).1.contains("不可用"));
    assert!(line(s, 5).1.contains("不可用"));
}

#[test]
fn test_should_render_every_english_unavailable_row_after_language_change() {
    use core::cell::Cell;
    use device_core::{Dimensions, Display, Rect, Rgb565};
    use device_runtime::{PortError, Work};
    use wattch5_bringup::{InputReport, InputSource, Readiness, ResetGate, TouchStatus};
    use wattch5_candidate::ui::{CandidateUi, SettingsPort};
    use wattch5_parity_ui_demo::Field;
    struct Panel;
    impl Display for Panel {
        type Error = ();
        fn dimensions(&self) -> Dimensions {
            Dimensions::new(128, 128)
        }
        fn write(&mut self, r: Rect, p: &[Rgb565]) -> Result<(), ()> {
            assert_eq!(r.width, 128);
            assert_eq!(p.len(), 128);
            Ok(())
        }
    }
    struct Input;
    impl InputSource for Input {
        fn sample(&mut self, _: u64) -> InputReport {
            InputReport {
                event: None,
                touch: TouchStatus::Ready,
                fault: None,
            }
        }
    }
    struct English;
    impl SettingsPort for English {
        fn load(&self) -> Result<Appearance, PortError> {
            Ok(Appearance {
                locale: Locale::En,
                ..Appearance::default()
            })
        }
        fn begin(&self, _: Field, _: u8) -> Result<(), PortError> {
            Err(PortError::Unavailable)
        }
        fn poll(&self) -> Result<bool, PortError> {
            Ok(true)
        }
        fn finish(&self) -> Result<(), PortError> {
            Ok(())
        }
    }
    let gate = ResetGate::new();
    gate.completed();
    assert!(gate.arm_ui().is_ok());
    let status = Cell::new(Readiness::new());
    let mut ui = CandidateUi::new(Panel, Input, |_| Ok(()), &status, &gate).unwrap();
    ui.bind(&English).unwrap();
    for row in 0..256 {
        assert!(ui.step(row).is_ok(), "failedrow{row}");
    }
}

mod health {
    use core::cell::{Cell, RefCell};
    use device_core::{Dimensions, Display, Rect, Rgb565};
    use device_runtime::{PortError, Work};
    use wattch5_bringup::{Event, InputReport, InputSource, Readiness, ResetGate, TouchStatus};
    use wattch5_candidate::ui::{CandidateUi, DeviceHealth, UiPage, health_value};
    use wattch5_parity_ui_demo::{MeasureKind, MeasureState, MeasurementSnapshot, Reading};

    struct Panel<'a>(Option<&'a RefCell<Vec<Rgb565>>>);
    impl Display for Panel<'_> {
        type Error = ();
        fn dimensions(&self) -> Dimensions {
            Dimensions::new(128, 128)
        }
        fn write(&mut self, r: Rect, p: &[Rgb565]) -> Result<(), ()> {
            assert_eq!(r.width, 128);
            assert_eq!(r.height, 1);
            assert_eq!(p.len(), 128);
            if let Some(frame) = self.0 {
                let offset = usize::from(r.y) * 128;
                frame.borrow_mut()[offset..offset + 128].copy_from_slice(p);
            }
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
    struct Health {
        page: Cell<[MeasurementSnapshot; 3]>,
        calls: RefCell<Vec<(MeasureKind, bool)>>,
        fail: Cell<bool>,
    }
    impl DeviceHealth for Health {
        fn page(&self, k: MeasureKind) -> Result<MeasurementSnapshot, PortError> {
            let index = match k {
                MeasureKind::Pressure => 0,
                MeasureKind::Oxygen => 1,
                MeasureKind::Glucose => 2,
                _ => return Err(PortError::InvalidInput),
            };
            Ok(self.page.get()[index])
        }
        fn request(&self, k: MeasureKind, start: bool) -> Result<(), PortError> {
            self.calls.borrow_mut().push((k, start));
            if self.fail.get() {
                Err(PortError::Unavailable)
            } else {
                Ok(())
            }
        }
    }
    impl Health {
        fn set(&self, kind: MeasureKind, value: MeasurementSnapshot) {
            let index = match kind {
                MeasureKind::Pressure => 0,
                MeasureKind::Oxygen => 1,
                MeasureKind::Glucose => 2,
                _ => panic!("unsupported fixture kind"),
            };
            let mut pages = self.page.get();
            pages[index] = value;
            self.page.set(pages);
        }
    }
    fn fixture() -> Health {
        Health {
            page: Cell::new([MeasurementSnapshot::default(); 3]),
            calls: RefCell::new(Vec::new()),
            fail: Cell::new(false),
        }
    }

    #[test]
    fn touch_reaches_each_page_routes_start_stop_and_keeps_imported_value() {
        let input = Cell::new(None);
        let status = Cell::new(Readiness::new());
        let gate = ResetGate::new();
        gate.completed();
        gate.arm_ui().unwrap();
        let health = fixture();
        let mut ui =
            CandidateUi::new(Panel(None), Input(&input), |_| Ok(()), &status, &gate).unwrap();
        ui.bind_health(&health).unwrap();
        for (row, page, kind, reading) in [
            (
                4,
                UiPage::Pressure,
                MeasureKind::Pressure,
                Reading::Pressure {
                    upper: 120,
                    lower: 80,
                },
            ),
            (5, UiPage::Oxygen, MeasureKind::Oxygen, Reading::Number(98)),
            (6, UiPage::Glucose, MeasureKind::Glucose, Reading::Tenth(54)),
        ] {
            input.set(Some(Event::Tap { x: 20, y: row * 16 }));
            ui.step(0).unwrap();
            assert_eq!(ui.view().page, page);
            input.set(Some(Event::Tap { x: 20, y: 96 }));
            ui.step(1).unwrap();
            assert_eq!(health.calls.borrow().last(), Some(&(kind, true)));
            health.set(
                kind,
                MeasurementSnapshot {
                    state: MeasureState::Measuring,
                    reading: Reading::Missing,
                },
            );
            ui.step(2).unwrap();
            input.set(Some(Event::Tap { x: 20, y: 96 }));
            ui.step(3).unwrap();
            assert_eq!(health.calls.borrow().last(), Some(&(kind, false)));
            // Completion/import is owned by the background log after the page stopped.
            health.set(
                kind,
                MeasurementSnapshot {
                    state: MeasureState::Result,
                    reading,
                },
            );
            for n in 4..390 {
                ui.step(n).unwrap();
            }
            assert_eq!(ui.view().measurement.reading, reading);
            input.set(Some(Event::Tap { x: 20, y: 112 }));
            ui.step(400).unwrap();
            assert_eq!(ui.view().page, UiPage::Settings);
        }
        assert_eq!(health.calls.borrow().len(), 6);
    }

    #[test]
    fn leaving_pending_page_sends_stop_once_without_discarding_background_source() {
        let input = Cell::new(Some(Event::Left));
        let status = Cell::new(Readiness::new());
        let gate = ResetGate::new();
        gate.completed();
        gate.arm_ui().unwrap();
        let health = fixture();
        let mut ui =
            CandidateUi::new(Panel(None), Input(&input), |_| Ok(()), &status, &gate).unwrap();
        ui.bind_health(&health).unwrap();
        ui.step(0).unwrap();
        input.set(Some(Event::Tap { x: 20, y: 96 }));
        ui.step(1).unwrap();
        health.set(
            MeasureKind::Pressure,
            MeasurementSnapshot {
                state: MeasureState::Pending,
                reading: Reading::Missing,
            },
        );
        input.set(Some(Event::Tap { x: 20, y: 112 }));
        ui.step(2).unwrap();
        assert_eq!(
            &*health.calls.borrow(),
            &[
                (MeasureKind::Pressure, true),
                (MeasureKind::Pressure, false)
            ]
        );
        assert_eq!(ui.view().page, UiPage::Settings);
        health.set(
            MeasureKind::Pressure,
            MeasurementSnapshot {
                state: MeasureState::Result,
                reading: Reading::Pressure {
                    upper: 118,
                    lower: 78,
                },
            },
        );
        input.set(Some(Event::Left));
        ui.step(3).unwrap();
        assert_eq!(
            ui.view().measurement.reading,
            Reading::Pressure {
                upper: 118,
                lower: 78
            }
        );
        assert_eq!(health.calls.borrow().len(), 2);
    }

    #[test]
    fn unavailable_start_preserves_actual_reading_and_marks_visible_error() {
        let input = Cell::new(Some(Event::Left));
        let status = Cell::new(Readiness::new());
        let gate = ResetGate::new();
        gate.completed();
        gate.arm_ui().unwrap();
        let health = fixture();
        let reading = Reading::Pressure {
            upper: 123,
            lower: 81,
        };
        health.set(
            MeasureKind::Pressure,
            MeasurementSnapshot {
                state: MeasureState::Result,
                reading,
            },
        );
        health.fail.set(true);
        let mut ui =
            CandidateUi::new(Panel(None), Input(&input), |_| Ok(()), &status, &gate).unwrap();
        ui.bind_health(&health).unwrap();
        ui.step(0).unwrap();
        input.set(Some(Event::Tap { x: 20, y: 96 }));
        assert_eq!(ui.step(1), Err(PortError::Unavailable));
        assert!(ui.view().health_error);
        assert_eq!(ui.view().measurement.reading, reading);
        for n in 2..390 {
            ui.step(n).unwrap();
        }
        assert!(ui.view().health_error);
        health.set(
            MeasureKind::Pressure,
            MeasurementSnapshot {
                state: MeasureState::Result,
                reading: Reading::Pressure {
                    upper: 124,
                    lower: 82,
                },
            },
        );
        ui.step(400).unwrap();
        assert!(!ui.view().health_error);
    }

    #[test]
    fn health_numbers_are_typed_formatted_without_origin_and_reject_unrenderable_bp() {
        assert_eq!(
            health_value(
                MeasureKind::Pressure,
                Reading::Pressure {
                    upper: 120,
                    lower: 80
                }
            )
            .unwrap()
            .as_str(),
            "120/80"
        );
        assert_eq!(
            health_value(MeasureKind::Oxygen, Reading::Number(98))
                .unwrap()
                .as_str(),
            "98"
        );
        assert_eq!(
            health_value(MeasureKind::Glucose, Reading::Tenth(54))
                .unwrap()
                .as_str(),
            "5.4"
        );
        assert_eq!(
            health_value(MeasureKind::Glucose, Reading::Tenth(6554))
                .unwrap()
                .as_str(),
            "655.4"
        );
        assert_eq!(
            health_value(
                MeasureKind::Pressure,
                Reading::Pressure {
                    upper: 1000,
                    lower: 80
                }
            ),
            Err(PortError::InvalidInput)
        );
        assert_eq!(
            health_value(MeasureKind::Oxygen, Reading::Tenth(980)),
            Err(PortError::InvalidInput)
        );
    }
    #[test]
    fn each_bilingual_health_frame_contains_actual_numeric_value_and_units() {
        use wattch5_candidate::ui::SettingsPort;
        use wattch5_parity_ui_demo::{Appearance, Field, Label, Locale, glyph};
        struct Settings(Locale);
        impl SettingsPort for Settings {
            fn load(&self) -> Result<Appearance, PortError> {
                Ok(Appearance {
                    locale: self.0,
                    ..Appearance::default()
                })
            }
            fn begin(&self, _: Field, _: u8) -> Result<(), PortError> {
                Err(PortError::Unavailable)
            }
            fn poll(&self) -> Result<bool, PortError> {
                Ok(true)
            }
            fn finish(&self) -> Result<(), PortError> {
                Ok(())
            }
        }
        fn assert_text(frame: &[Rgb565], line: usize, text: &str) {
            for row in 0..12 {
                let mut expected = [Rgb565(0); 128];
                let mut x = 1;
                for ch in text.chars() {
                    let g = glyph(ch).unwrap();
                    for bit in 0..usize::from(g.width) {
                        if g.rows[row] & (1 << (usize::from(g.width) - 1 - bit)) != 0 {
                            expected[x + bit] = Rgb565(0xffff);
                        }
                    }
                    x += usize::from(g.width) + 1;
                }
                let offset = (line * 16 + row) * 128;
                assert_eq!(
                    &frame[offset..offset + 128],
                    &expected,
                    "line {line} row {row} text {text}"
                );
            }
        }
        for locale in [Locale::Zh, Locale::En] {
            let input = Cell::new(None);
            let status = Cell::new(Readiness::new());
            let gate = ResetGate::new();
            gate.completed();
            gate.arm_ui().unwrap();
            let health = fixture();
            health.set(
                MeasureKind::Pressure,
                MeasurementSnapshot {
                    state: MeasureState::Result,
                    reading: Reading::Pressure {
                        upper: 120,
                        lower: 80,
                    },
                },
            );
            health.set(
                MeasureKind::Oxygen,
                MeasurementSnapshot {
                    state: MeasureState::Result,
                    reading: Reading::Number(98),
                },
            );
            health.set(
                MeasureKind::Glucose,
                MeasurementSnapshot {
                    state: MeasureState::Result,
                    reading: Reading::Tenth(54),
                },
            );
            let frame = RefCell::new(vec![Rgb565(0); 128 * 128]);
            let settings = Settings(locale);
            let mut ui = CandidateUi::new(
                Panel(Some(&frame)),
                Input(&input),
                |_| Ok(()),
                &status,
                &gate,
            )
            .unwrap();
            ui.bind(&settings).unwrap();
            ui.bind_health(&health).unwrap();
            for n in 0..390 {
                ui.step(n).unwrap();
            }
            for (title, value, unit) in [
                (Label::Pressure, "120/80", "mmHg"),
                (Label::Oxygen, "98", "%"),
                (Label::Glucose, "5.4", "mmol/L"),
            ] {
                input.set(Some(Event::Left));
                for n in 0..390 {
                    ui.step(n).unwrap();
                }
                let pixels = frame.borrow();
                assert_text(&pixels, 0, title.text(locale));
                assert_text(&pixels, 2, value);
                assert_text(&pixels, 3, unit);
                assert_text(&pixels, 4, Label::Result.text(locale));
                assert_text(&pixels, 5, "");
                assert_text(&pixels, 6, Label::Start.text(locale));
                assert_text(&pixels, 7, Label::Back.text(locale));
            }
        }
    }

    #[test]
    fn accepted_settings_finish_in_health_page_without_health_action_or_authority_change() {
        use wattch5_candidate::ui::SettingsPort;
        use wattch5_parity_ui_demo::{Appearance, Field, TimeFormat};
        struct Settings {
            appearance: Cell<Appearance>,
            complete: Cell<bool>,
            finished: Cell<bool>,
        }
        impl SettingsPort for Settings {
            fn load(&self) -> Result<Appearance, PortError> {
                Ok(self.appearance.get())
            }
            fn begin(&self, f: Field, v: u8) -> Result<(), PortError> {
                assert_eq!(f, Field::Format);
                assert_eq!(v, 1);
                Ok(())
            }
            fn poll(&self) -> Result<bool, PortError> {
                Ok(self.complete.get())
            }
            fn finish(&self) -> Result<(), PortError> {
                self.finished.set(true);
                Ok(())
            }
        }
        let input = Cell::new(None);
        let status = Cell::new(Readiness::new());
        let gate = ResetGate::new();
        gate.completed();
        gate.arm_ui().unwrap();
        let health = fixture();
        let settings = Settings {
            appearance: Cell::new(Appearance::default()),
            complete: Cell::new(false),
            finished: Cell::new(false),
        };
        let mut ui =
            CandidateUi::new(Panel(None), Input(&input), |_| Ok(()), &status, &gate).unwrap();
        ui.bind(&settings).unwrap();
        ui.bind_health(&health).unwrap();
        input.set(Some(Event::Tap { x: 5, y: 16 }));
        ui.step(0).unwrap();
        assert_eq!(ui.view().appearance.format, TimeFormat::Hour24);
        input.set(Some(Event::Left));
        ui.step(1).unwrap();
        assert_eq!(ui.view().page, UiPage::Pressure);
        settings.appearance.set(Appearance {
            format: TimeFormat::Hour12,
            ..Appearance::default()
        });
        settings.complete.set(true);
        ui.step(2).unwrap();
        assert!(settings.finished.get());
        assert_eq!(ui.view().appearance.format, TimeFormat::Hour12);
        assert!(health.calls.borrow().is_empty());
    }
}
