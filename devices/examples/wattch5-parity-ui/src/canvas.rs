//! Bounded clipped primitives and shared integer font geometry.
use crate::{Model, Snapshot, TimeFormat, Weekday, glyph};
use core::fmt::{self, Write};
use device_ui::graphics::{pixelcolor::Rgb565, prelude::*, primitives::Rectangle};
/// Invalid data/layout, unavailable glyph or display error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderError<E> {
    /// Invalid geometry or palette.
    Bounds,
    /// Bounded formatting overflow.
    Text,
    /// Unsupported external glyph.
    UnsupportedGlyph,
    /// Caller data rejected before drawing.
    Snapshot(crate::UiError),
    /// Display target failure.
    Target(E),
}
const fn rgb(region: u8, glyph_data: u8, b: u8) -> u16 {
    ((region as u16 >> 3) << 11) | ((glyph_data as u16 >> 2) << 5) | (b as u16 >> 3)
}
pub(crate) const COLORS: [[u16; 4]; 3] = [
    [
        rgb(7, 19, 13),
        rgb(216, 235, 200),
        rgb(184, 228, 107),
        rgb(137, 161, 123),
    ],
    [
        rgb(28, 18, 7),
        rgb(244, 219, 176),
        rgb(255, 193, 90),
        rgb(182, 155, 114),
    ],
    [
        rgb(11, 21, 32),
        rgb(216, 233, 243),
        rgb(134, 199, 243),
        rgb(130, 156, 175),
    ],
];
const SEGMENT_MASKS: [u8; 10] = [63, 6, 91, 79, 102, 109, 125, 7, 127, 111];
const SEGMENT_BARS: [[u8; 4]; 7] = [
    [3, 0, 14, 3],
    [17, 3, 3, 14],
    [17, 20, 3, 14],
    [3, 34, 14, 3],
    [0, 20, 3, 14],
    [0, 3, 3, 14],
    [3, 17, 14, 3],
];

