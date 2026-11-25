//! Bounded local subset; unsupported characters are explicit errors.
use crate::{UiError, font_data as data};
/// Static graphic source bytes, excluding code, copy, alignment and model data.
pub const GRAPHIC_DATA_BYTES: usize = 6730;
/// Twelve rows of a glyph; low width bits are pixels, high bit is leftmost.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Glyph {
    /// Cell width5ASCII/12Chinese.
    pub width: u8,
    /// Rows; ASCII is aligned to rows2..8.
    pub rows: [u16; 12],
}
/// Looks up the licensed local subset, case-insensitive for ASCII.
/// # Errors
/// Returns `UnsupportedGlyph` for any unavailable codepoint.
pub fn glyph(ch: char) -> Result<Glyph, UiError> {
    if ch.is_ascii() {
        let b = ch.to_ascii_uppercase() as u8;
        let i = data::ASCII
            .iter()
            .position(|x| *x == b)
            .ok_or(UiError::UnsupportedGlyph)?;
        let mut rows = [0; 12];
        for (dst, v) in rows[2..9].iter_mut().zip(data::ASCII_ROWS[i]) {
            *dst = u16::from(v);
        }
        return Ok(Glyph { width: 5, rows });
    }
    let cp = u16::try_from(u32::from(ch)).map_err(|_| UiError::UnsupportedGlyph)?;
    let i = data::ZH_INDEX
        .binary_search(&cp)
        .map_err(|_| UiError::UnsupportedGlyph)?;
    Ok(Glyph {
        width: 12,
        rows: data::ZH_ROWS[i],
    })
}
