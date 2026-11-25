//! Literal palette, glyph and tile-clipping expectations independent of renderer traversal.
use device_ui::graphics::{pixelcolor::Rgb565, prelude::*, primitives::Rectangle};
use wattch5_retro_ui_demo::{Model, Screen};
#[derive(Debug)]
struct Target {
    pixels: Vec<Rgb565>,
    area: Rectangle,
}
impl Dimensions for Target {
    fn bounding_box(&self) -> Rectangle {
        self.area
    }
}
impl DrawTarget for Target {
    type Color = Rgb565;
    type Error = core::convert::Infallible;
    fn draw_iter<I: IntoIterator<Item = Pixel<Rgb565>>>(
        &mut self,
        pixels: I,
    ) -> Result<(), Self::Error> {
        for Pixel(p, c) in pixels {
            assert!(self.area.contains(p));
            let i = usize::try_from(p.y * 128 + p.x).unwrap_or_else(|e| panic!("{e:?}"));
            self.pixels[i] = c;
        }
        Ok(())
    }
}
#[test]
fn test_should_draw_default_palette_and_original_5x7_glyph_without_clipped_writes() {
    assert_eq!(wattch5_retro_ui_demo::GRAPHIC_DATA_BYTES, 438);
    let mut target = Target {
        pixels: vec![Rgb565::new(0, 0, 0); 16384],
        area: Rectangle::new(Point::zero(), Size::new(128, 128)),
    };
    Model::default()
        .draw(&mut target)
        .unwrap_or_else(|e| panic!("{e:?}"));
    // PHOSPHOR background #07130D, accent #B8E46B. Top-left W pixel at8,6.
    assert_eq!(target.pixels[0], Rgb565::new(0, 4, 1));
    assert_eq!(target.pixels[6 * 128 + 8], Rgb565::new(23, 57, 13));
    let mut row = Target {
        pixels: vec![Rgb565::new(0, 0, 0); 16384],
        area: Rectangle::new(Point::new(0, 6), Size::new(128, 1)),
    };
    Model::default()
        .draw(&mut row)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(row.pixels[6 * 128 + 8], target.pixels[6 * 128 + 8]);
}
#[test]
fn test_should_render_all_fifteen_pages_within_logical_bounds() {
    for page in [
        Screen::Dial,
        Screen::Tools,
        Screen::Quick,
        Screen::Settings,
        Screen::Display,
        Screen::Brightness,
        Screen::Face,
        Screen::Theme,
        Screen::Format,
        Screen::About,
        Screen::Timer,
        Screen::Stopwatch,
        Screen::System,
        Screen::Confirm,
        Screen::Request,
    ] {
        let mut model = Model::default();
        model.preview(page).unwrap_or_else(|e| panic!("{e:?}"));
        let mut target = Target {
            pixels: vec![Rgb565::new(0, 0, 0); 16384],
            area: Rectangle::new(Point::zero(), Size::new(128, 128)),
        };
        model
            .draw(&mut target)
            .unwrap_or_else(|e| panic!("{page:?}:{e:?}"));
    }
}
