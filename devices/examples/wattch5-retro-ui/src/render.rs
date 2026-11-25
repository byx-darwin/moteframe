//! Integer-only rendering of the approved indexed design, clipped before pixel iteration.
use crate::{
    Model, font,
    model::menu,
    types::{Notice, Screen, StopwatchState, TimeFormat, TimerState, UiRequest, Weekday},
};
use core::fmt::{self, Write};
use device_ui::graphics::{pixelcolor::Rgb565, prelude::*, primitives::Rectangle};
/// Scene validation, formatting, glyph or target failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderError<E> {
    /// A design primitive exceeds128x128 or an index is invalid.
    Bounds,
    /// Internal bounded formatting exceeded its capacity.
    Text,
    /// The font does not contain the requested character.
    UnsupportedGlyph,
    /// The draw target rejected pixels.
    Target(E),
}
const fn rgb(r: u8, g: u8, b: u8) -> u16 {
    ((r as u16 >> 3) << 11) | ((g as u16 >> 2) << 5) | (b as u16 >> 3)
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

#[derive(Default)]
struct Text {
    bytes: [u8; 32],
    len: usize,
}
impl Write for Text {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let end = self.len.checked_add(s.len()).ok_or(fmt::Error)?;
        let out = self.bytes.get_mut(self.len..end).ok_or(fmt::Error)?;
        out.copy_from_slice(s.as_bytes());
        self.len = end;
        Ok(())
    }
}
impl Text {
    fn value(&self) -> Result<&str, fmt::Error> {
        core::str::from_utf8(&self.bytes[..self.len]).map_err(|_| fmt::Error)
    }
}
struct Canvas<'a, D> {
    target: &'a mut D,
    clip: Rectangle,
    colors: [Rgb565; 4],
}
impl<D: DrawTarget<Color = Rgb565>> Canvas<'_, D> {
    fn rect(
        &mut self,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
        color: usize,
    ) -> Result<(), RenderError<D::Error>> {
        if x < 0
            || y < 0
            || u32::try_from(x)
                .map_err(|_| RenderError::Bounds)?
                .checked_add(width)
                .is_none_or(|e| e > 128)
            || u32::try_from(y)
                .map_err(|_| RenderError::Bounds)?
                .checked_add(height)
                .is_none_or(|e| e > 128)
        {
            return Err(RenderError::Bounds);
        }
        let rect =
            Rectangle::new(Point::new(x, y), Size::new(width, height)).intersection(&self.clip);
        let color = *self.colors.get(color).ok_or(RenderError::Bounds)?;
        if rect.size.width > 0 && rect.size.height > 0 {
            self.target
                .fill_solid(&rect, color)
                .map_err(RenderError::Target)?;
        }
        Ok(())
    }
    fn width(s: &str, scale: i32) -> i32 {
        if s.is_empty() {
            0
        } else {
            (i32::try_from(s.len()).unwrap_or(i32::MAX).saturating_mul(6) - 1).saturating_mul(scale)
        }
    }
    fn text(
        &mut self,
        s: &str,
        mut x: i32,
        y: i32,
        scale: i32,
        c: usize,
    ) -> Result<(), RenderError<D::Error>> {
        if scale <= 0 {
            return Err(RenderError::Bounds);
        }
        for byte in s.bytes() {
            let index = font::ASCII
                .iter()
                .position(|value| *value == byte)
                .ok_or(RenderError::UnsupportedGlyph)?;
            let glyph = Rectangle::new(
                Point::new(x, y),
                Size::new(
                    u32::try_from(5 * scale).map_err(|_| RenderError::Bounds)?,
                    u32::try_from(7 * scale).map_err(|_| RenderError::Bounds)?,
                ),
            );
            if glyph.top_left.x < 0
                || glyph.top_left.y < 0
                || glyph
                    .bottom_right()
                    .is_none_or(|p| p.x >= 128 || p.y >= 128)
            {
                return Err(RenderError::Bounds);
            }
            if glyph.intersection(&self.clip).size != Size::zero() {
                for (row, bits) in font::ROWS[index].iter().enumerate() {
                    for col in 0..5 {
                        if bits & (16 >> col) != 0 {
                            self.rect(
                                x + col * scale,
                                y + i32::try_from(row).map_err(|_| RenderError::Bounds)? * scale,
                                u32::try_from(scale).map_err(|_| RenderError::Bounds)?,
                                u32::try_from(scale).map_err(|_| RenderError::Bounds)?,
                                c,
                            )?;
                        }
                    }
                }
            }
            x += 6 * scale;
        }
        Ok(())
    }
    fn center(
        &mut self,
        s: &str,
        y: i32,
        scale: i32,
        c: usize,
    ) -> Result<(), RenderError<D::Error>> {
        self.text(s, (128 - Self::width(s, scale)) / 2, y, scale, c)
    }
    fn formatted(
        &mut self,
        args: fmt::Arguments<'_>,
        position: Option<i32>,
        y: i32,
        scale: i32,
        c: usize,
    ) -> Result<(), RenderError<D::Error>> {
        let mut text = Text::default();
        text.write_fmt(args).map_err(|_| RenderError::Text)?;
        let value = text.value().map_err(|_| RenderError::Text)?;
        if let Some(x) = position {
            self.text(value, x, y, scale, c)
        } else {
            self.center(value, y, scale, c)
        }
    }
    fn header(&mut self, title: &str, right: &str) -> Result<(), RenderError<D::Error>> {
        self.text(title, 8, 7, 1, 2)?;
        self.text(right, 120 - Self::width(right, 1), 7, 1, 3)?;
        self.rect(8, 19, 112, 1, 3)
    }
    fn button(&mut self, label: &str) -> Result<(), RenderError<D::Error>> {
        self.rect(4, 100, 120, 26, 2)?;
        self.center(label, 109, 1, 0)
    }
    fn footer(&mut self, label: &str) -> Result<(), RenderError<D::Error>> {
        self.center(label, 118, 1, 3)
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
                for (i, r) in SEGMENT_BARS.iter().enumerate() {
                    if mask & (1 << i) != 0 {
                        self.rect(
                            x + i32::from(r[0]),
                            y + i32::from(r[1]),
                            u32::from(r[2]),
                            u32::from(r[3]),
                            2,
                        )?;
                    }
                }
                x += 24;
            }
        }
        Ok(())
    }
    fn dial(&mut self, m: &Model, face: u8, preview: bool) -> Result<(), RenderError<D::Error>> {
        let d = m.data;
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
const fn request_label(value: UiRequest) -> &'static str {
    match value {
        UiRequest::TimerStart => "TIMER START",
        UiRequest::TimerPause => "TIMER PAUSE",
        UiRequest::TimerResume => "TIMER RESUME",
        UiRequest::TimerDataRequest => "TIMER DATA",
        UiRequest::StopwatchStart => "WATCH START",
        UiRequest::StopwatchPause => "WATCH PAUSE",
        UiRequest::StopwatchResume => "WATCH RESUME",
        UiRequest::StopwatchDataRequest => "WATCH DATA",
        UiRequest::DisplaySleep => "SCREEN OFF",
        UiRequest::DisplayToggle => "SCREEN KEY",
        UiRequest::Restart => "RESTART",
        UiRequest::PowerOff => "POWER OFF",
        UiRequest::ResetSettings => "RESET",
        UiRequest::BrightnessPreview(_) => "BRIGHT",
        UiRequest::AppearanceCommit { .. } => "APPEARANCE",
    }
}
impl Model {
    /// Draws all fifteen approved pages with no framebuffer, heap or external image package.
    /// The target bounding box is respected before iterating filled glyph pixels.
    /// # Errors
    /// Reports invalid scene coordinates/glyphs or target failures; callers retain dirty state.
    pub fn draw<D: DrawTarget<Color = Rgb565>>(
        &self,
        target: &mut D,
    ) -> Result<(), RenderError<D::Error>> {
        let theme = if self.page == Screen::Theme {
            usize::from(self.draft)
        } else {
            self.appearance.theme as usize
        };
        let colors = COLORS
            .get(theme)
            .ok_or(RenderError::Bounds)?
            .map(|p| Rgb565::from(device_ui::graphics::pixelcolor::raw::RawU16::new(p)));
        let clip = target.bounding_box();
        let mut c = Canvas {
            target,
            clip,
            colors,
        };
        c.rect(0, 0, 128, 128, 0)?;
        if self.page == Screen::Dial {
            return c.dial(self, self.appearance.face as u8, false);
        }
        if menu(self.page).is_some() {
            return c.menu(self);
        }

        if matches!(self.page, Screen::Timer | Screen::Stopwatch) {
            return c.timer(self);
        }
        c.page(self)
    }
}

