// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Bounded source-backed decoding. Descriptors never own pixels or a reader.
//! Resource identity/catalogs remain provider concerns. Callers separately own
//! mutable `ReadAt` access, decoded output and byte scratch; none is retained.
#![no_std]
#![forbid(unsafe_code)]

mod formats;
mod raster;
pub use formats::{Gray8Bytes, Mono8, Rgb565Be, Rgb565Le, Rgb888Bytes};
pub use raster::{Glyph, GlyphMetrics, Raster};

/// A descriptor is invalid before any source is accessed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum GeometryError {
    /// Zero-sized raster.
    Empty,
    /// Pixel extent or offset does not fit the u32 source address space.
    Overflow,
    /// Pixel format has zero or unrepresentable byte width.
    InvalidFormat,
}

/// A stored pixel is malformed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DecodeError {
    /// Decoder did not receive exactly one pixel's bytes.
    InvalidLength,
    /// Stored value is not valid for this format.
    InvalidValue,
}

/// Span validation or first source/decoder failure. Discard the entire output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum AssetError<E> {
    /// Empty/out-of-row span, truncated source or unrepresentable range.
    Bounds,
    /// Scratch cannot hold one encoded pixel.
    ScratchTooSmall,
    /// Original read failure; output and scratch may be partially modified.
    Source(E),
    /// Original pixel validation failure; preceding output may be modified.
    Decode(DecodeError),
}

/// Fixed-width stored-pixel decoding, independent of panel wire encoding.
pub trait PixelFormat {
    /// Native logical color; UI drawing additionally requires `PixelColor`.
    type Color: Copy;
    /// Positive byte width; `Raster` validates it before use.
    const BYTES_PER_PIXEL: usize;
    /// Decodes exactly one stored pixel without I/O or retained borrows.
    /// # Errors
    /// Reject wrong input length or an invalid stored value.
    fn decode(bytes: &[u8]) -> Result<Self::Color, DecodeError>;
}
