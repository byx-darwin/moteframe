//! Host-only replay helpers; all rendering and state transitions stay in the Rust library.
use device_core::{Dimensions as Size, Display, Rect, Rgb565 as Packed};
use device_ui::{DirtyRegion, graphics::pixelcolor::Rgb565, repaint};
use std::error::Error;
use wattch5_retro_ui_demo::{Model, Screen};
pub fn choose<T: Copy>(s: &str, items: &[(&str, T)]) -> Result<T, Box<dyn Error>> {
    items
        .iter()
        .find(|(name, _)| *name == s)
        .map(|(_, value)| *value)
        .ok_or_else(|| format!("invalid choice:{s}").into())
}
pub fn page(s: &str) -> Result<Screen, Box<dyn Error>> {
    choose(
        s,
        &[
            ("dial", Screen::Dial),
            ("tools", Screen::Tools),
            ("quick", Screen::Quick),
            ("settings", Screen::Settings),
            ("display", Screen::Display),
            ("brightness", Screen::Brightness),
            ("face", Screen::Face),
            ("theme", Screen::Theme),
            ("format", Screen::Format),
            ("about", Screen::About),
            ("timer", Screen::Timer),
            ("stopwatch", Screen::Stopwatch),
            ("system", Screen::System),
            ("confirm", Screen::Confirm),
            ("request", Screen::Request),
        ],
    )
}

#[derive(Debug)]
struct Raster(Vec<Packed>);
impl Display for Raster {
    type Error = ();
    fn dimensions(&self) -> Size {
        Size::new(128, 128)
    }
    fn write(&mut self, r: Rect, p: &[Packed]) -> Result<(), ()> {
        if !r.fits(self.dimensions()) || p.len() != usize::try_from(r.area()).map_err(|_| ())? {
            return Err(());
        }
        for row in 0..usize::from(r.height) {
            let dest = (usize::from(r.y) + row) * 128 + usize::from(r.x);
            let src = row * usize::from(r.width);
            self.0[dest..dest + usize::from(r.width)]
                .copy_from_slice(&p[src..src + usize::from(r.width)]);
        }
        Ok(())
    }
}
pub fn render(model: Model) -> Result<Vec<u16>, Box<dyn Error>> {
    let mut target = Raster(vec![Packed(0); 16384]);
    let mut dirty = DirtyRegion::new(target.dimensions());
    dirty.invalidate_all();
    repaint(
        &mut target,
        &mut dirty,
        &mut [Packed(0); 128],
        Rgb565::new(0, 0, 0),
        |tile| model.draw(tile),
    )
    .map_err(|e| format!("{e:?}"))?;
    Ok(target.0.into_iter().map(|p| p.0).collect())
}
