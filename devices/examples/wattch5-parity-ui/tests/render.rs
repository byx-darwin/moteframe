//! Regression checks for the V2 presentation contract.
use device_ui::graphics::{pixelcolor::Rgb565, prelude::*};
use wattch5_parity_ui_demo::*;
#[derive(Debug)]
struct Raster {
    pixels: Vec<u16>,
    writes: usize,
}
impl Raster {
    fn new() -> Self {
        Self {
            pixels: vec![0; 16384],
            writes: 0,
        }
    }
}
impl OriginDimensions for Raster {
    fn size(&self) -> Size {
        Size::new(128, 128)
    }
}
impl DrawTarget for Raster {
    type Color = Rgb565;
    type Error = ();
    fn draw_iter<I: IntoIterator<Item = Pixel<Rgb565>>>(&mut self, i: I) -> Result<(), ()> {
        for Pixel(p, message_before) in i {
            assert!(p.x >= 0 && p.x < 128 && p.y >= 0 && p.y < 128);
            self.pixels[usize::try_from(p.y).map_err(|_| ())? * 128
                + usize::try_from(p.x).map_err(|_| ())?] = message_before.into_storage();
            self.writes += 1;
        }
        Ok(())
    }
}
#[test]
fn test_should_render_all_scenes_in_both_locales_and_three_themes() {
    let data = Snapshot::default();
    for page in Screen::ALL {
        for locale in [Locale::Zh, Locale::En] {
            for theme in [Theme::Phosphor, Theme::Amber, Theme::Ice] {
                let mut model = Model::default();
                model
                    .set_appearance(Appearance {
                        locale,
                        theme,
                        ..Appearance::default()
                    })
                    .unwrap_or_else(|error| panic!("{error:?}"));
                model
                    .preview(page, &data)
                    .unwrap_or_else(|error| panic!("{error:?}"));
                let mut out = Raster::new();
                model
                    .scene(&data)
                    .unwrap_or_else(|error| panic!("{error:?}"))
                    .draw(&mut out)
                    .unwrap_or_else(|error| panic!("{error:?}"));
                assert!(out.writes > 0, "{page:?}");
            }
        }
    }
}
#[test]
fn test_should_render_injected_heart_history_and_actual_chinese_message() {
    let mut model = Model::default();
    let mut data = Snapshot::default();
    model
        .preview(Screen::Heart, &data)
        .unwrap_or_else(|error| panic!("{error:?}"));
    let mut before = Raster::new();
    model
        .scene(&data)
        .unwrap_or_else(|error| panic!("{error:?}"))
        .draw(&mut before)
        .unwrap_or_else(|error| panic!("{error:?}"));
    data.heart_high = Some(150);
    data.heart_low = Some(55);
    let mut after = Raster::new();
    model
        .scene(&data)
        .unwrap_or_else(|error| panic!("{error:?}"))
        .draw(&mut after)
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_ne!(before.pixels, after.pixels);
    let one = [Message {
        zh: "心率",
        en: "HEART",
    }];
    data.messages = &one;
    model
        .preview(Screen::MessageDetail, &data)
        .unwrap_or_else(|error| panic!("{error:?}"));
    let mut message_before = Raster::new();
    model
        .scene(&data)
        .unwrap_or_else(|error| panic!("{error:?}"))
        .draw(&mut message_before)
        .unwrap_or_else(|error| panic!("{error:?}"));
    let two = [Message {
        zh: "血压",
        en: "PRESSURE",
    }];
    data.messages = &two;
    let mut error = Raster::new();
    model
        .scene(&data)
        .unwrap_or_else(|error| panic!("{error:?}"))
        .draw(&mut error)
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_ne!(message_before.pixels, error.pixels);
}
#[test]
fn test_should_reject_unsupported_text_before_drawing() {
    let bad = [Message { zh: "龘", en: "A" }];
    let data = Snapshot {
        messages: &bad,
        ..Snapshot::default()
    };
    let model = Model::default();
    assert!(model.scene(&data).is_err());
}
#[test]
fn test_should_reject_invalid_appearance_atomically() {
    let mut model = Model::default();
    let before = model;
    assert_eq!(
        model.set_appearance(Appearance {
            brightness: 5,
            ..Appearance::default()
        }),
        Err(UiError::Data)
    );
    assert_eq!(model, before);
}
#[test]
fn test_should_reject_long_message_layout_before_any_display_write() {
    let body = [Message {
        zh: "心率",
        en: "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
    }];
    let data = Snapshot {
        messages: &body,
        ..Snapshot::default()
    };
    let mut model = Model::default();
    model
        .set_appearance(Appearance {
            locale: Locale::En,
            ..Appearance::default()
        })
        .unwrap_or_else(|error| panic!("{error:?}"));
    model
        .preview(Screen::MessageDetail, &data)
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert!(model.scene(&data).is_err());
}
#[test]
fn test_should_wrap_mixed_unicode_at_character_boundaries() {
    let body = [Message {
        zh: "心率",
        en: "A心率心率心率心率心率",
    }];
    let data = Snapshot {
        messages: &body,
        ..Snapshot::default()
    };
    let mut model = Model::default();
    model
        .set_appearance(Appearance {
            locale: Locale::En,
            ..Appearance::default()
        })
        .unwrap_or_else(|error| panic!("{error:?}"));
    model
        .preview(Screen::MessageDetail, &data)
        .unwrap_or_else(|error| panic!("{error:?}"));
    let mut out = Raster::new();
    model
        .scene(&data)
        .unwrap_or_else(|error| panic!("{error:?}"))
        .draw(&mut out)
        .unwrap_or_else(|error| panic!("{error:?}"));
}
#[test]
fn test_should_prepare_valid_scene_when_caller_snapshot_shrinks() {
    let mut model = Model::default();
    let mut data = Snapshot::default();
    model
        .preview(Screen::MessageDetail, &data)
        .unwrap_or_else(|e| panic!("{e:?}"));
    model
        .handle(Event::Select, &data)
        .unwrap_or_else(|e| panic!("{e:?}"));
    data.messages = &data.messages[..1];
    let mut out = Raster::new();
    model
        .scene(&data)
        .unwrap_or_else(|e| panic!("{e:?}"))
        .draw(&mut out)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(model.message_index(), 1);
}
#[test]
fn test_should_distinguish_unavailable_timing_data_and_mask_idle_readings() {
    let mut model = Model::default();
    let mut data = Snapshot::default();
    for page in [Screen::TimerRun, Screen::Stopwatch] {
        model
            .preview(page, &data)
            .unwrap_or_else(|e| panic!("{e:?}"));
        let mut before = Raster::new();
        model
            .scene(&data)
            .unwrap_or_else(|e| panic!("{e:?}"))
            .draw(&mut before)
            .unwrap_or_else(|e| panic!("{e:?}"));
        data.timer.state = TimerState::Unavailable;
        data.stopwatch.state = StopwatchState::Unavailable;
        let mut after = Raster::new();
        model
            .scene(&data)
            .unwrap_or_else(|e| panic!("{e:?}"))
            .draw(&mut after)
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert_ne!(before.pixels, after.pixels);
        data = Snapshot::default();
    }
    model
        .preview(Screen::Heart, &data)
        .unwrap_or_else(|e| panic!("{e:?}"));
    let mut before = Raster::new();
    model
        .scene(&data)
        .unwrap_or_else(|e| panic!("{e:?}"))
        .draw(&mut before)
        .unwrap_or_else(|e| panic!("{e:?}"));
    data.measurements[0].reading = Reading::Number(999);
    let mut after = Raster::new();
    model
        .scene(&data)
        .unwrap_or_else(|e| panic!("{e:?}"))
        .draw(&mut after)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(before.pixels, after.pixels);
}
#[test]
fn test_should_keep_unavailable_readings_missing_for_every_measurement() {
    let readings = [
        Reading::Number(72),
        Reading::Pressure {
            upper: 120,
            lower: 80,
        },
        Reading::Number(98),
        Reading::Tenth(365),
        Reading::Tenth(52),
        Reading::Tenth(54),
    ];
    for (i, page) in [
        Screen::Heart,
        Screen::Pressure,
        Screen::Oxygen,
        Screen::Temperature,
        Screen::Glucose,
        Screen::Hba1c,
    ]
    .into_iter()
    .enumerate()
    {
        let mut data = Snapshot::default();
        data.measurements[i] = MeasurementSnapshot {
            state: MeasureState::Unavailable,
            reading: readings[i],
        };
        let before = data;
        let mut model = Model::default();
        model
            .preview(page, &data)
            .unwrap_or_else(|e| panic!("{e:?}"));
        let mut stale = Raster::new();
        model
            .scene(&data)
            .unwrap_or_else(|e| panic!("{e:?}"))
            .draw(&mut stale)
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(data, before);
        data.measurements[i].reading = Reading::Missing;
        let mut missing = Raster::new();
        model
            .scene(&data)
            .unwrap_or_else(|e| panic!("{e:?}"))
            .draw(&mut missing)
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(stale.pixels, missing.pixels, "{page:?}");
    }
}
