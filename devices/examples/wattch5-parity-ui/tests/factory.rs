//! Factory pixels stay multicolor, bounded and independent of the retro palette.
#![allow(clippy::unwrap_used)]
use device_ui::graphics::{pixelcolor::Rgb565, prelude::*};
use moteframe_core::ReadAt;
use wattch5_parity_ui_demo::{Effect, Event, FactoryAssets, Model, Screen, Snapshot, UiRequest};

#[derive(Debug)]
struct Source(Vec<u8>);
impl ReadAt for Source {
    type Error = ();
    fn capacity(&self) -> u32 {
        u32::try_from(self.0.len()).unwrap()
    }
    fn read_exact(&mut self, at: u32, out: &mut [u8]) -> Result<(), ()> {
        out.copy_from_slice(self.0.get(at as usize..at as usize + out.len()).ok_or(())?);
        Ok(())
    }
}
fn pack() -> Vec<u8> {
    let mut p = vec![0; 9888 + 512];
    p[..8].copy_from_slice(b"W5UIF001");
    for (at, n) in [
        (8, 1u32),
        (12, 2336),
        (16, 9376),
        (20, 9888),
        (24, 10400),
        (28, 2),
    ] {
        p[at..at + 4].copy_from_slice(&n.to_le_bytes());
    }
    let key = usize::from(Model::default().background_key());
    p[32 + key * 4..36 + key * 4].copy_from_slice(&9376u32.to_le_bytes());
    for n in 0..128 {
        p[9376 + n * 2..9378 + n * 2]
            .copy_from_slice(&(u16::try_from(n).unwrap() % 2).to_le_bytes());
    }
    for (n, c) in [0xf800u16, 0x07e0].into_iter().enumerate() {
        for pixel in p[9888 + n * 256..9888 + (n + 1) * 256].chunks_exact_mut(2) {
            pixel.copy_from_slice(&c.to_be_bytes());
        }
    }
    p
}
#[test]
fn test_should_decode_factory_colors_and_row_identity_without_retro_palette() {
    let a = FactoryAssets::open(Source(pack())).unwrap();
    let mut out = [Rgb565::new(0, 0, 0); 128];
    a.row(&Model::default(), 0, &mut out).unwrap();
    assert!(out.iter().all(|p| p.into_storage() == 0xf800));
    a.row(&Model::default(), 1, &mut out).unwrap();
    assert!(out.iter().all(|p| p.into_storage() == 0x07e0));
    assert!(a.row(&Model::default(), 128, &mut out).is_err());
}
#[test]
fn test_should_reject_bad_directory_and_row_without_advancing_output() {
    let key = usize::from(Model::default().background_key());
    for (at, raw) in [
        (32 + key * 4, 9377u32.to_le_bytes().to_vec()),
        (9376, 2u16.to_le_bytes().to_vec()),
    ] {
        let mut bytes = pack();
        bytes[at..at + raw.len()].copy_from_slice(&raw);
        let a = FactoryAssets::open(Source(bytes)).unwrap();
        let mut out = [Rgb565::new(1, 2, 3); 128];
        let before = out;
        assert!(a.row(&Model::default(), 0, &mut out).is_err());
        assert_eq!(out, before);
    }
}
#[test]
fn test_should_not_paint_retro_text_over_original_factory_menu() {
    let a = FactoryAssets::open(Source(pack())).unwrap();
    let mut out = [Rgb565::new(0, 0, 0); 128];
    a.render(&Model::default(), &Snapshot::unavailable(), 0, &mut out)
        .unwrap();
    assert!(out.iter().all(|p| p.into_storage() == 0xf800));
}
#[test]
fn test_should_map_factory_yes_left_and_cancel_right_without_reversing_actions() {
    let data = Snapshot::unavailable();
    let mut m = Model::default();
    m.preview(Screen::Clear, &data).unwrap();
    assert_eq!(
        m.handle_factory(Event::Tap { x: 20, y: 115 }, &data)
            .unwrap(),
        Effect::Request(UiRequest::ClearData)
    );
    let mut m = Model::default();
    m.preview(Screen::Clear, &data).unwrap();
    assert_eq!(
        m.handle_factory(Event::Tap { x: 90, y: 115 }, &data)
            .unwrap(),
        Effect::None
    );
    assert_ne!(m.screen(), Screen::Request);
}
#[test]
fn test_should_select_factory_language_label_and_ignore_menu_gaps() {
    let d = Snapshot::unavailable();
    let mut m = Model::default();
    m.preview(Screen::Language, &d).unwrap();
    m.handle_factory(Event::Tap { x: 60, y: 70 }, &d).unwrap();
    assert_eq!(m.draft(), 1);
    let e = m.handle_factory(Event::Tap { x: 60, y: 115 }, &d).unwrap();
    assert!(matches!(
        e,
        Effect::Request(UiRequest::AppearanceCommit { value: 1, .. })
    ));
    m.preview(Screen::Settings, &d).unwrap();
    m.handle_factory(Event::Tap { x: 60, y: 44 }, &d).unwrap();
    assert_eq!(m.screen(), Screen::Settings);
}
#[test]
fn test_should_match_horizontal_shortcut_and_music_bitmap_controls() {
    let d = Snapshot::unavailable();
    let mut m = Model::default();
    m.preview(Screen::Quick, &d).unwrap();
    m.handle_factory(Event::Tap { x: 64, y: 50 }, &d).unwrap();
    assert_eq!(m.screen(), Screen::Brightness);
    m.preview(Screen::Quick, &d).unwrap();
    m.handle_factory(Event::Tap { x: 106, y: 50 }, &d).unwrap();
    assert_eq!(m.screen(), Screen::Settings);
    m.preview(Screen::Music, &d).unwrap();
    for (x, y, expected) in [
        (64, 68, UiRequest::MusicPlayPause),
        (28, 115, UiRequest::VolumeUp),
        (100, 116, UiRequest::VolumeDown),
    ] {
        assert_eq!(
            m.handle_factory(Event::Tap { x, y }, &d).unwrap(),
            Effect::Request(expected)
        );
    }
}
fn digits_pack() -> Vec<u8> {
    let rows = 9376 + 11 * 256;
    let mut p = vec![0; rows + 11 * 256];
    p[..8].copy_from_slice(b"W5UIF001");
    for (at, n) in [
        (8, 1u32),
        (12, 2336),
        (16, 9376),
        (20, u32::try_from(rows).unwrap()),
        (24, u32::try_from(rows + 11 * 256).unwrap()),
        (28, 11),
    ] {
        p[at..at + 4].copy_from_slice(&n.to_le_bytes());
    }
    for b in p[32..9376].chunks_exact_mut(4) {
        b.copy_from_slice(&9376u32.to_le_bytes());
    }
    for digit in 0..10usize {
        let map = 9376 + (digit + 1) * 256;
        let key = 2232 + digit;
        p[32 + key * 4..36 + key * 4].copy_from_slice(&u32::try_from(map).unwrap().to_le_bytes());
        for b in p[map..map + 256].chunks_exact_mut(2) {
            b.copy_from_slice(&u16::try_from(digit + 1).unwrap().to_le_bytes());
        }
        for b in p[rows + (digit + 1) * 256..rows + (digit + 2) * 256].chunks_exact_mut(2) {
            b.copy_from_slice(&u16::try_from(digit + 1).unwrap().to_be_bytes());
        }
    }
    p
}
#[test]
fn test_should_preserve_whole_and_four_digit_tenth_temperature_and_hide_unavailable_readings() {
    use wattch5_parity_ui_demo::{MeasureKind, MeasureState, MeasurementSnapshot, Reading};
    let a = FactoryAssets::open(Source(digits_pack())).unwrap();
    let mut d = Snapshot::unavailable();
    let mut m = Model::default();
    m.preview(Screen::Temperature, &d).unwrap();
    d.measurements[MeasureKind::Temperature as usize] = MeasurementSnapshot {
        state: MeasureState::Result,
        reading: Reading::Tenth(1234),
    };
    let mut out = [Rgb565::new(0, 0, 0); 128];
    a.render(&m, &d, 100, &mut out).unwrap();
    assert_eq!(
        [
            out[16].into_storage(),
            out[33].into_storage(),
            out[50].into_storage(),
            out[73].into_storage()
        ],
        [2, 3, 4, 5]
    );
    d.measurements[MeasureKind::Temperature as usize].reading = Reading::Number(36);
    a.render(&m, &d, 108, &mut out).unwrap();
    assert_eq!(
        out[64].into_storage(),
        0,
        "whole degrees have no decimal point"
    );
    d.measurements[MeasureKind::Temperature as usize].state = MeasureState::Unavailable;
    a.render(&m, &d, 100, &mut out).unwrap();
    assert!(out.iter().all(|p| p.into_storage() == 0));
}
#[test]
fn test_should_render_supplied_weather_and_distinct_measurement_failure_feedback() {
    use wattch5_parity_ui_demo::{MeasureKind, MeasureState, WeatherCondition, WeatherSnapshot};
    let a = FactoryAssets::open(Source(digits_pack())).unwrap();
    let mut d = Snapshot::unavailable();
    let mut m = Model::default();
    m.preview(Screen::Weather, &d).unwrap();
    let mut absent = Vec::new();
    let mut supplied = Vec::new();
    let mut row = [Rgb565::new(0, 0, 0); 128];
    for y in 21..128 {
        a.render(&m, &d, y, &mut row).unwrap();
        absent.extend(row);
    }
    d.weather = Some(WeatherSnapshot {
        low: -2,
        high: 18,
        condition: WeatherCondition::Rain,
    });
    for y in 21..128 {
        a.render(&m, &d, y, &mut row).unwrap();
        supplied.extend(row);
    }
    assert_ne!(
        absent, supplied,
        "weather provider must affect actual pixels"
    );
    m.preview(Screen::Oxygen, &d).unwrap();
    let mut unavailable = Vec::new();
    let mut failed = Vec::new();
    for y in 116..128 {
        a.render(&m, &d, y, &mut row).unwrap();
        unavailable.extend(row);
    }
    d.measurements[MeasureKind::Oxygen as usize].state = MeasureState::Error;
    for y in 116..128 {
        a.render(&m, &d, y, &mut row).unwrap();
        failed.extend(row);
    }
    assert_ne!(
        unavailable, failed,
        "provider errors cannot look like an idle page"
    );
}
#[test]
fn test_should_decode_factory_rows_with_only_small_bounded_source_reads() {
    #[derive(Debug)]
    struct Small(Source);
    impl ReadAt for Small {
        type Error = ();
        fn capacity(&self) -> u32 {
            self.0.capacity()
        }
        fn read_exact(&mut self, at: u32, out: &mut [u8]) -> Result<(), ()> {
            if out.len() > 64 {
                return Err(());
            }
            self.0.read_exact(at, out)
        }
    }
    let a = FactoryAssets::open(Small(Source(pack()))).unwrap();
    let mut row = [Rgb565::new(0, 0, 0); 128];
    a.row(&Model::default(), 0, &mut row).unwrap();
    assert!(row.iter().all(|p| p.into_storage() == 0xf800));
}