#[derive(Debug)]
pub(crate) struct Text {
    bytes: [u8; 96],
    len: usize,
}
impl Default for Text {
    fn default() -> Self {
        Self {
            bytes: [0; 96],
            len: 0,
        }
    }
}
impl Write for Text {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let end = self.len.checked_add(s.len()).ok_or(fmt::Error)?;
        self.bytes
            .get_mut(self.len..end)
            .ok_or(fmt::Error)?
            .copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(())
    }
}
impl Text {
    pub(crate) fn value(&self) -> Result<&str, fmt::Error> {
        core::str::from_utf8(&self.bytes[..self.len]).map_err(|_| fmt::Error)
    }
}
pub(crate) struct Canvas<'a, D, const PRODUCT: bool = false> {
    pub(crate) target: &'a mut D,
    pub(crate) clip: Rectangle,
    pub(crate) colors: [Rgb565; 4],
    pub(crate) base: bool,
    #[cfg(test)]
    pub(crate) visited_rows: usize,
}
impl<D: DrawTarget<Color = Rgb565>, const PRODUCT: bool> Canvas<'_, D, PRODUCT> {
    pub(crate) fn rect(
        &mut self,
        x: i32,
        y: i32,
        w: u32,
        h: u32,
        color_index: usize,
    ) -> Result<(), RenderError<D::Error>> {
        if x < 0
            || y < 0
            || u32::try_from(x)
                .map_err(|_| RenderError::Bounds)?
                .checked_add(w)
                .is_none_or(|n| n > 128)
            || u32::try_from(y)
                .map_err(|_| RenderError::Bounds)?
                .checked_add(h)
                .is_none_or(|n| n > 128)
        {
            return Err(RenderError::Bounds);
        }
        let region = Rectangle::new(Point::new(x, y), Size::new(w, h)).intersection(&self.clip);
        let color = *self.colors.get(color_index).ok_or(RenderError::Bounds)?;
        if region.size != Size::zero() {
            self.target
                .fill_solid(&region, color)
                .map_err(RenderError::Target)?;
        }
        Ok(())
    }
    pub(crate) fn width(s: &str, scale: i32) -> Result<i32, RenderError<D::Error>> {
        let mut w = 0_i32;
        for ch in s.chars() {
            let glyph_data = glyph(ch).map_err(|_| RenderError::UnsupportedGlyph)?;
            w = w
                .checked_add(if glyph_data.width == 5 { 6 } else { 12 })
                .ok_or(RenderError::Text)?;
        }
        Ok((w - scale.signum()).max(0) * scale)
    }
    pub(crate) fn text(
        &mut self,
        s: &str,
        mut x: i32,
        y: i32,
        scale: i32,
        color_index: usize,
    ) -> Result<(), RenderError<D::Error>> {
        if scale <= 0 {
            return Err(RenderError::Bounds);
        }
        for ch in s.chars() {
            let glyph_data = glyph(ch).map_err(|_| RenderError::UnsupportedGlyph)?;
            let start = if self.base && glyph_data.width == 5 {
                2
            } else {
                0
            };
            let end = if self.base && glyph_data.width == 5 {
                9
            } else {
                12
            };
            let rows = &glyph_data.rows[start..end];
            let ink = rows.iter().fold(0u16, |a, b| a | b);
            let clip_right = self
                .clip
                .top_left
                .x
                .saturating_add_unsigned(self.clip.size.width);
            let clip_bottom = self
                .clip
                .top_left
                .y
                .saturating_add_unsigned(self.clip.size.height);
            if ink != 0 {
                let first_col = i32::from(glyph_data.width)
                    - i32::try_from(16 - ink.leading_zeros()).map_err(|_| RenderError::Bounds)?;
                let last_col = i32::from(glyph_data.width)
                    - i32::try_from(ink.trailing_zeros()).map_err(|_| RenderError::Bounds)?;
                let first_row = i32::try_from(
                    rows.iter()
                        .position(|v| *v != 0)
                        .ok_or(RenderError::Bounds)?,
                )
                .map_err(|_| RenderError::Bounds)?;
                let last_row = i32::try_from(
                    rows.iter()
                        .rposition(|v| *v != 0)
                        .ok_or(RenderError::Bounds)?
                        + 1,
                )
                .map_err(|_| RenderError::Bounds)?;
                let left = x
                    .checked_add(first_col.checked_mul(scale).ok_or(RenderError::Bounds)?)
                    .ok_or(RenderError::Bounds)?;
                let right = x
                    .checked_add(last_col.checked_mul(scale).ok_or(RenderError::Bounds)?)
                    .ok_or(RenderError::Bounds)?;
                let top = y
                    .checked_add(first_row.checked_mul(scale).ok_or(RenderError::Bounds)?)
                    .ok_or(RenderError::Bounds)?;
                let bottom = y
                    .checked_add(last_row.checked_mul(scale).ok_or(RenderError::Bounds)?)
                    .ok_or(RenderError::Bounds)?;
                if left < 0
                    || top < 0
                    || right > 128
                    || bottom > 128
                    || color_index >= self.colors.len()
                {
                    return Err(RenderError::Bounds);
                }
                if left < clip_right
                    && right > self.clip.top_left.x
                    && top < clip_bottom
                    && bottom > self.clip.top_left.y
                {
                    for row in start..end {
                        let dy = y + i32::try_from(row - start).map_err(|_| RenderError::Bounds)?
                            * scale;
                        if dy >= clip_bottom || dy + scale <= self.clip.top_left.y {
                            continue;
                        }
                        #[cfg(test)]
                        {
                            self.visited_rows += 1;
                        }
                        for col in 0..glyph_data.width {
                            if glyph_data.rows[row] & (1 << (glyph_data.width - 1 - col)) != 0 {
                                self.rect(
                                    x + i32::from(col) * scale,
                                    dy,
                                    u32::try_from(scale).map_err(|_| RenderError::Bounds)?,
                                    u32::try_from(scale).map_err(|_| RenderError::Bounds)?,
                                    color_index,
                                )?;
                            }
                        }
                    }
                }
            }
            x += if glyph_data.width == 5 { 6 } else { 12 } * scale;
        }
        Ok(())
    }
    pub(crate) fn center(
        &mut self,
        s: &str,
        y: i32,
        scale: i32,
        color_index: usize,
    ) -> Result<(), RenderError<D::Error>> {
        self.text(
            s,
            (128 - Self::width(s, scale)?).div_euclid(2),
            y,
            scale,
            color_index,
        )
    }
    pub(crate) fn formatted(
        &mut self,
        args: fmt::Arguments<'_>,
        x: Option<i32>,
        y: i32,
        scale: i32,
        color_index: usize,
    ) -> Result<(), RenderError<D::Error>> {
        let mut text = Text::default();
        text.write_fmt(args).map_err(|_| RenderError::Text)?;
        let s = text.value().map_err(|_| RenderError::Text)?;
        if let Some(x) = x {
            self.text(s, x, y, scale, color_index)
        } else {
            self.center(s, y, scale, color_index)
        }
    }
    pub(crate) fn header(&mut self, title: &str, right: &str) -> Result<(), RenderError<D::Error>> {
        self.text(title, 8, 7, 1, 2)?;
        self.text(right, 120 - Self::width(right, 1)?, 7, 1, 3)?;
        self.rect(8, 19, 112, 1, 3)
    }
    fn seven(&mut self, s: &str, mut x: i32, y: i32) -> Result<(), RenderError<D::Error>> {
        for ch in s.bytes() {
            if ch == b':' {
                self.rect(x + 1, y + 10, 2, 3, 2)?;
                self.rect(x + 1, y + 22, 2, 3, 2)?;
                x += 7;
            } else {
                let index = usize::from(ch.checked_sub(b'0').ok_or(RenderError::UnsupportedGlyph)?);
                let mask = *SEGMENT_MASKS
                    .get(index)
                    .ok_or(RenderError::UnsupportedGlyph)?;
                for (i, region) in SEGMENT_BARS.iter().enumerate() {
                    if mask & (1 << i) != 0 {
                        self.rect(
                            x + i32::from(region[0]),
                            y + i32::from(region[1]),
                            u32::from(region[2]),
                            u32::from(region[3]),
                            2,
                        )?;
                    }
                }
                x += 24;
            }
        }
        Ok(())
    }
    fn product_dial(
        &mut self,
        m: &Model,
        face: u8,
        preview: bool,
    ) -> Result<(), RenderError<D::Error>> {
        let mut top = Text::default();
        if preview {
            write!(&mut top, "FACE {}/3", m.draft + 1).map_err(|_| RenderError::Text)?;
        } else {
            top.write_str("WATTCH 5").map_err(|_| RenderError::Text)?;
        }
        let top = top.value().map_err(|_| RenderError::Text)?;
        match face {
            0 => {
                self.text(top, 8, 6, 1, 2)?;
                self.text(
                    if m.appearance.format == TimeFormat::Hour24 {
                        "24H"
                    } else {
                        "12H"
                    },
                    94,
                    6,
                    1,
                    3,
                )?;
                self.text("--", 8, 23, if preview { 5 } else { 6 }, 1)?;
                self.text(
                    "--",
                    8,
                    if preview { 60 } else { 74 },
                    if preview { 5 } else { 6 },
                    1,
                )?;
                self.rect(81, 23, 1, if preview { 73 } else { 93 }, 3)?;
                self.text("--", 90, 25, 2, 2)?;
                self.text("--", 90, 46, 2, 2)?;
                self.text("--", 90, 74, 1, 1)?;
                if !preview {
                    self.text("--%", 90, 96, 1, 2)?;
                }
            }
            1 | 2 => {
                self.header(
                    top,
                    if m.appearance.format == TimeFormat::Hour24 {
                        "24H"
                    } else {
                        "12H"
                    },
                )?;
                self.center(
                    "--:--",
                    if face == 1 { 39 } else { 50 },
                    if face == 1 { 4 } else { 3 },
                    if face == 1 { 1 } else { 2 },
                )?;
                self.center("-- --/--", if face == 1 { 81 } else { 29 }, 1, 2)?;
                if !preview {
                    self.center("--%", 107, 1, 3)?;
                }
            }
            _ => return Err(RenderError::Bounds),
        }
        Ok(())
    }
    pub(crate) fn dial(
        &mut self,
        m: &Model,
        data: &Snapshot<'_>,
        face: u8,
        preview: bool,
    ) -> Result<(), RenderError<D::Error>> {
        if PRODUCT {
            return self.product_dial(m, face, preview);
        }
        let d = *data;
        let h = if m.appearance.format == TimeFormat::Hour12 {
            if d.hour.is_multiple_of(12) {
                12
            } else {
                d.hour % 12
            }
        } else {
            d.hour
        };
        let mut top = Text::default();
        if preview {
            write!(&mut top, "FACE {}/3", m.draft + 1).map_err(|_| RenderError::Text)?;
        } else {
            top.write_str("WATTCH 5").map_err(|_| RenderError::Text)?;
        }
        let top = top.value().map_err(|_| RenderError::Text)?;
        match face {
            0 => {
                self.text(top, 8, 6, 1, 2)?;
                let right = if preview {
                    "DEMO"
                } else if m.appearance.format == TimeFormat::Hour12 {
                    if d.hour < 12 { "AM" } else { "PM" }
                } else {
                    "24H"
                };
                self.text(right, 94, 6, 1, 3)?;
                self.formatted(
                    format_args!("{h:02}"),
                    Some(8),
                    23,
                    if preview { 5 } else { 6 },
                    1,
                )?;
                self.formatted(
                    format_args!("{:02}", d.minute),
                    Some(8),
                    if preview { 60 } else { 74 },
                    if preview { 5 } else { 6 },
                    1,
                )?;
                self.rect(81, 23, 1, if preview { 73 } else { 93 }, 3)?;
                self.formatted(format_args!("{:02}", d.month), Some(90), 25, 2, 2)?;
                self.formatted(format_args!("{:02}", d.day), Some(90), 46, 2, 2)?;
                self.text(weekday(d.weekday), 90, 74, 1, 1)?;
                if !preview {
                    self.formatted(format_args!("{}%", d.battery), Some(90), 96, 1, 2)?;
                    self.text("DEMO", 90, 108, 1, 3)?;
                }
            }
            1 => {
                self.header(top, if preview { "DEMO" } else { "24H" })?;
                self.formatted(format_args!("{h:02}:{:02}", d.minute), Some(7), 39, 4, 1)?;
                self.formatted(
                    format_args!("{} {:02}/{:02}", weekday(d.weekday), d.month, d.day),
                    None,
                    81,
                    1,
                    2,
                )?;
                self.formatted(format_args!("{}%  DEMO", d.battery), None, 107, 1, 3)?;
            }
            2 => {
                self.header(top, if preview { "DEMO" } else { "24H" })?;
                self.formatted(
                    format_args!("{} {:02}/{:02}", weekday(d.weekday), d.month, d.day),
                    None,
                    29,
                    1,
                    1,
                )?;
                let mut digits = Text::default();
                write!(&mut digits, "{h:02}:{:02}", d.minute).map_err(|_| RenderError::Text)?;
                self.seven(digits.value().map_err(|_| RenderError::Text)?, 12, 50)?;
                self.formatted(format_args!("{}%  DEMO", d.battery), None, 107, 1, 3)?;
            }
            _ => return Err(RenderError::Bounds),
        }
        if !preview && face != 0 && m.appearance.format == TimeFormat::Hour12 {
            self.rect(95, 5, 25, 11, 0)?;
            self.text(if d.hour < 12 { "AM" } else { "PM" }, 108, 7, 1, 3)?;
        }
        Ok(())
    }
}
const fn weekday(value: Weekday) -> &'static str {
    match value {
        Weekday::Mon => "MON",
        Weekday::Tue => "TUE",
        Weekday::Wed => "WED",
        Weekday::Thu => "THU",
        Weekday::Fri => "FRI",
        Weekday::Sat => "SAT",
        Weekday::Sun => "SUN",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use device_ui::graphics::mock_display::MockDisplay;
    #[test]
    fn test_should_visit_only_visible_glyph_rows() {
        let mut target = MockDisplay::<Rgb565>::new();
        let mut c = Canvas::<_, false> {
            target: &mut target,
            clip: Rectangle::new(Point::new(0, 30), Size::new(128, 1)),
            colors: [Rgb565::new(0, 0, 0); 4],
            base: false,
            visited_rows: 0,
        };
        assert!(c.text("HELLO", 0, 4, 1, 1).is_ok());
        assert_eq!(c.visited_rows, 0);
        assert!(c.text("HELLO", 0, 28, 1, 1).is_ok());
        assert_eq!(c.visited_rows, 5);
        assert!(matches!(
            c.text("🦀", 0, 4, 1, 1),
            Err(RenderError::UnsupportedGlyph)
        ));
    }
}
