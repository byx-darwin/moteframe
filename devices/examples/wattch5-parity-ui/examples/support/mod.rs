//! Host raster storage only; the runtime draws through128-pixel rows.
use device_core::{Dimensions as Size, Display, Rect, Rgb565 as Packed};
use device_ui::{DirtyRegion, graphics::pixelcolor::Rgb565, repaint};
use std::error::Error;
use wattch5_parity_ui_demo::{Model, Snapshot};
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
pub fn render(model: &Model, data: &Snapshot<'_>) -> Result<Vec<u16>, Box<dyn Error>> {
    let scene = model.scene(data).map_err(|e| format!("{e:?}"))?;
    let mut target = Raster(vec![Packed(0); 16384]);
    let mut dirty = DirtyRegion::new(target.dimensions());
    dirty.invalidate_all();
    repaint(
        &mut target,
        &mut dirty,
        &mut [Packed(0); 128],
        Rgb565::new(0, 0, 0),
        |tile| scene.draw(tile),
    )
    .map_err(|e| format!("{e:?}"))?;
    Ok(target.0.into_iter().map(|p| p.0).collect())
}
