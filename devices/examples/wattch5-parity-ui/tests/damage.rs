//! Marked damage must reconstruct the complete reference image, including erasure.
use device_core::Rect;
use device_ui::graphics::{pixelcolor::Rgb565, prelude::*, primitives::Rectangle};
use wattch5_parity_ui_demo::*;
#[derive(Clone, Debug)]
struct Raster {
    pixels: Vec<u16>,
    clip: Rectangle,
}
impl Raster {
    fn new() -> Self {
        Self {
            pixels: vec![0; 16384],
            clip: Rectangle::new(Point::zero(), Size::new(128, 128)),
        }
    }
}
impl Dimensions for Raster {
    fn bounding_box(&self) -> Rectangle {
        self.clip
    }
}
impl DrawTarget for Raster {
    type Color = Rgb565;
    type Error = ();
    fn draw_iter<I: IntoIterator<Item = Pixel<Rgb565>>>(&mut self, i: I) -> Result<(), ()> {
        for Pixel(p, c) in i {
            assert!(self.clip.contains(p));
            self.pixels[usize::try_from(p.y).map_err(|_| ())? * 128
                + usize::try_from(p.x).map_err(|_| ())?] = c.into_storage();
        }
        Ok(())
    }
}
fn check(a: &Model, x: &Snapshot<'_>, b: &Model, y: &Snapshot<'_>) -> Option<Rect> {
    let old = a.scene(x).unwrap_or_else(|e| panic!("{e:?}"));
    let new = b.scene(y).unwrap_or_else(|e| panic!("{e:?}"));
    let area = new.damage_from(Some(&old));
    let mut before = Raster::new();
    old.draw(&mut before).unwrap_or_else(|e| panic!("{e:?}"));
    let mut expected = Raster::new();
    new.draw(&mut expected).unwrap_or_else(|e| panic!("{e:?}"));
    if let Some(r) = area {
        assert!(r.fits(device_core::Dimensions::new(128, 128)));
        before.clip = Rectangle::new(
            Point::new(i32::from(r.x), i32::from(r.y)),
            Size::new(u32::from(r.width), u32::from(r.height)),
        );
        new.draw(&mut before).unwrap_or_else(|e| panic!("{e:?}"));
    }
    assert_eq!(
        before.pixels,
        expected.pixels,
        "damage {area:?} on {:?}",
        b.screen()
    );
    area
}
#[test]
fn test_should_mark_clock_and_battery_without_full_frame_diff() {
    let m = Model::default();
    let a = Snapshot::default();
    let mut b = a;
    b.minute = 51;
    assert_eq!(check(&m, &a, &m, &b), Some(Rect::new(8, 74, 66, 42)));
    b = a;
    b.battery = 9;
    assert_eq!(check(&m, &a, &m, &b), Some(Rect::new(90, 96, 36, 12)));
    assert_eq!(check(&m, &a, &m, &a), None);
}
#[test]
fn test_should_clear_shrinking_progress_and_fall_back_for_appearance() {
    let mut model = Model::default();
    let mut before = Snapshot::default();
    model
        .preview(Screen::Ota, &before)
        .unwrap_or_else(|e| panic!("{e:?}"));
    before.progress = 100;
    let mut after = before;
    after.progress = 1;
    let region = check(&model, &before, &model, &after).unwrap_or_else(|| panic!("missing damage"));
    assert!(region.area() < 16384);
    let mut changed = model;
    changed
        .set_appearance(Appearance {
            theme: Theme::Ice,
            ..model.appearance()
        })
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        check(&model, &before, &changed, &after),
        Some(Rect::new(0, 0, 128, 128))
    );
}
#[test]
fn test_should_include_am_pm_when_hour_crosses_noon() {
    let mut m = Model::default();
    m.set_appearance(Appearance {
        format: TimeFormat::Hour12,
        ..m.appearance()
    })
    .unwrap_or_else(|e| panic!("{e:?}"));
    let a = Snapshot::default();
    let mut b = a;
    b.hour = 14;
    check(&m, &a, &m, &b);
}
#[test]
fn test_should_cover_snapshot_and_input_mutations_in_every_skin() {
    let base = Snapshot::default();
    let variants = data_variants(base);
    let mut cases = 0;
    for page in Screen::ALL {
        for locale in [Locale::Zh, Locale::En] {
            for theme in [Theme::Phosphor, Theme::Amber, Theme::Ice] {
                let mut m = Model::default();
                m.set_appearance(Appearance {
                    locale,
                    theme,
                    ..Appearance::default()
                })
                .unwrap_or_else(|e| panic!("{e:?}"));
                m.preview(page, &base).unwrap_or_else(|e| panic!("{e:?}"));
                for data in &variants {
                    check(&m, &base, &m, data);
                    check(&m, data, &m, &base);
                    cases += 2;
                }
                for event in [
                    Event::Up,
                    Event::Down,
                    Event::Left,
                    Event::Right,
                    Event::Select,
                    Event::Tap { x: 64, y: 63 },
                    Event::Tap { x: 64, y: 96 },
                ] {
                    let mut n = m;
                    if n.handle(event, &base).is_ok() {
                        check(&m, &base, &n, &base);
                        cases += 1;
                    }
                }
            }
        }
    }
    assert!(cases > 12000, "only {cases} transitions");
}
#[test]
fn test_should_cover_each_face_format_and_editor_draft() {
    let a = Snapshot::default();
    for face in [Face::Stack, Face::Line, Face::Segment] {
        for format in [TimeFormat::Hour12, TimeFormat::Hour24] {
            for locale in [Locale::Zh, Locale::En] {
                let mut m = Model::default();
                m.set_appearance(Appearance {
                    face,
                    format,
                    locale,
                    ..Appearance::default()
                })
                .unwrap_or_else(|e| panic!("{e:?}"));
                for (hour, minute, battery) in [(0, 0, 0), (11, 59, 100), (12, 0, 9), (23, 59, 1)] {
                    let mut b = a;
                    b.hour = hour;
                    b.minute = minute;
                    b.battery = battery;
                    check(&m, &a, &m, &b);
                    check(&m, &b, &m, &a);
                }
            }
        }
    }
    for page in [
        Screen::Language,
        Screen::Brightness,
        Screen::Textsize,
        Screen::Format,
        Screen::Theme,
        Screen::Face,
    ] {
        let mut m = Model::default();
        m.preview(page, &a).unwrap_or_else(|e| panic!("{e:?}"));
        for _ in 0..10 {
            let old = m;
            let e = if page == Screen::Face {
                Event::Left
            } else {
                Event::Up
            };
            m.handle(e, &a).unwrap_or_else(|e| panic!("{e:?}"));
            check(&old, &a, &m, &a);
        }
    }
}
#[test]
fn test_should_cover_individual_date_fields_in_every_clock_layout() {
    let a = Snapshot::default();
    for face in [Face::Stack, Face::Line, Face::Segment] {
        for locale in [Locale::Zh, Locale::En] {
            let mut m = Model::default();
            m.set_appearance(Appearance {
                face,
                locale,
                ..m.appearance()
            })
            .unwrap_or_else(|e| panic!("{e:?}"));
            let mut b = a;
            b.month = 1;
            check(&m, &a, &m, &b);
            b = a;
            b.day = 1;
            check(&m, &a, &m, &b);
            b = a;
            b.weekday = Weekday::Sun;
            check(&m, &a, &m, &b);
        }
    }
}

