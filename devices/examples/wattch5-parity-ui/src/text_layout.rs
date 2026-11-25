//! Rejects overflow explicitly, preserves UTF-8 boundaries, no silent truncation.
use crate::{Locale, Model, Screen, Snapshot, UiError, glyph};
fn width(text: &str, scale: usize) -> Result<usize, UiError> {
    text.chars().try_fold(0_usize, |n, ch| {
        Ok(n + (if glyph(ch)?.width == 5 { 6 } else { 12 }) * scale)
    })
}
pub(crate) fn lines(text: &str, scale: usize) -> Result<[&str; 3], UiError> {
    let max = if text.is_ascii() { 3 } else { 2 };
    let mut lines = [""; 3];
    let mut current = 0;
    let mut start = 0;
    let mut used = 0;
    for (index, ch) in text.char_indices() {
        let advance = (if glyph(ch)?.width == 5 { 6 } else { 12 }) * scale;
        if used + advance > 112 {
            if current + 1 >= max {
                return Err(UiError::TextOverflow);
            }
            lines[current] = text.get(start..index).ok_or(UiError::TextOverflow)?;
            current += 1;
            start = index;
            used = 0;
        }
        used += advance;
    }
    lines[current] = text.get(start..).ok_or(UiError::TextOverflow)?;
    Ok(lines)
}
pub(crate) fn validate(m: &Model, d: &Snapshot<'_>) -> Result<(), UiError> {
    if !matches!(m.screen(), Screen::MessageDetail | Screen::NoticeApp) {
        return Ok(());
    }
    let Some(body) = d.messages.get(m.message_index()) else {
        return Ok(());
    };
    if m.appearance().locale == Locale::En {
        lines(body.en, if m.appearance().textsize == 0 { 1 } else { 2 })?;
    } else if m.appearance().textsize == 0 {
        if width(body.zh, 1)?.saturating_sub(1) > 112 {
            return Err(UiError::TextOverflow);
        }
    } else {
        let end = body
            .zh
            .char_indices()
            .nth(4)
            .map_or(body.zh.len(), |(i, _)| i);
        if width(body.zh.get(end..).ok_or(UiError::TextOverflow)?, 2)?.saturating_sub(2) > 112 {
            return Err(UiError::TextOverflow);
        }
    }
    Ok(())
}
