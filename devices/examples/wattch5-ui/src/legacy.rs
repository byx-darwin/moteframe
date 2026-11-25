//! Read-only adapter for the source resource tool's8-byte raster header.
use device_assets::{Bitmap, Format, ReadAt, ReadError, Region};
use device_core::Rgb565;
/// Source, metadata or unsupported source encoding failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LegacyError<E> {
    /// Header/data/palette extent is invalid.
    Bounds,
    /// This adapter does not support the declared encoding.
    Encoding,
    /// An exact read failed.
    Read(ReadError<E>),
}
/// Loads checked metadata and, if present, the16-color local palette only.
/// Pixel data stays in the read-only source. No guessed address/version fallback.
/// # Errors
/// Rejects invalid extents, width outside1..128, unknown encodings and failed reads.
pub fn open_legacy<S: ReadAt>(
    source: &mut S,
    address: u32,
) -> Result<Bitmap, LegacyError<S::Error>> {
    let head = Region::new(address, 8, source.capacity()).map_err(|_| LegacyError::Bounds)?;
    let mut bytes = [0; 8];
    head.read(source, 0, &mut bytes)
        .map_err(LegacyError::Read)?;
    let width = u16::from_be_bytes([bytes[2], bytes[3]]);
    let height = u16::from_be_bytes([bytes[4], bytes[5]]);
    if width == 0 || width > 128 || height == 0 {
        return Err(LegacyError::Bounds);
    }
    let pixels = u32::from(width) * u32::from(height);
    let start = address.checked_add(8).ok_or(LegacyError::Bounds)?;
    let (data_start, len, format) = match bytes[0] {
        0 => (
            start,
            pixels.checked_mul(2).ok_or(LegacyError::Bounds)?,
            Format::Rgb565Be,
        ),
        127 => {
            let region =
                Region::new(start, 32, source.capacity()).map_err(|_| LegacyError::Bounds)?;
            let mut raw = [0; 32];
            region
                .read(source, 0, &mut raw)
                .map_err(LegacyError::Read)?;
            let mut palette = [Rgb565(0); 16];
            for (p, pair) in palette.iter_mut().zip(raw.chunks_exact(2)) {
                p.0 = u16::from_be_bytes([pair[0], pair[1]]);
            }
            (
                start.checked_add(32).ok_or(LegacyError::Bounds)?,
                pixels.div_ceil(2),
                Format::Indexed4(palette),
            )
        }
        _ => return Err(LegacyError::Encoding),
    };
    let data = Region::new(data_start, len, source.capacity()).map_err(|_| LegacyError::Bounds)?;
    Bitmap::new(width, height, data, format).map_err(|_| LegacyError::Bounds)
}
