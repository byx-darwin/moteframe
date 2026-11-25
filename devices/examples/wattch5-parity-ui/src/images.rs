//! Private immutable V2 background catalogue using the existing `MoteFrame` raster decoder.
use crate::{Model, Theme};
use core::cell::{Cell, RefCell};
use device_ui::graphics::{
    pixelcolor::{Rgb565, raw::RawU16},
    prelude::*,
};
use moteframe_assets::{Raster, Rgb565Be};
use moteframe_core::ReadAt;
/// Image failures preserve source errors, including transient storage contention.
#[derive(Debug, PartialEq, Eq)]
pub enum ImageError<E> {
    /// Underlying immutable reader failed.
    Source(E),
    /// Foreign version, bounds, palette or missing image.
    Format,
    /// A concurrent foreground borrower owns the reader.
    Busy,
}
/// Image source with no framebuffer, cache or mutable storage capability.
pub struct ImageAssets<R> {
    source: RefCell<R>,
    tile_start: u32,
    end: u32,
    scene: Cell<(u16, u32)>,
}
impl<R> core::fmt::Debug for ImageAssets<R> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // The reader is a capability, not a diagnostic payload. Keep formatting
        // bounded and avoid linking source/pixel or numeric formatter callbacks.
        f.debug_struct("ImageAssets").finish_non_exhaustive()
    }
}

fn word(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}
impl<R: ReadAt> ImageAssets<R> {
    /// Check the complete fixed catalogue geometry before use.
    /// # Errors
    /// Rejects foreign version, insufficient source and source errors.
    pub fn open(mut source: R) -> Result<Self, ImageError<R::Error>> {
        let mut h = [0; 32];
        source.read_exact(0, &mut h).map_err(ImageError::Source)?;
        let tile_start = word(&h, 20);
        let end = word(&h, 24);
        let packed = &h[..8] == b"W5UIV202" && word(&h, 8) == 2;
        let legacy = &h[..8] == b"W5UIV201" && word(&h, 8) == 1;
        let scenes = word(&h, 28);
        if (!packed && !legacy)
            || word(&h, 12) != 2232
            || word(&h, 16) != 8960
            || end <= tile_start
            || end > 0x20_0000
            || source.capacity() < end
            || scenes == 0
            || (packed
                && (tile_start != 8960 || scenes.checked_mul(4096) != end.checked_sub(tile_start)))
            || (legacy
                && (tile_start <= 8960
                    || !(tile_start - 8960).is_multiple_of(128)
                    || !(end - tile_start).is_multiple_of(512)))
        {
            return Err(ImageError::Format);
        }
        Ok(Self {
            source: RefCell::new(source),
            tile_start,
            end,
            scene: Cell::new((u16::MAX, 0)),
        })
    }
    /// Load one complete128-pixel background row in four bounded tile spans.
    /// No display writes occur here; callers discard the row after any error.
    /// # Errors
    /// Propagates contention/source errors and rejects missing or corrupt image references.
    #[inline(never)]
    pub fn row(
        &self,
        m: &Model,
        y: u16,
        out: &mut [Rgb565; 128],
    ) -> Result<(), ImageError<R::Error>> {
        if y >= 128 {
            return Err(ImageError::Format);
        }
        let mut source = self.source.try_borrow_mut().map_err(|_| ImageError::Busy)?;
        if self.tile_start == 8960 {
            return self.packed_row(&mut *source, m, y, out);
        }
        let mut bytes = [0; 8];
        source
            .read_exact(32 + u32::from(m.background_key()) * 4, &mut bytes[..4])
            .map_err(ImageError::Source)?;
        let map = word(&bytes, 0);
        if map < 8960
            || !map.is_multiple_of(128)
            || map.checked_add(128).is_none_or(|end| end > self.tile_start)
        {
            return Err(ImageError::Format);
        }
        source
            .read_exact(map + u32::from(y / 8) * 8, &mut bytes)
            .map_err(ImageError::Source)?;
        for (chunk, index) in out.chunks_exact_mut(32).zip(bytes.chunks_exact(2)) {
            let start = self.tile_start + u32::from(u16::from_le_bytes([index[0], index[1]])) * 512;
            if start.checked_add(512).is_none_or(|end| end > self.end) {
                return Err(ImageError::Format);
            }
            let raster = Raster::<Rgb565Be>::new(start, 32, 8).map_err(|_| ImageError::Format)?;
            let mut encoded = [0; 64];
            raster
                .read_span(&mut *source, 0, y % 8, chunk, &mut encoded)
                .map_err(|e| match e {
                    moteframe_assets::AssetError::Source(e) => ImageError::Source(e),
                    _ => ImageError::Format,
                })?;
        }
        let theme = if m.screen() == crate::Screen::Theme {
            m.draft()
        } else {
            m.appearance().theme as u8
        };
        let palette = crate::canvas::COLORS[usize::from(theme)];
        for color in out {
            let index = match color.into_storage() {
                129 => 0,
                57177 => 1,
                48941 => 2,
                36111 => 3,
                _ => return Err(ImageError::Format),
            };
            if theme != Theme::Phosphor as u8 {
                *color = Rgb565::from(RawU16::new(palette[index]));
            }
        }
        Ok(())
    }
    // Each immutable four-color row is32 contiguous bytes. Cache only its
    // checked scene address, never pixel data or a source borrow.
    #[inline(never)]
    fn packed_row(
        &self,
        source: &mut R,
        m: &Model,
        y: u16,
        out: &mut [Rgb565; 128],
    ) -> Result<(), ImageError<R::Error>> {
        let key = m.background_key();
        let (cached_key, mut start) = self.scene.get();
        if cached_key != key {
            let mut address = [0; 4];
            source
                .read_exact(32 + u32::from(key) * 4, &mut address)
                .map_err(ImageError::Source)?;
            start = u32::from_le_bytes(address);
            if start < 8960
                || !(start - 8960).is_multiple_of(4096)
                || start.checked_add(4096).is_none_or(|end| end > self.end)
            {
                return Err(ImageError::Format);
            }
            self.scene.set((key, start));
        }
        let mut bytes = [0; 32];
        source
            .read_exact(start + u32::from(y) * 32, &mut bytes)
            .map_err(ImageError::Source)?;
        let theme = if m.screen() == crate::Screen::Theme {
            m.draft()
        } else {
            m.appearance().theme as u8
        };
        let palette =
            crate::canvas::COLORS[usize::from(theme)].map(|v| Rgb565::from(RawU16::new(v)));
        for (chunk, byte) in out.chunks_exact_mut(4).zip(bytes) {
            for (pixel, shift) in chunk.iter_mut().zip([6, 4, 2, 0]) {
                *pixel = palette[usize::from((byte >> shift) & 3)];
            }
        }
        Ok(())
    }
}
