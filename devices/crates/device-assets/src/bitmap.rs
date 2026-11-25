//! Bounded spans of tightly packed rasters; coordinates never depend on a panel.
use crate::{Bounds, ReadAt, ReadError, Region};
use device_core::Rgb565;
/// Encoding of the tightly packed source pixel stream.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    /// Two bytes per pixel, most significant byte first.
    Rgb565Be,
    /// Two bytes per pixel, least significant byte first.
    Rgb565Le,
    /// Two pixels per byte, high nibble first, with a validated caller palette.
    Indexed4([Rgb565; 16]),
}
/// Source-backed bitmap. Image bytes are never loaded as a whole.
#[derive(Clone, Copy, Debug)]
pub struct Bitmap {
    width: u16,
    height: u16,
    data: Region,
    format: Format,
}
impl Bitmap {
    /// Validates nonempty geometry and complete tightly packed pixel extent.
    /// # Errors
    /// Rejects zero dimensions and truncated or overflowing extents.
    pub fn new(width: u16, height: u16, data: Region, format: Format) -> Result<Self, Bounds> {
        if width == 0 || height == 0 {
            return Err(Bounds);
        }
        let pixels = u32::from(width) * u32::from(height);
        let bytes = match format {
            Format::Indexed4(_) => pixels.div_ceil(2),
            _ => pixels.checked_mul(2).ok_or(Bounds)?,
        };
        if bytes > data.len() {
            return Err(Bounds);
        }
        Ok(Self {
            width,
            height,
            data,
            format,
        })
    }
    /// Source width.
    #[must_use]
    pub const fn width(self) -> u16 {
        self.width
    }
    /// Source height, including all strip frames.
    #[must_use]
    pub const fn height(self) -> u16 {
        self.height
    }
    /// Decodes up to128 adjacent pixels from one source row.
    /// Yield to other work between spans, never inside a selected SPI transaction.
    /// # Errors
    /// Rejects empty/oversized or crossing-row spans before I/O. Source errors propagate.
    pub fn read_span<S: ReadAt>(
        &self,
        source: &mut S,
        x: u16,
        y: u16,
        out: &mut [Rgb565],
    ) -> Result<(), ReadError<S::Error>> {
        let count = u32::try_from(out.len()).map_err(|_| ReadError::Bounds)?;
        if count == 0
            || count > 128
            || y >= self.height
            || u32::from(x) + count > u32::from(self.width)
        {
            return Err(ReadError::Bounds);
        }
        let pixel = u32::from(y) * u32::from(self.width) + u32::from(x);
        let mut bytes = [0_u8; 256];
        match self.format {
            Format::Rgb565Be | Format::Rgb565Le => {
                self.data
                    .read(source, pixel * 2, &mut bytes[..out.len() * 2])?;
                for (value, pair) in out.iter_mut().zip(bytes.chunks_exact(2)) {
                    value.0 = if self.format == Format::Rgb565Be {
                        u16::from_be_bytes([pair[0], pair[1]])
                    } else {
                        u16::from_le_bytes([pair[0], pair[1]])
                    };
                }
            }
            Format::Indexed4(palette) => {
                let odd = usize::try_from(pixel % 2).map_err(|_| ReadError::Bounds)?;
                let n = (out.len() + odd).div_ceil(2);
                self.data.read(source, pixel / 2, &mut bytes[..n])?;
                for (i, value) in out.iter_mut().enumerate() {
                    let at = i + odd;
                    let byte = bytes[at / 2];
                    let index = if at % 2 == 0 { byte >> 4 } else { byte & 15 };
                    *value = palette[usize::from(index)];
                }
            }
        }
        Ok(())
    }
}
