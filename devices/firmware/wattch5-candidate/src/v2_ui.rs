//! Complete V2 presentation over the existing independently scheduled product owners.
use crate::ui::{DeviceHealth, SettingsPort, health_value};
use core::cell::Cell;
use device_core::{Dimensions, Display};
use device_runtime::{PortError, Work};
use moteframe_ui::graphics::{pixelcolor::Rgb565 as Color, prelude::*, primitives::Rectangle};
use wattch5_bringup::{Event, InputSource, Readiness, ResetGate};
use wattch5_parity_ui_demo::{
    Effect, Field, MeasureKind, MeasureState, MeasurementSnapshot, Model, Snapshot, UiRequest,
};

/// Image-backed V2 adapter: one128-pixel row per turn, without a framebuffer.
/// The compatibility procedural renderer yields after32 pixels.
/// Storage admission never advances Flash and only confirmed readback updates durable settings.
pub struct V2Ui<'a, D, I, L> {
    display: D,
    input: I,
    light: L,
    gate: &'a ResetGate,
    settings: Option<&'a dyn SettingsPort>,
    health: Option<&'a dyn DeviceHealth>,
    model: Model,
    frame: Model,
    measurements: [MeasurementSnapshot; 3],
    cursor: Option<u16>,
    end: u16,
    pending: bool,
    settings_retry: bool,
    paint_retry: bool,
    on: bool,
    duty: u8,
    frames: u32,
    next_refresh: u64,
    request: Option<UiRequest>,
    background: Option<&'a dyn Background>,
}
impl<D, I, L> core::fmt::Debug for V2Ui<'_, D, I, L> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("V2Ui")
            .field("model", &self.model)
            .field("cursor", &self.cursor)
            .field("pending", &self.pending)
            .finish_non_exhaustive()
    }
}
impl<'a, D: Display, I: InputSource, L: FnMut(u8) -> Result<(), PortError>> V2Ui<'a, D, I, L> {
    /// Attach an initialized panel/input after the observed Host reset handover.
    /// # Errors
    /// A wrong panel size or unavailable UI ownership rejects construction.
    pub fn new(
        display: D,
        input: I,
        light: L,
        _status: &Cell<Readiness>,
        gate: &'a ResetGate,
    ) -> Result<Self, PortError> {
        if display.dimensions() != Dimensions::new(128, 128) || !gate.ui_allowed() {
            return Err(PortError::InvalidInput);
        }
        let model = Model::default();
        Ok(Self {
            display,
            input,
            light,
            gate,
            settings: None,
            health: None,
            model,
            frame: model,
            measurements: [MeasurementSnapshot {
                state: MeasureState::Unavailable,
                reading: wattch5_parity_ui_demo::Reading::Missing,
            }; 3],
            cursor: Some(0),
            end: 128,
            pending: false,
            settings_retry: false,
            paint_retry: false,
            on: true,
            duty: 0,
            frames: 0,
            next_refresh: 0,
            request: None,
            background: None,
        })
    }
    /// Bind the validated immutable V2 image source; force a complete confirmed frame.
    pub fn bind_background(&mut self, background: &'a dyn Background) {
        self.background = Some(background);
        self.cursor = Some(0);
        self.end = 128;
        self.paint_retry = false;
    }
    /// Bind the same durable property owner used by Bluetooth.
    /// # Errors
    /// Unreadable values or actual backlight failure reject binding.
    pub fn bind(&mut self, settings: &'a dyn SettingsPort) -> Result<(), PortError> {
        let actual = settings.load()?;
        self.model
            .set_appearance(actual)
            .map_err(|_| PortError::InvalidInput)?;
        self.set_light(actual.brightness)?;
        self.settings = Some(settings);
        Ok(())
    }
    /// Bind the same recovered health log used by imports, LATEST and history.
    /// # Errors
    /// Corrupt or unavailable log pages reject binding.
    pub fn bind_health(&mut self, health: &'a dyn DeviceHealth) -> Result<(), PortError> {
        for kind in [
            MeasureKind::Pressure,
            MeasureKind::Oxygen,
            MeasureKind::Glucose,
        ] {
            health_value(kind, health.page(kind)?.reading)?;
        }
        self.health = Some(health);
        Ok(())
    }
    /// Current full V2 navigation model, including editor state.
    #[must_use]
    pub const fn model(&self) -> &Model {
        &self.model
    }
    /// Fully completed frame count; partial rows never count as a frame.
    #[must_use]
    pub const fn frames(&self) -> u32 {
        self.frames
    }
    /// Next unconfirmed display pixel, or none once the frozen scene is complete.
    #[must_use]
    pub const fn next_paint_pixel(&self) -> Option<u16> {
        match self.cursor {
            Some(cursor) => Some(cursor),
            None => None,
        }
    }
    /// Last requested action for diagnostics; this is not an execution receipt.
    #[must_use]
    pub const fn last_request(&self) -> Option<UiRequest> {
        self.request
    }
    // Unbound observations are immutable missing values, not retained services.
    // Only the three bound health readings survive scheduling turns. The full borrowed
    // V2 snapshot is reconstructed inside an input/render/validation sibling.
    #[inline(always)]
    fn snapshot(&self) -> Snapshot<'static> {
        let mut data = Snapshot::unavailable();
        data.measurements[MeasureKind::Pressure as usize] = self.measurements[0];
        data.measurements[MeasureKind::Oxygen as usize] = self.measurements[1];
        data.measurements[MeasureKind::Glucose as usize] = self.measurements[2];
        data
    }
    fn set_light(&mut self, level: u8) -> Result<(), PortError> {
        let duty = if self.on {
            *[10, 20, 40, 70, 100]
                .get(usize::from(level))
                .ok_or(PortError::InvalidInput)?
        } else {
            0
        };
        if duty != self.duty {
            (self.light)(duty)?;
            self.duty = duty;
        }
        Ok(())
    }
    #[inline(never)]
    fn refresh_settings(&mut self) -> Result<(), PortError> {
        let Some(settings) = self.settings else {
            return Ok(());
        };
        if self.pending && !settings.poll()? {
            return Ok(());
        }
        let actual = settings.load()?;
        let mut appearance = self.model.appearance();
        appearance.brightness = actual.brightness;
        appearance.locale = actual.locale;
        appearance.format = actual.format;
        if self.pending {
            settings.finish()?;
            self.pending = false;
        }
        let level = if self.model.screen() == wattch5_parity_ui_demo::Screen::Brightness {
            self.model.draft()
        } else {
            appearance.brightness
        };
        self.set_light(level)?;
        self.model
            .set_appearance(appearance)
            .map_err(|_| PortError::Fault)?;
        Ok(())
    }
    #[inline(never)]
    fn event(&mut self, event: Event) -> Result<(), PortError> {
        if !self.on {
            self.on = true;
            return self.set_light(self.model.appearance().brightness);
        }
        let mut next = self.model;
        let effect = self.handle_event(&mut next, event)?;
        // Leave a live measurement through its real owner, without abandoning an accepted Flash write.
        let mut result = Ok(());
        if self.model.measurement_session() != next.measurement_session()
            && let Some(kind) = self.model.measurement_session()
            && matches!(
                kind,
                MeasureKind::Pressure | MeasureKind::Oxygen | MeasureKind::Glucose
            )
            && let Some(health) = self.health
        {
            result = health.page(kind).and_then(|actual| {
                if matches!(
                    actual.state,
                    MeasureState::Pending | MeasureState::Measuring
                ) {
                    health.request(kind, false)
                } else {
                    Ok(())
                }
            });
        }
        if let Effect::Request(request) = effect {
            self.request = Some(request);
            self.admit_request(&mut next, request, &mut result)?;
        }
        self.model = next;
        result
    }
    #[inline(never)]
    fn handle_event(&self, next: &mut Model, event: Event) -> Result<Effect, PortError> {
        use wattch5_parity_ui_demo::Event as E;
        let event = match event {
            Event::Up => E::Up,
            Event::Down => E::Down,
            Event::Left => E::Left,
            Event::Right => E::Right,
            Event::Tap { x, y } => E::Tap {
                x: u16::from(x),
                y: u16::from(y),
            },
            Event::LongPress => E::LongPress,
            Event::KeyRelease => E::KeyRelease,
            Event::KeyLongPress => E::KeyLongPress,
        };
        let data = self.snapshot();
        if self.background.is_some_and(Background::is_factory) {
            next.handle_factory(event, &data)
        } else {
            next.handle(event, &data)
        }
        .map_err(|_| PortError::InvalidInput)
    }
    // Keep the staged56-byte navigation model out of the owner's admission frame.
    // Request/provider failures may still publish navigation; admission failures
    // return early so optimistic appearance changes never become confirmed.
    #[inline(never)]
    fn admit_request(
        &mut self,
        next: &mut Model,
        request: UiRequest,
        result: &mut Result<(), PortError>,
    ) -> Result<(), PortError> {
        match request {
            UiRequest::BrightnessPreview(level) => self.set_light(level)?,
            UiRequest::AppearanceCommit { field, value } => {
                if matches!(field, Field::Format | Field::Language | Field::Brightness) {
                    if self.pending {
                        return Err(PortError::Unavailable);
                    }
                    self.settings
                        .ok_or(PortError::Unavailable)?
                        .begin(field, value)?;
                    // The model's optimistic edit is staged until actual durable readback.
                    next.set_appearance(self.model.appearance())
                        .map_err(|_| PortError::Fault)?;
                    self.pending = true;
                }
                // Face/theme/font size are deliberately session-local, as in the original V2.
            }
            UiRequest::MeasureStart(kind) | UiRequest::MeasureStop(kind) => {
                *result = self
                    .health
                    .ok_or(PortError::Unavailable)
                    .and_then(|health| {
                        if matches!(
                            kind,
                            MeasureKind::Pressure | MeasureKind::Oxygen | MeasureKind::Glucose
                        ) {
                            let active = matches!(
                                health.page(kind)?.state,
                                MeasureState::Pending | MeasureState::Measuring
                            );
                            health.request(
                                kind,
                                matches!(request, UiRequest::MeasureStart(_)) && !active,
                            )
                        } else {
                            Err(PortError::Unavailable)
                        }
                    });
            }
            UiRequest::DisplayToggle => {
                self.on = false;
                self.set_light(self.model.appearance().brightness)?;
            }
            UiRequest::NoticeDismiss(_) => {}
            _ => *result = Err(PortError::Unavailable),
        }
        Ok(())
    }
    fn model_damage(&self) -> Option<device_core::Rect> {
        use wattch5_parity_ui_demo::Screen as S;
        if self.model == self.frame {
            return None;
        }
        let page = self.model.screen();
        // The product adapter changes only navigation, appearance and a single
        // health cell. Layout-owned bounds avoid staging two180-byte snapshots.
        Some(
            if page != self.frame.screen()
                || self.model.appearance() != self.frame.appearance()
                || matches!(
                    page,
                    S::Dial | S::Theme | S::Face | S::SportList | S::Tools | S::Quick | S::Settings
                )
            {
                device_core::Rect::new(0, 0, 128, 128)
            } else {
                match page {
                    S::Brightness | S::Textsize => device_core::Rect::new(0, 21, 128, 79),
                    S::Language => device_core::Rect::new(0, 30, 128, 68),
                    _ => device_core::Rect::new(0, 21, 128, 107),
                }
            },
        )
    }
    #[inline(never)]
    fn refresh_health(&mut self) -> (bool, Result<(), PortError>) {
        let Some(health) = self.health else {
            return (false, Ok(()));
        };
        let Some(kind) = self.model.measurement_session() else {
            return (false, Ok(()));
        };
        let index = match kind {
            MeasureKind::Pressure => 0,
            MeasureKind::Oxygen => 1,
            MeasureKind::Glucose => 2,
            _ => return (false, Ok(())),
        };
        let (actual, result) = match health.page(kind).and_then(|actual| {
            health_value(kind, actual.reading)?;
            Ok(actual)
        }) {
            Ok(actual) => (actual, Ok(())),
            Err(error) => (
                wattch5_parity_ui_demo::MeasurementSnapshot {
                    state: MeasureState::Error,
                    reading: wattch5_parity_ui_demo::Reading::Missing,
                },
                Err(error),
            ),
        };
        let old = self.measurements[index];
        if old == actual {
            return (false, result);
        }
        // Only one bounded reading changes. Validate and roll back that cell;
        // the immutable missing fields are reconstructed only for validation.
        self.measurements[index] = actual;
        if self.snapshot().validate().is_err() {
            self.measurements[index] = old;
            return (false, Err(PortError::Fault));
        }
        (true, result)
    }
    #[inline(never)]
    fn prepare_frame(&mut self, interrupted: bool) -> Result<(), PortError> {
        // Navigation damage needs no old/new snapshot copy: only the visible
        // health reading can change, with its own conservative body bounds.
        // An unfinished scene may still contain pixels from an older page.
        // Restart the whole display rather than leaving its unpainted header/body.
        let mut area = if interrupted {
            Some(device_core::Rect::new(0, 0, 128, 128))
        } else {
            self.model_damage()
        };
        let (health_changed, result) = self.refresh_health();
        if health_changed {
            let body = device_core::Rect::new(0, 21, 128, 107);
            area = Some(area.map_or(body, |r| {
                let start = r.y.min(body.y);
                let end = (r.y + r.height).max(body.y + body.height);
                device_core::Rect::new(0, start, 128, end - start)
            }));
        }
        if let Some(area) = area {
            self.cursor = Some(area.y * 128);
            self.end = area.y + area.height;
        }
        self.frame = self.model;
        result
    }
    #[inline(always)]
    fn draw_row(&mut self) -> Result<(), PortError> {
        if self.cursor.is_none() {
            return Ok(());
        }
        if self.background.is_some() {
            self.draw_image_row()
        } else {
            #[cfg(not(feature = "factory-ui"))]
            {
                self.draw_procedural_row()
            }
            #[cfg(feature = "factory-ui")]
            {
                Ok(())
            }
        }
    }
    #[inline(never)]
    #[cfg(not(feature = "factory-ui"))]
    fn draw_procedural_row(&mut self) -> Result<(), PortError> {
        let data = self.snapshot();
        let prepared = prepare_scene(&self.frame, &data);
        let scene = prepared.as_ref().map_err(|error| *error)?;
        // One native32-pixel render/write per turn replaces four8-pixel
        // scene traversals. Keep the original Controller polling quantum;
        // avoid extending its scheduling gap into a whole row during setup.
        // Scratch is temporary and released before protocol READ/ACK.
        let cursor = self.cursor.ok_or(PortError::Fault)?;
        let mut scratch = [Color::new(0, 0, 0); 32];
        scene
            .draw(&mut ImageRow {
                y: cursor / 128,
                x: cursor % 128,
                pixels: &mut scratch,
            })
            .map_err(|_| PortError::Fault)?;
        self.display
            .write_rgb565(
                device_core::Rect::new(cursor % 128, cursor / 128, 32, 1),
                &scratch,
            )
            .map_err(|_| {
                self.paint_retry = true;
                PortError::Fault
            })?;
        // A partly transmitted chunk is unconfirmed: retry that frozen
        // chunk after an error. Previously completed chunks remain confirmed.
        self.paint_retry = false;
        if cursor + 32 == self.end * 128 {
            self.cursor = None;
            self.frames = self.frames.saturating_add(1);
        } else {
            self.cursor = Some(cursor + 32);
        }
        Ok(())
    }
    #[inline(never)]
    fn draw_image_row(&mut self) -> Result<(), PortError> {
        let y = self.cursor.ok_or(PortError::Fault)? / 128;
        let mut pixels = [Color::new(0, 0, 0); 128];
        if let Err(error) = self.image_pixels(y, &mut pixels) {
            // No panel write was attempted. A changed page may abandon this
            // unavailable/corrupt source; it must not inherit a display retry.
            self.paint_retry = false;
            return if error == PortError::Unavailable {
                Ok(())
            } else {
                Err(error)
            };
        }
        if self.write_image_pixels(y, &pixels).is_err() {
            self.paint_retry = true;
            return Err(PortError::Fault);
        }
        self.paint_retry = false;
        if y + 1 == self.end {
            self.cursor = None;
            self.frames = self.frames.saturating_add(1);
        } else {
            self.cursor = Some((y + 1) * 128);
        }
        Ok(())
    }
    // The sender's SPI/rectangle temporaries must not overlap scene decoding.
    #[inline(never)]
    fn write_image_pixels(&mut self, y: u16, pixels: &[Color; 128]) -> Result<(), PortError> {
        self.display
            .write_rgb565(device_core::Rect::new(0, y, 128, 1), pixels)
            .map_err(|_| PortError::Fault)
    }
    #[inline(never)]
    fn image_pixels(&self, y: u16, pixels: &mut [Color; 128]) -> Result<(), PortError> {
        let data = self.snapshot();
        self.background
            .ok_or(PortError::Fault)?
            .render(&self.frame, &data, y, pixels)
    }
}
impl<D: Display, I: InputSource, L: FnMut(u8) -> Result<(), PortError>> Work for V2Ui<'_, D, I, L> {
    fn step(&mut self, now: u64) -> Result<(), PortError> {
        if !self.gate.ui_allowed() {
            return Err(PortError::Fault);
        }
        let report = self.input.sample(now.saturating_mul(1000));
        let mut result = report.fault.map_or(Ok(()), Err);
        // Input and accepted commits are serviced every turn. Read-only
        // provider refreshes use a20ms cadence instead of repeatedly reading
        // durable configuration/history while idle or after every display row.
        let refresh_due = now >= self.next_refresh;
        if refresh_due {
            self.next_refresh = now.saturating_add(20);
        }
        if self.pending || self.settings_retry || refresh_due {
            let refreshed = self.refresh_settings();
            self.settings_retry = refreshed.is_err();
            result = result.and(refreshed);
        }
        if let Some(event) = report.event {
            result = result.and(self.event(event));
        }
        if (self.cursor.is_none() && refresh_due) || (!self.paint_retry && self.model != self.frame)
        {
            let interrupted = self.cursor.is_some();
            result = result.and(self.prepare_frame(interrupted));
        }
        result.and(self.draw_row())
    }
}

