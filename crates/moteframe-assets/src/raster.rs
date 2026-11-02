// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

use crate::{AssetError, GeometryError, PixelFormat};
use core::{fmt, marker::PhantomData};
use moteframe_core::ReadAt;

/// Immutable tightly packed raster descriptor; no image/scratch/backend storage.
pub struct Raster<F: PixelFormat> {
    offset: u32,
    end: u32,
    width: u16,
    height: u16,
    bytes_per_pixel: u32,
    format: PhantomData<fn() -> F>,
}
impl<F: PixelFormat> Copy for Raster<F> {}
impl<F: PixelFormat> Clone for Raster<F> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<F: PixelFormat> fmt::Debug for Raster<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Raster")
            .field("offset", &self.offset)
            .field("width", &self.width)
            .field("height", &self.height)
            .finish_non_exhaustive()
    }
}
impl<F: PixelFormat> Raster<F> {
    /// Checks nonempty geometry and complete encoded extent without reading.
    /// # Errors
    /// Returns invalid format, empty geometry or overflowing source extent.
    pub fn new(offset: u32, width: u16, height: u16) -> Result<Self, GeometryError> {
        if width == 0 || height == 0 {
            return Err(GeometryError::Empty);
        }
        let bytes_per_pixel =
            u32::try_from(F::BYTES_PER_PIXEL).map_err(|_| GeometryError::InvalidFormat)?;
        if bytes_per_pixel == 0 {
            return Err(GeometryError::InvalidFormat);
        }
        let end = u32::from(width)
            .checked_mul(u32::from(height))
            .and_then(|n| n.checked_mul(bytes_per_pixel))
            .and_then(|n| n.checked_add(offset))
            .ok_or(GeometryError::Overflow)?;
        Ok(Self {
            offset,
            end,
            width,
            height,
            bytes_per_pixel,
            format: PhantomData,
        })
    }
    /// Logical source width.
    #[must_use]
    pub const fn width(self) -> u16 {
        self.width
    }
    /// Logical source height.
    #[must_use]
    pub const fn height(self) -> u16 {
        self.height
    }

    /// Decodes one nonempty in-row span using only caller-owned buffers.
    ///
    /// Each read is at most `scratch.len()` bytes, rounded down to whole pixels.
    /// Source capacity must contain the whole descriptor at this call. No
    /// snapshot is implied: the provider must keep content stable for the frame.
    /// Callers choose a bounded span and yield to other work between spans.
    /// # Errors
    /// Validation errors occur before I/O/output mutation. A source or decode
    /// error stops immediately without retry; discard the entire output span.
    pub fn read_span<R: ReadAt + ?Sized>(
        &self,
        source: &mut R,
        x: u16,
        y: u16,
        out: &mut [F::Color],
        scratch: &mut [u8],
    ) -> Result<(), AssetError<R::Error>> {
        let count = u32::try_from(out.len()).map_err(|_| AssetError::Bounds)?;
        if count == 0
            || y >= self.height
            || u32::from(x)
                .checked_add(count)
                .is_none_or(|end| end > u32::from(self.width))
            || source.capacity() < self.end
        {
            return Err(AssetError::Bounds);
        }
        let per_chunk = scratch.len() / F::BYTES_PER_PIXEL;
        if per_chunk == 0 {
            return Err(AssetError::ScratchTooSmall);
        }
        let mut cursor = self.offset
            + (u32::from(y) * u32::from(self.width) + u32::from(x)) * self.bytes_per_pixel;
        for chunk in out.chunks_mut(per_chunk) {
            let length = chunk.len() * F::BYTES_PER_PIXEL;
            let bytes = &mut scratch[..length];
            source
                .read_exact(cursor, bytes)
                .map_err(AssetError::Source)?;
            for (color, encoded) in chunk.iter_mut().zip(bytes.chunks_exact(F::BYTES_PER_PIXEL)) {
                *color = F::decode(encoded).map_err(AssetError::Decode)?;
            }
            cursor += u32::try_from(length).map_err(|_| AssetError::Bounds)?;
        }
        Ok(())
    }
}

/// Provider-defined glyph positioning; independent of character map or font data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlyphMetrics {
    /// Signed horizontal bearing in logical pixels.
    pub bearing_x: i16,
    /// Signed vertical bearing in logical pixels.
    pub bearing_y: i16,
    /// Horizontal cursor advance in logical pixels.
    pub advance: u16,
}
/// A source-backed glyph and its positioning, with no copied font table.
pub struct Glyph<F: PixelFormat> {
    raster: Raster<F>,
    metrics: GlyphMetrics,
}
impl<F: PixelFormat> fmt::Debug for Glyph<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Glyph")
            .field("raster", &self.raster)
            .field("metrics", &self.metrics)
            .finish()
    }
}
impl<F: PixelFormat> Glyph<F> {
    /// Combines a validated raster with caller-supplied font metrics.
    #[must_use]
    pub const fn new(raster: Raster<F>, metrics: GlyphMetrics) -> Self {
        Self { raster, metrics }
    }
    /// Borrows the immutable raster descriptor.
    #[must_use]
    pub const fn raster(&self) -> &Raster<F> {
        &self.raster
    }
    /// Returns positioning without loading glyph bytes.
    #[must_use]
    pub const fn metrics(&self) -> GlyphMetrics {
        self.metrics
    }
}