impl<D: DrawTarget<Color = Rgb565>> Canvas<'_, D> {
    fn menu(&mut self, model: &Model) -> Result<(), RenderError<D::Error>> {
        let entries = menu(model.page).ok_or(RenderError::Bounds)?;

        let title = match model.page {
            Screen::Tools => "TOOLS",
            Screen::Quick => "QUICK",
            Screen::Settings => "SETTINGS",
            Screen::Display => "DISPLAY",
            Screen::System => "SYSTEM",
            _ => return Err(RenderError::Bounds),
        };
        let mut right = Text::default();
        write!(&mut right, "{}/3", model.focus + 1).map_err(|_| RenderError::Text)?;
        self.header(title, right.value().map_err(|_| RenderError::Text)?)?;
        for (i, (label, _)) in entries.iter().enumerate() {
            let y = 25 + i32::try_from(i).map_err(|_| RenderError::Bounds)? * 29;
            let selected = usize::from(model.focus) == i;
            if selected {
                self.rect(4, y, 120, 27, 2)?;
            }
            self.text(label, 10, y + 7, 2, usize::from(!selected))?;
        }
        self.footer("RIGHT BACK")
    }
    fn timer(&mut self, model: &Model) -> Result<(), RenderError<D::Error>> {
        let timer = model.page == Screen::Timer;
        self.header(if timer { "TIMER" } else { "STOPWATCH" }, "DEMO")?;
        let unavailable = if timer {
            model.data.timer_state == TimerState::Unavailable
        } else {
            model.data.stopwatch_state == StopwatchState::Unavailable
        };
        if unavailable {
            self.center("NO DATA", 44, 2, 1)?;
            self.center("DEMO VIEW", 75, 1, 3)?;
            self.button("REQUEST")?;
        } else {
            let (seconds, state) = if timer {
                (
                    u32::from(model.data.timer_seconds),
                    match model.data.timer_state {
                        TimerState::Idle => "IDLE",
                        TimerState::Running => "RUNNING",
                        TimerState::Paused => "PAUSED",
                        TimerState::Done => "DONE",
                        TimerState::Unavailable => return Err(RenderError::Bounds),
                    },
                )
            } else {
                (
                    model.data.stopwatch_centis / 100,
                    match model.data.stopwatch_state {
                        StopwatchState::Idle => "IDLE",
                        StopwatchState::Running => "RUNNING",
                        StopwatchState::Paused => "PAUSED",
                        StopwatchState::Unavailable => return Err(RenderError::Bounds),
                    },
                )
            };
            self.formatted(
                format_args!("{:02}:{:02}", seconds / 60, seconds % 60),
                None,
                37,
                3,
                1,
            )?;
            if timer {
                self.center(state, 72, 1, 2)?;
            } else {
                self.formatted(
                    format_args!(".{:02} {state}", model.data.stopwatch_centis % 100),
                    None,
                    72,
                    1,
                    2,
                )?;
            }
            self.button(match state {
                "RUNNING" => "PAUSE",
                "PAUSED" => "RESUME",
                _ => "START",
            })?;
        }
        Ok(())
    }
    fn page(&mut self, model: &Model) -> Result<(), RenderError<D::Error>> {
        match model.page {
            Screen::Brightness => {
                self.header("BRIGHT", "DEMO")?;
                self.formatted(format_args!("{}", model.draft + 1), None, 29, 5, 1)?;
                for i in 0..5 {
                    self.rect(
                        12 + i * 22,
                        77,
                        16,
                        12,
                        if i <= i32::from(model.draft) { 2 } else { 3 },
                    )?;
                }
                self.button("APPLY")?;
            }
            Screen::Face => {
                self.dial(model, model.draft, true)?;
                self.rect(0, 100, 128, 28, 0)?;
                self.button(match model.draft {
                    0 => "STACK",
                    1 => "LINE",
                    _ => "SEGMENT",
                })?;
            }
            Screen::Theme => {
                let mut right = Text::default();
                write!(&mut right, "{}/3", model.draft + 1).map_err(|_| RenderError::Text)?;
                self.header("THEME", right.value().map_err(|_| RenderError::Text)?)?;
                self.center(
                    match model.draft {
                        0 => "PHOSPHOR",
                        1 => "AMBER",
                        _ => "ICE",
                    },
                    34,
                    2,
                    1,
                )?;
                for i in 0..4 {
                    self.rect(
                        16 + i * 26,
                        65,
                        18,
                        18,
                        usize::try_from(i).map_err(|_| RenderError::Bounds)?,
                    )?;
                }
                self.button("APPLY")?;
            }
            Screen::Format => {
                self.header("FORMAT", "DEMO")?;
                self.center(if model.draft == 0 { "24H" } else { "12H" }, 32, 3, 1)?;
                self.center(
                    if model.draft == 0 {
                        "22:50"
                    } else {
                        "10:50 PM"
                    },
                    74,
                    1,
                    2,
                )?;
                self.button("APPLY")?;
            }
            Screen::About => {
                self.header("ABOUT", "DEMO")?;
                self.text("WATTCH 5", 8, 30, 2, 1)?;
                self.text("RETRO DIGITAL", 8, 54, 1, 2)?;
                self.text("DESIGN V1", 8, 70, 1, 1)?;
                self.text("128 X 128", 8, 86, 1, 1)?;
                self.footer("DEMO DATA ONLY")?;
            }

            Screen::Confirm => {
                self.header("CONFIRM", "DEMO")?;
                self.center(request_label(model.confirm), 37, 2, 1)?;
                self.center("REQUEST ONLY", 65, 1, 3)?;
                self.center("NO DEVICE ACTION", 80, 1, 3)?;
                self.rect(0, 96, 64, 32, 2)?;
                self.text("CANCEL", 14, 108, 1, 0)?;
                self.text("SEND", 84, 108, 1, 1)?;
            }
            Screen::Request => {
                self.header("REQUEST", "DEMO")?;
                self.center(
                    if model.notice == Notice::Unavailable {
                        "UNAVAILABLE"
                    } else {
                        "DEMO ONLY"
                    },
                    33,
                    1,
                    2,
                )?;
                self.center(request_label(model.request), 54, 1, 1)?;
                self.center("NO ACTION RUN", 77, 1, 3)?;
                self.button("BACK")?;
            }
            _ => return Err(RenderError::Bounds),
        }
        Ok(())
    }
}

/// Bytes of authored glyph/map/palette/segment source data, excluding code, labels and alignment.
pub const GRAPHIC_DATA_BYTES: usize = core::mem::size_of_val(font::ASCII)
    + core::mem::size_of_val(&font::ROWS)
    + core::mem::size_of_val(&COLORS)
    + core::mem::size_of_val(&SEGMENT_MASKS)
    + core::mem::size_of_val(&SEGMENT_BARS);