fn data_variants(base: Snapshot<'static>) -> Vec<Snapshot<'static>> {
    let mut variants = Vec::new();
    macro_rules! variant {
        ($x:ident,$body:block) => {
            let mut $x = base;
            $body;
            variants.push($x);
        };
    }
    variant!(d, {
        d.hour = 0;
        d.minute = 0;
        d.battery = 100;
        d.month = 1;
        d.day = 1;
        d.weekday = Weekday::Sun;
    });
    variant!(d, {
        d.hour = 23;
        d.minute = 59;
        d.battery = 9;
        d.month = 12;
        d.day = 31;
    });
    variant!(d, {
        d.measurements = [MeasurementSnapshot {
            state: MeasureState::Result,
            reading: Reading::Number(99),
        }; 6];
        d.measurements[1].reading = Reading::Pressure {
            upper: 120,
            lower: 80,
        };
        d.measurements[3].reading = Reading::Tenth(365);
        d.measurements[4].reading = Reading::Tenth(56);
        d.measurements[5].reading = Reading::Tenth(48);
    });
    variant!(d, {
        d.measurements = [MeasurementSnapshot {
            state: MeasureState::Unavailable,
            reading: Reading::Missing,
        }; 6];
    });
    variant!(d, {
        d.heart_high = Some(999);
        d.heart_low = Some(1);
    });
    variant!(d, {
        d.progress = 100;
        d.charging = 100;
    });
    variant!(d, {
        d.progress = 1;
        d.charging = 1;
    });
    variant!(d, {
        d.timer.seconds = 359_999;
        d.timer.state = TimerState::Running;
    });
    variant!(d, {
        d.timer.state = TimerState::Unavailable;
        d.stopwatch.state = StopwatchState::Unavailable;
    });
    variant!(d, {
        d.stopwatch.centis = 599_999;
        d.stopwatch.laps = &[1234; 20];
    });
    variant!(d, {
        d.stopwatch.laps = &[];
    });
    variant!(d, {
        d.messages = &[];
    });
    variant!(d, {
        d.messages = &[Message {
            zh: "心率",
            en: "A",
        }];
    });
    variant!(d, {
        d.weather = Some(WeatherSnapshot {
            low: -99,
            high: 99,
            condition: WeatherCondition::Snow,
        });
    });
    variant!(d, {
        d.steps = 99_999;
        d.distance = 99_999;
        d.calories = 99_999;
        d.sleep_total = Some(5999);
        d.sleep_deep = Some(10);
        d.sleep_light = Some(2);
    });
    variant!(d, {
        d.sport.phase = SportPhase::Done;
        d.sport.elapsed_seconds = 359_999;
        d.sport.heart = Some(99);
        d.sport.locked = true;
    });
    variants
}
