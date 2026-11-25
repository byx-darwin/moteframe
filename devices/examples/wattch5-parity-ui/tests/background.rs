#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "Host fixture assertions fail immediately on invalid generated resources."
)]
//! Capture provenance and bounds for image-backed product presentation.
use device_ui::graphics::{pixelcolor::Rgb565, prelude::*};
use wattch5_parity_ui_demo::*;
#[derive(Debug)]
struct Frame(Vec<u16>);
impl OriginDimensions for Frame {
    fn size(&self) -> Size {
        Size::new(128, 128)
    }
}
impl DrawTarget for Frame {
    type Color = Rgb565;
    type Error = ();
    fn draw_iter<I: IntoIterator<Item = Pixel<Rgb565>>>(&mut self, i: I) -> Result<(), ()> {
        for Pixel(p, c) in i {
            self.0[usize::try_from(p.y).map_err(|_| ())? * 128
                + usize::try_from(p.x).map_err(|_| ())?] = c.into_storage();
        }
        Ok(())
    }
}
#[test]
fn test_should_restore_background_then_overlay_real_health_without_stale_digits() {
    let mut m = Model::default();
    let mut d = Snapshot::unavailable();
    m.preview(Screen::Pressure, &d).unwrap();
    let mut got = Frame(vec![0; 16384]);
    m.draw_background(&mut got).unwrap();
    for (upper, lower) in [(123, 81), (99, 65)] {
        d.measurements[1] = MeasurementSnapshot {
            state: MeasureState::Result,
            reading: Reading::Pressure { upper, lower },
        };
        let scene = m.product_scene(&d).unwrap();
        scene.draw_dynamic(&mut got).unwrap();
        let mut reference = Frame(vec![0; 16384]);
        scene.draw(&mut reference).unwrap();
        assert_eq!(got.0, reference.0);
    }
}
#[cfg(feature = "asset-build")]
#[test]
fn test_should_match_every_background_variant_locale_theme_and_live_snapshot() {
    let messages = [Message {
        zh: "心率",
        en: "HEART",
    }];
    let live = Snapshot {
        messages: &messages,
        ..Snapshot::default()
    };
    for page in Screen::ALL {
        for locale in [Locale::Zh, Locale::En] {
            for variant in 0..Model::background_variants(page) {
                let m = Model::background_model(page, locale, variant).unwrap();
                for theme in [Theme::Phosphor, Theme::Amber, Theme::Ice] {
                    for d in [Snapshot::unavailable(), live] {
                        let mut m = m;
                        let mut a = m.appearance();
                        a.theme = theme;
                        m.set_appearance(a).unwrap();
                        let scene = m.product_scene(&d).unwrap();
                        let mut background = Frame(vec![0; 16384]);
                        scene
                            .background_model()
                            .draw_background(&mut background)
                            .unwrap();
                        scene.draw_dynamic(&mut background).unwrap();
                        let mut reference = Frame(vec![0; 16384]);
                        scene.draw(&mut reference).unwrap();
                        assert_eq!(
                            background.0, reference.0,
                            "{page:?} {locale:?} {variant} {theme:?}"
                        );
                    }
                }
            }
        }
    }
}
