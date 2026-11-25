//! Product rendering must never promote unobserved scalar defaults into readings.
use device_core::Rect;
use device_ui::graphics::{pixelcolor::Rgb565, prelude::*};
use wattch5_parity_ui_demo::*;
#[derive(Debug)]
struct Raster(Vec<u16>);
impl OriginDimensions for Raster {
    fn size(&self) -> Size {
        Size::new(128, 128)
    }
}
impl DrawTarget for Raster {
    type Color = Rgb565;
    type Error = ();
    fn draw_iter<I: IntoIterator<Item = Pixel<Rgb565>>>(&mut self, pixels: I) -> Result<(), ()> {
        for Pixel(p, c) in pixels {
            assert!((0..128).contains(&p.x) && (0..128).contains(&p.y));
            self.0[usize::try_from(p.y).map_err(|_| ())? * 128
                + usize::try_from(p.x).map_err(|_| ())?] = c.into_storage();
        }
        Ok(())
    }
}
fn draw(model: &Model, data: &Snapshot<'_>) -> Vec<u16> {
    let mut raster = Raster(vec![0; 16384]);
    model
        .product_scene(data)
        .unwrap_or_else(|e| panic!("{e:?}"))
        .draw(&mut raster)
        .unwrap_or_else(|e| panic!("page {:?}: {e:?}", model.screen()));
    raster.0
}
#[test]
fn test_should_select_product_presentation_at_compile_time_without_runtime_mode_storage() {
    let model = Model::default();
    let data = Snapshot::unavailable();
    let product: Scene<'_, '_, true> = model
        .product_scene(&data)
        .unwrap_or_else(|e| panic!("{e:?}"));
    let demo: Scene<'_, '_> = model.scene(&data).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        core::mem::size_of_val(&product),
        core::mem::size_of_val(&demo)
    );
    assert_eq!(
        product.damage_from(Some(&demo)),
        Some(Rect::new(0, 0, 128, 128))
    );
    assert_eq!(
        product.damage_from::<true>(None),
        Some(Rect::new(0, 0, 128, 128))
    );
}
#[test]
fn test_should_start_product_without_demo_records_or_fake_service_phases() {
    let data = Snapshot::unavailable();
    data.validate().unwrap_or_else(|e| panic!("{e:?}"));
    assert!(data.messages.is_empty());
    assert!(data.stopwatch.laps.is_empty());
    assert_eq!(data.timer.state, TimerState::Unavailable);
    assert_eq!(data.stopwatch.state, StopwatchState::Unavailable);
    assert!(
        data.measurements
            .iter()
            .all(|v| v.state == MeasureState::Unavailable && v.reading == Reading::Missing)
    );
}
#[test]
fn test_should_mask_unbound_clock_activity_workout_and_update_scalars() {
    let mut a = Snapshot::unavailable();
    // Keep workout layouts selected while exercising missing scalar rendering.
    a.sport.phase = SportPhase::Countdown;
    let mut b = a;
    b.hour = 23;
    b.minute = 59;
    b.month = 12;
    b.day = 31;
    b.weekday = Weekday::Sun;
    b.battery = 99;
    b.steps = 99999;
    b.distance = 12345;
    b.calories = 56789;
    b.charging = 88;
    b.progress = 75;
    b.sport.countdown = 3;
    b.sport.elapsed_seconds = 1234;
    b.sport.distance = 1234;
    b.sport.calories = 5678;
    for page in [
        Screen::Dial,
        Screen::Face,
        Screen::Activity,
        Screen::Weather,
        Screen::SportCountdown,
        Screen::SportRun,
        Screen::Charge,
        Screen::Ota,
        Screen::Applet,
    ] {
        for face in [Face::Stack, Face::Line, Face::Segment] {
            let mut model = Model::default();
            model
                .set_appearance(Appearance {
                    face,
                    ..Appearance::default()
                })
                .unwrap_or_else(|e| panic!("{e:?}"));
            model.preview(page, &a).unwrap_or_else(|e| panic!("{e:?}"));
            assert!(draw(&model, &a) == draw(&model, &b), "{page:?} {face:?}");
        }
    }
}
#[test]
fn test_should_preserve_actual_health_observations_and_remove_demo_chrome() {
    let mut data = Snapshot::unavailable();
    let mut model = Model::default();
    model
        .preview(Screen::Pressure, &data)
        .unwrap_or_else(|e| panic!("{e:?}"));
    let missing = draw(&model, &data);
    data.measurements[MeasureKind::Pressure as usize] = MeasurementSnapshot {
        state: MeasureState::Result,
        reading: Reading::Pressure {
            upper: 123,
            lower: 81,
        },
    };
    let result = draw(&model, &data);
    assert_ne!(missing, result);
    // The old demo label occupied this corner; product header leaves it blank.
    for y in 0..18 {
        for x in 97..125 {
            assert_eq!(result[y * 128 + x], 129);
        }
    }
    let old = model.scene(&data).unwrap_or_else(|e| panic!("{e:?}"));
    let new = model
        .product_scene(&data)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(new.damage_from(Some(&old)), Some(Rect::new(0, 0, 128, 128)));
}
#[test]
fn test_should_draw_all_product_scenes_and_bilingual_labels_with_missing_data() {
    let mut data = Snapshot::unavailable();
    data.sport.phase = SportPhase::Countdown;
    for page in Screen::ALL {
        for locale in [Locale::Zh, Locale::En] {
            let mut model = Model::default();
            model
                .set_appearance(Appearance {
                    locale,
                    ..Appearance::default()
                })
                .unwrap_or_else(|e| panic!("{e:?}"));
            model
                .preview(page, &data)
                .unwrap_or_else(|e| panic!("{e:?}"));
            assert_eq!(draw(&model, &data).len(), 16384);
        }
    }
}
