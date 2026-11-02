// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

use crate::{DecodeError, PixelFormat};
use embedded_graphics_core::pixelcolor::{BinaryColor, Gray8, Rgb565, Rgb888};

/// Stored RGB565, most significant byte first.
#[derive(Clone, Copy, Debug)]
pub struct Rgb565Be;
/// Stored RGB565, least significant byte first.
#[derive(Clone, Copy, Debug)]
pub struct Rgb565Le;
/// Three stored bytes: red, green, blue.
#[derive(Clone, Copy, Debug)]
pub struct Rgb888Bytes;
/// One stored byte per gray pixel.
#[derive(Clone, Copy, Debug)]
pub struct Gray8Bytes;
/// One stored byte per monochrome pixel, exactly 0 or 1; usable for glyph masks.
#[derive(Clone, Copy, Debug)]
pub struct Mono8;

fn rgb565(value: u16) -> Rgb565 {
    Rgb565::new(
        ((value >> 11) & 31) as u8,
        ((value >> 5) & 63) as u8,
        (value & 31) as u8,
    )
}
impl PixelFormat for Rgb565Be {
    type Color = Rgb565;
    const BYTES_PER_PIXEL: usize = 2;
    fn decode(bytes: &[u8]) -> Result<Rgb565, DecodeError> {
        let [a, b] = bytes else {
            return Err(DecodeError::InvalidLength);
        };
        Ok(rgb565(u16::from_be_bytes([*a, *b])))
    }
}
impl PixelFormat for Rgb565Le {
    type Color = Rgb565;
    const BYTES_PER_PIXEL: usize = 2;
    fn decode(bytes: &[u8]) -> Result<Rgb565, DecodeError> {
        let [a, b] = bytes else {
            return Err(DecodeError::InvalidLength);
        };
        Ok(rgb565(u16::from_le_bytes([*a, *b])))
    }
}
impl PixelFormat for Rgb888Bytes {
    type Color = Rgb888;
    const BYTES_PER_PIXEL: usize = 3;
    fn decode(bytes: &[u8]) -> Result<Rgb888, DecodeError> {
        let [r, g, b] = bytes else {
            return Err(DecodeError::InvalidLength);
        };
        Ok(Rgb888::new(*r, *g, *b))
    }
}
impl PixelFormat for Gray8Bytes {
    type Color = Gray8;
    const BYTES_PER_PIXEL: usize = 1;
    fn decode(bytes: &[u8]) -> Result<Gray8, DecodeError> {
        let [value] = bytes else {
            return Err(DecodeError::InvalidLength);
        };
        Ok(Gray8::new(*value))
    }
}
impl PixelFormat for Mono8 {
    type Color = BinaryColor;
    const BYTES_PER_PIXEL: usize = 1;
    fn decode(bytes: &[u8]) -> Result<BinaryColor, DecodeError> {
        match bytes {
            [0] => Ok(BinaryColor::Off),
            [1] => Ok(BinaryColor::On),
            [_] => Err(DecodeError::InvalidValue),
            _ => Err(DecodeError::InvalidLength),
        }
    }
}
