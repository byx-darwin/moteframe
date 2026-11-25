//! Private factory skin. Lossless RGB565 rows, no framebuffer or hardware owner.
use crate::{Effect, Event, ImageError, MeasureKind, Model, Reading, Screen, Snapshot, UiError};
use core::cell::{Cell, RefCell};
use device_ui::graphics::{
    pixelcolor::{Rgb565, raw::RawU16},
    prelude::*,
    primitives::Rectangle,
};
use moteframe_core::ReadAt;

/// Checked factory resource reader. The source grants no write capability.
pub struct FactoryAssets<R> {
    source: RefCell<R>,
    rows: u32,
    end: u32,
    scene: Cell<(u16, u32)>,
}
impl<R> core::fmt::Debug for FactoryAssets<R> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FactoryAssets").finish_non_exhaustive()
    }
}
fn word(raw: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([raw[at], raw[at + 1], raw[at + 2], raw[at + 3]])
}
impl<R: ReadAt> FactoryAssets<R> {
    /// Mount a factory catalogue within the immutable lower2MiB partition.
    /// # Errors
    /// Rejects foreign geometry, version, extent and source failures.
    pub fn open(mut source: R) -> Result<Self, ImageError<R::Error>> {
        let mut h = [0; 32];
        source.read_exact(0, &mut h).map_err(ImageError::Source)?;
        let rows = word(&h, 20);
        let end = word(&h, 24);
        if &h[..8] != b"W5UIF001"
            || word(&h, 8) != 1
            || word(&h, 12) != 2336
            || word(&h, 16) != 9376
            || rows <= 9376
            || !(rows - 9376).is_multiple_of(256)
            || end <= rows
            || !(end - rows).is_multiple_of(256)
            || end > 0x20_0000
            || source.capacity() < end
            || word(&h, 28) == 0
            || word(&h, 28) > (rows - 9376) / 256
        {
            return Err(ImageError::Format);
        }
        Ok(Self {
            source: RefCell::new(source),
            rows,
            end,
            scene: Cell::new((u16::MAX, 0)),
        })
    }
    /// Read a full source-colored row. No theme recoloring or retro overlay.
    /// # Errors
    /// Rejects missing/invalid references and preserves source errors. On a source
    /// error the scratch row may be partial; discard it without a panel write.
    #[inline(never)]
    pub fn row(
        &self,
        m: &Model,
        y: u16,
        out: &mut [Rgb565; 128],
    ) -> Result<(), ImageError<R::Error>> {
        let mut source = self.source.try_borrow_mut().map_err(|_| ImageError::Busy)?;
        let at = self.row_address(&mut *source, m.background_key(), y)?;
        Self::span(&mut *source, at, out)
    }
    fn row_address(&self, source: &mut R, key: u16, y: u16) -> Result<u32, ImageError<R::Error>> {
        if key >= 2336 || y >= 128 {
            return Err(ImageError::Format);
        }
        let (cached, mut map) = self.scene.get();
        if cached != key {
            let mut b = [0; 4];
            source
                .read_exact(32 + u32::from(key) * 4, &mut b)
                .map_err(ImageError::Source)?;
            map = u32::from_le_bytes(b);
            if map < 9376
                || !(map - 9376).is_multiple_of(256)
                || map.checked_add(256).is_none_or(|v| v > self.rows)
            {
                return Err(ImageError::Format);
            }
            self.scene.set((key, map));
        }
        let mut b = [0; 2];
        source
            .read_exact(map + u32::from(y) * 2, &mut b)
            .map_err(ImageError::Source)?;
        let at = self.rows + u32::from(u16::from_le_bytes(b)) * 256;
        if at.checked_add(256).is_none_or(|v| v > self.end) {
            return Err(ImageError::Format);
        }
        Ok(at)
    }
    #[inline(never)]
    fn span(source: &mut R, at: u32, out: &mut [Rgb565]) -> Result<(), ImageError<R::Error>> {
        // Caller-owned row scratch may change on error. The product adapter
        // publishes no panel pixels until the entire row succeeds.
        let mut bytes = [0; 64];
        for (index, chunk) in out.chunks_mut(32).enumerate() {
            let offset = u32::try_from(index * 64).map_err(|_| ImageError::Format)?;
            source
                .read_exact(at + offset, &mut bytes[..chunk.len() * 2])
                .map_err(ImageError::Source)?;
            for (pixel, pair) in chunk.iter_mut().zip(bytes.chunks_exact(2)) {
                *pixel = Rgb565::from(RawU16::new(u16::from_be_bytes([pair[0], pair[1]])));
            }
        }
        Ok(())
    }
    /// Compose a factory row with only caller-supplied health/sleep observations.
    /// Unbound mandatory scalars remain the pack's missing markers.
    /// # Errors
    /// Returns invalid snapshots, unavailable resources and source failures.
    pub fn render(
        &self,
        m: &Model,
        d: &Snapshot<'_>,
        y: u16,
        out: &mut [Rgb565; 128],
    ) -> Result<(), ImageError<R::Error>> {
        let mut model = *m;
        prepare_model(&mut model, d).map_err(|_| ImageError::Format)?;
        let m = &model;
        self.row(m, y, out)?;
        if uses_portable_body(m.screen()) {
            return draw_portable(m, d, y, out).map_err(|_| ImageError::Format);
        }
        self.observations(m, d, y, out)
    }
    #[inline(never)]
    fn observations(
        &self,
        m: &Model,
        d: &Snapshot<'_>,
        y: u16,
        out: &mut [Rgb565; 128],
    ) -> Result<(), ImageError<R::Error>> {
        draw_feedback(m, d, y, out).map_err(|_| ImageError::Format)?;
        match m.screen() {
            Screen::Heart => {
                if let Reading::Number(v) = reading(d, MeasureKind::Heart) {
                    self.digits(v, (&[36, 51, 66], 26, None), y, out)?;
                }
            }
            Screen::Oxygen => {
                if let Reading::Number(v) = reading(d, MeasureKind::Oxygen) {
                    self.digits(v, (&[36, 51, 66], 25, None), y, out)?;
                }
            }
            Screen::Temperature => {
                let value = reading(d, MeasureKind::Temperature);
                if value != Reading::Missing && (94..111).contains(&y) {
                    out[16..86].fill(Rgb565::new(0, 0, 0));
                }
                match value {
                    Reading::Number(v) => self.digits(v, (&[30, 47, 73], 94, None), y, out)?,
                    Reading::Tenth(v) => self.digits(
                        v,
                        (
                            if v >= 1000 {
                                &[16, 33, 50, 73]
                            } else {
                                &[30, 47, 73]
                            },
                            94,
                            Some(64),
                        ),
                        y,
                        out,
                    )?,
                    _ => {}
                }
            }
            Screen::Sleep => {
                if let Some(v) = d.sleep_total {
                    self.digits(v / 6, (&[32, 48, 71], 88, Some(64)), y, out)?;
                }
            }
            Screen::Pressure => {
                if let Reading::Pressure { upper, lower } = reading(d, MeasureKind::Pressure) {
                    self.digits(upper, (&[19, 34, 49], 33, None), y, out)?;
                    self.digits(lower, (&[75, 90, 105], 33, None), y, out)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn digits(
        &self,
        value: u16,
        geometry: (&[u16], u16, Option<u16>),
        y: u16,
        out: &mut [Rgb565; 128],
    ) -> Result<(), ImageError<R::Error>> {
        let (columns, top, point) = geometry;
        if y < top || y >= top + 17 {
            return Ok(());
        }
        let count = u32::try_from(columns.len()).map_err(|_| ImageError::Format)?;
        if count == 0 || count > 4 || u32::from(value) >= 10u32.pow(count) {
            return Err(ImageError::Format);
        }
        let mut source = self.source.try_borrow_mut().map_err(|_| ImageError::Busy)?;
        let mut divisor = 10u16.pow(count - 1);
        for &start in columns {
            let at = self.row_address(&mut *source, 2232 + value / divisor % 10, y - top)?;
            let pixels = out
                .get_mut(usize::from(start)..usize::from(start + 13))
                .ok_or(ImageError::Format)?;
            Self::span(&mut *source, at, pixels)?;
            divisor /= 10;
        }
        if let Some(x) = point
            && y >= top + 13
            && y < top + 15
        {
            out.get_mut(usize::from(x)..usize::from(x + 2))
                .ok_or(ImageError::Format)?
                .fill(Rgb565::new(31, 63, 31));
        }
        Ok(())
    }
}
#[inline(never)]
fn prepare_model(m: &mut Model, d: &Snapshot<'_>) -> Result<(), UiError> {
    let scene = m.product_scene(d)?;
    *m = *scene.background_model();
    Ok(())
}

#[inline(never)]
fn draw_portable(
    m: &Model,
    d: &Snapshot<'_>,
    y: u16,
    out: &mut [Rgb565; 128],
) -> Result<(), crate::RenderError<()>> {
    let mut target = FactoryRow { y, out };
    let bottom = if m.screen() == Screen::MeasureConfirm {
        96
    } else {
        128
    };
    let clip = Rectangle::new(Point::new(0, 21), Size::new(128, bottom - 21))
        .intersection(&target.bounding_box());
    let mut canvas = crate::canvas::Canvas::<_, true> {
        target: &mut target,
        clip,
        colors: [
            Rgb565::new(0, 0, 0),
            Rgb565::new(31, 63, 31),
            Rgb565::new(0, 63, 20),
            Rgb565::new(17, 34, 17),
        ],
        base: false,
        #[cfg(test)]
        visited_rows: 0,
    };
    canvas.rect(0, 21, 128, bottom - 21, 0)?;
    canvas.content(m, d)?;
    Ok(())
}
fn uses_portable_body(page: Screen) -> bool {
    matches!(
        page,
        Screen::Weather
            | Screen::Charge
            | Screen::Ota
            | Screen::SportCountdown
            | Screen::SportRun
            | Screen::Glucose
            | Screen::Hba1c
            | Screen::Messages
            | Screen::MessageDetail
            | Screen::NoticeApp
            | Screen::Request
            | Screen::TimerCustom
            | Screen::TimerRun
            | Screen::Stopwatch
            | Screen::About
            | Screen::DeviceQr
            | Screen::MeasureTips
            | Screen::MeasureConfirm
            | Screen::SportEnd
            | Screen::Reset
            | Screen::Alarm
            | Screen::Format
            | Screen::Theme
            | Screen::Applet
            | Screen::Lock
            | Screen::Call
            | Screen::NoticeDisconnect
    )
}
struct FactoryRow<'a> {
    y: u16,
    out: &'a mut [Rgb565; 128],
}
impl device_ui::graphics::geometry::Dimensions for FactoryRow<'_> {
    fn bounding_box(&self) -> Rectangle {
        Rectangle::new(Point::new(0, i32::from(self.y)), Size::new(128, 1))
    }
}
impl DrawTarget for FactoryRow<'_> {
    type Color = Rgb565;
    type Error = ();
    fn draw_iter<T: IntoIterator<Item = Pixel<Rgb565>>>(&mut self, pixels: T) -> Result<(), ()> {
        for Pixel(p, c) in pixels {
            if p.y != i32::from(self.y) {
                return Err(());
            }
            *self
                .out
                .get_mut(usize::try_from(p.x).map_err(|_| ())?)
                .ok_or(())? = c;
        }
        Ok(())
    }
}
fn reading(d: &Snapshot<'_>, kind: MeasureKind) -> Reading {
    let m = d.measurement(kind);
    if matches!(
        m.state,
        crate::MeasureState::Idle | crate::MeasureState::Unavailable
    ) {
        Reading::Missing
    } else {
        m.reading
    }
}
#[inline(never)]
fn draw_feedback(
    m: &Model,
    d: &Snapshot<'_>,
    y: u16,
    out: &mut [Rgb565; 128],
) -> Result<(), crate::RenderError<()>> {
    let (kind, top) = match m.screen() {
        Screen::Heart => (MeasureKind::Heart, 84),
        Screen::Pressure => (MeasureKind::Pressure, 116),
        Screen::Oxygen => (MeasureKind::Oxygen, 116),
        Screen::Temperature => (MeasureKind::Temperature, 116),
        _ => return Ok(()),
    };
    if y < top || y >= top + 12 {
        return Ok(());
    }
    let label = match d.measurement(kind).state {
        crate::MeasureState::Idle => crate::Label::Idle,
        crate::MeasureState::Pending => crate::Label::Pending,
        crate::MeasureState::Measuring => crate::Label::Measuring,
        crate::MeasureState::Result => crate::Label::Result,
        crate::MeasureState::Error => crate::Label::Error,
        crate::MeasureState::Unavailable => crate::Label::Unavailable,
    };
    let mut target = FactoryRow { y, out };
    let clip = target.bounding_box();
    let mut canvas = crate::canvas::Canvas::<_, true> {
        target: &mut target,
        clip,
        colors: [
            Rgb565::new(0, 0, 0),
            Rgb565::new(31, 63, 31),
            Rgb565::new(0, 63, 20),
            Rgb565::new(17, 34, 17),
        ],
        base: false,
        #[cfg(test)]
        visited_rows: 0,
    };
    canvas.rect(
        0,
        u32::from(top)
            .try_into()
            .map_err(|_| crate::RenderError::Bounds)?,
        128,
        12,
        0,
    )?;
    canvas.center(label.text(m.appearance().locale), i32::from(top), 1, 1)
}

impl Model {
    /// Normalize original bitmap hit rectangles into the existing transactional model.
    /// No service is executed here. Yes is on the left in the factory artwork.
    /// # Errors
    /// Invalid input/snapshot or navigation depth rejects without changing the model.
    pub fn handle_factory(&mut self, event: Event, data: &Snapshot<'_>) -> Result<Effect, UiError> {
        data.validate()?;
        let mut next = *self;
        let mut event = event;
        if next.screen() == Screen::Language && matches!(event, Event::Up | Event::Down) {
            next.draft = u8::from(event == Event::Up);
            *self = next;
            return Ok(Effect::None);
        }
        if let Event::Tap { x, y } = event {
            if x >= 128 || y >= 128 {
                return self.handle(event, data);
            }
            match next.screen() {
                Screen::Quick => {
                    let target = match (x, y) {
                        (2..42, 30..70) => Screen::DeviceQr,
                        (44..84, 30..70) => Screen::Brightness,
                        (86..126, 30..70) => Screen::Settings,
                        _ => return Ok(Effect::None),
                    };
                    next.open(target, data)?;
                    *self = next;
                    return Ok(Effect::None);
                }
                Screen::Music => {
                    event = match (x, y) {
                        (1..20, 64..78) => Event::Tap { x: 20, y: 40 },
                        (56..73, 62..75) => Event::Tap { x: 64, y: 40 },
                        (108..127, 64..78) => Event::Tap { x: 100, y: 40 },
                        (20..36, 107..123) => Event::Tap { x: 100, y: 80 },
                        (93..109, 115..119) => Event::Tap { x: 20, y: 80 },
                        _ => return Ok(Effect::None),
                    };
                }
                Screen::Clear | Screen::Restart | Screen::Power | Screen::MeasureConfirm => {
                    if !(106..128).contains(&y) || !((4..61).contains(&x) || (67..124).contains(&x))
                    {
                        return Ok(Effect::None);
                    }
                    event = Event::Tap { x: 127 - x, y: 115 };
                }
                Screen::Language => {
                    if (38..90).contains(&x) && ((37..52).contains(&y) || (64..79).contains(&y)) {
                        next.draft = u8::from(y >= 64);
                        *self = next;
                        return Ok(Effect::None);
                    }
                    if y < 103 {
                        return Ok(Effect::None);
                    }
                }
                Screen::Settings | Screen::Tools | Screen::SportList => {
                    if !(4..126).contains(&x)
                        || !((6..42).contains(&y)
                            || (46..82).contains(&y)
                            || (86..122).contains(&y))
                    {
                        return Ok(Effect::None);
                    }
                }
                Screen::Brightness | Screen::Textsize if y < 103 => return Ok(Effect::None),
                _ => {}
            }
        }
        let effect = next.handle(event, data)?;
        *self = next;
        Ok(effect)
    }
}