// Scene validation has its own scratch lifetime; its formatting temporaries
// must not extend the frame that owns the native row during actual drawing.
#[inline(never)]
fn prepare_scene<'s, 'a>(
    model: &Model,
    data: &'s Snapshot<'a>,
) -> Result<wattch5_parity_ui_demo::Scene<'s, 'a, true>, PortError> {
    model.product_scene(data).map_err(|_| PortError::Fault)
}

/// Immutable source boundary; no chip or storage mutation appears in the UI.
pub trait Background: core::fmt::Debug {
    /// Select original bitmap hit rectangles. Existing V2 sources retain their contract.
    fn is_factory(&self) -> bool {
        false
    }
    /// Load one complete native row. Unavailable means retry later without display mutation.
    /// # Errors
    /// A failed row is unconfirmed and must not advance the display cursor.
    fn row(&self, model: &Model, y: u16, out: &mut [Color; 128]) -> Result<(), PortError>;
    /// Compose this skin's data overlays after its immutable row is available.
    /// # Errors
    /// Failed rows remain unconfirmed and must be retried by the scheduler.
    fn render(
        &self,
        model: &Model,
        data: &Snapshot<'_>,
        y: u16,
        out: &mut [Color; 128],
    ) -> Result<(), PortError> {
        let prepared = prepare_scene(model, data)?;
        self.row(prepared.background_model(), y, out)?;
        prepared
            .draw_dynamic_row(&mut ImageRow {
                y,
                x: 0,
                pixels: out,
            })
            .map_err(|_| PortError::Fault)
    }
}
impl<R: moteframe_core::ReadAt<Error = PortError>> Background
    for wattch5_parity_ui_demo::FactoryAssets<R>
{
    fn is_factory(&self) -> bool {
        true
    }
    fn row(&self, model: &Model, y: u16, out: &mut [Color; 128]) -> Result<(), PortError> {
        self.row(model, y, out).map_err(factory_error)
    }
    fn render(
        &self,
        model: &Model,
        data: &Snapshot<'_>,
        y: u16,
        out: &mut [Color; 128],
    ) -> Result<(), PortError> {
        self.render(model, data, y, out).map_err(factory_error)
    }
}
fn factory_error(e: wattch5_parity_ui_demo::ImageError<PortError>) -> PortError {
    match e {
        wattch5_parity_ui_demo::ImageError::Source(e) => e,
        wattch5_parity_ui_demo::ImageError::Busy => PortError::Unavailable,
        wattch5_parity_ui_demo::ImageError::Format => PortError::Fault,
    }
}
impl<R: moteframe_core::ReadAt<Error = PortError> + core::fmt::Debug> Background
    for wattch5_parity_ui_demo::ImageAssets<R>
{
    fn row(&self, model: &Model, y: u16, out: &mut [Color; 128]) -> Result<(), PortError> {
        wattch5_parity_ui_demo::ImageAssets::row(self, model, y, out).map_err(|e| match e {
            wattch5_parity_ui_demo::ImageError::Source(e) => e,
            wattch5_parity_ui_demo::ImageError::Busy => PortError::Unavailable,
            wattch5_parity_ui_demo::ImageError::Format => PortError::Fault,
        })
    }
}
struct ImageRow<'a> {
    y: u16,
    x: u16,
    pixels: &'a mut [Color],
}
impl moteframe_ui::graphics::geometry::Dimensions for ImageRow<'_> {
    fn bounding_box(&self) -> Rectangle {
        Rectangle::new(
            Point::new(i32::from(self.x), i32::from(self.y)),
            Size::new(u32::try_from(self.pixels.len()).unwrap_or(128), 1),
        )
    }
}
impl DrawTarget for ImageRow<'_> {
    type Color = Color;
    type Error = ();
    fn draw_iter<T: IntoIterator<Item = Pixel<Color>>>(&mut self, pixels: T) -> Result<(), ()> {
        for Pixel(p, c) in pixels {
            if p.y != i32::from(self.y) {
                return Err(());
            }
            *self
                .pixels
                .get_mut(usize::try_from(p.x - i32::from(self.x)).map_err(|_| ())?)
                .ok_or(())? = c;
        }
        Ok(())
    }
}
