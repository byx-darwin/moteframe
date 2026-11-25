//! Compatibility for the original packed-word display contract.
use crate::{Display, Rect, Rgb565};
use embedded_graphics_core::{pixelcolor::Rgb565 as Native, prelude::IntoStorage};
/// Full-input validation or the original device failure, without hidden retries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeWriteError<E> {
    /// Invalid geometry or full pixel count, before I/O.
    InvalidInput,
    /// Original display error; earlier chunks may already have completed.
    Display(E),
}
pub(super) fn write<D: Display + ?Sized>(
    d: &mut D,
    r: Rect,
    pixels: &[Native],
) -> Result<(), NativeWriteError<D::Error>> {
    if !r.fits(d.dimensions()) || u32::try_from(pixels.len()).ok() != Some(r.area()) {
        return Err(NativeWriteError::InvalidInput);
    }
    let mut words = [Rgb565(0); 8];
    for (dy, row) in pixels.chunks(usize::from(r.width)).enumerate() {
        for (dx, chunk) in row.chunks(8).enumerate() {
            for (out, pixel) in words.iter_mut().zip(chunk) {
                *out = Rgb565(pixel.into_storage());
            }
            let x = r.x + u16::try_from(dx * 8).map_err(|_| NativeWriteError::InvalidInput)?;
            let y = r.y + u16::try_from(dy).map_err(|_| NativeWriteError::InvalidInput)?;
            let width = u16::try_from(chunk.len()).map_err(|_| NativeWriteError::InvalidInput)?;
            d.write(Rect::new(x, y, width, 1), &words[..chunk.len()])
                .map_err(NativeWriteError::Display)?;
        }
    }
    Ok(())
}
