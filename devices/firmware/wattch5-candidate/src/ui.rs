//! Row-sliced native settings/health view over independently owned durable services.
use core::cell::{Cell, RefCell};
use core::fmt::Write;
use device_core::{Dimensions, Display, Rect, Rgb565};
#[cfg(feature = "native-health")]
use device_history_store::HistoryClock;
use device_property_store::{AsyncPropertyStore, CodecSchemas};
use device_runtime::{PortError, Work};
use device_services::PropertyAccess;
use device_storage::incremental::AsyncMedium;
use wattch5_bringup::{Event, InputSource, LinkStage, Readiness, ResetGate};
#[cfg(feature = "native-health")]
use wattch5_parity_ui_demo::UiRequest;
use wattch5_parity_ui_demo::{
    Appearance, Field, Label, Locale, MeasureKind, MeasureState, MeasurementSnapshot, Reading,
    TimeFormat, glyph,
};
#[cfg(feature = "native-health")]
use wattch5_product::runtime::Actions;
#[cfg(feature = "native-health")]
use wattch5_product::runtime::legacy_health::{HealthLog, LegacyActions, LegacyInputSource};
use wattch5_product::runtime::{AsyncSettings, Dictionary};

/// Native pages; the three health pages read the same log as BLE import and LATEST.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiPage {
    /// Confirmed appearance settings and health navigation.
    Settings,
    /// Blood pressure.
    Pressure,
    /// Blood oxygen.
    Oxygen,
    /// Blood glucose.
    Glucose,
}
impl UiPage {
    fn kind(self) -> Option<MeasureKind> {
        match self {
            Self::Settings => None,
            Self::Pressure => Some(MeasureKind::Pressure),
            Self::Oxygen => Some(MeasureKind::Oxygen),
            Self::Glucose => Some(MeasureKind::Glucose),
        }
    }
    fn next(self, forward: bool) -> Self {
        match (self, forward) {
            (Self::Settings, true) | (Self::Oxygen, false) => Self::Pressure,
            (Self::Pressure, true) | (Self::Glucose, false) => Self::Oxygen,
            (Self::Oxygen, true) | (Self::Settings, false) => Self::Glucose,
            _ => Self::Settings,
        }
    }
}

/// Read-only committed/pending pages and bounded action admission; no Flash progress here.
pub trait DeviceHealth {
    /// Read one actual source-free page.
    /// # Errors
    /// Unsupported or unavailable observations fail explicitly.
    fn page(&self, kind: MeasureKind) -> Result<MeasurementSnapshot, PortError>;
    /// Admit a local start/stop request; accepted persistence remains background-owned.
    /// # Errors
    /// Missing real inputs, busy storage or invalid actions fail explicitly.
    fn request(&self, kind: MeasureKind, start: bool) -> Result<(), PortError>;
}

/// Native UI adapter sharing exactly the log used by App imports, LATEST and history.
#[cfg(feature = "native-health")]
pub struct LocalHealth<'a, 's, I, M: AsyncMedium, L: AsyncMedium, C: HistoryClock> {
    inputs: &'a I,
    log: &'a HealthLog<'s, M, L, C>,
}
#[cfg(feature = "native-health")]
impl<I, M: AsyncMedium, L: AsyncMedium, C: HistoryClock> core::fmt::Debug
    for LocalHealth<'_, '_, I, M, L, C>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LocalHealth").finish_non_exhaustive()
    }
}
#[cfg(feature = "native-health")]
impl<'a, 's, I: LegacyInputSource, M: AsyncMedium, L: AsyncMedium, C: HistoryClock>
    LocalHealth<'a, 's, I, M, L, C>
{
    /// Bind actual product inputs and the boot-owned durable log without inventing defaults.
    #[must_use]
    pub const fn new(inputs: &'a I, log: &'a HealthLog<'s, M, L, C>) -> Self {
        Self { inputs, log }
    }
}
#[cfg(feature = "native-health")]
impl<I: LegacyInputSource, M: AsyncMedium, L: AsyncMedium, C: HistoryClock> DeviceHealth
    for LocalHealth<'_, '_, I, M, L, C>
{
    fn page(&self, kind: MeasureKind) -> Result<MeasurementSnapshot, PortError> {
        self.log.page(kind).map_err(|_| PortError::Unavailable)
    }
    fn request(&self, kind: MeasureKind, start: bool) -> Result<(), PortError> {
        LegacyActions::new(self.inputs, self.log).perform(if start {
            UiRequest::MeasureStart(kind)
        } else {
            UiRequest::MeasureStop(kind)
        })
    }
}

/// Actual settings plus explicit service readiness. No health/time snapshot is invented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UiSnapshot {
    /// Last confirmed durable appearance.
    pub appearance: Appearance,
    /// Current local navigation page.
    pub page: UiPage,
    /// Current actual page reading, without source metadata.
    pub measurement: MeasurementSnapshot,
    /// Last page action/observation failed; committed readings are still retained.
    pub health_error: bool,
    health_bound: bool,
    ready: bool,
    pending: bool,
    fault: bool,
    link: LinkStage,
}
impl UiSnapshot {
    /// Fallback state before storage recovery; Chinese is the existing UI default.
    pub fn new(appearance: Appearance) -> Self {
        Self {
            appearance,
            page: UiPage::Settings,
            measurement: MeasurementSnapshot::default(),
            health_error: false,
            health_bound: false,
            ready: false,
            pending: false,
            fault: false,
            link: LinkStage::Initializing,
        }
    }
}
/// Fixed existing copy and literal status; no timestamps or measurements are supplied.
pub fn line(s: UiSnapshot, n: usize) -> (&'static str, &'static str) {
    let locale = s.appearance.locale;
    let absent = Label::Unavailable.text(locale);
    if s.health_bound {
        let target = match n {
            4 => Some(Label::Pressure),
            5 => Some(Label::Oxygen),
            6 => Some(Label::Glucose),
            _ => None,
        };
        if let Some(label) = target {
            return (label.text(locale), ">");
        }
    }
    match n {
        0 => ("W5 CANDIDATE", ""),
        1 => (
            Label::Format.text(locale),
            if !s.ready {
                Label::Pending.text(locale)
            } else {
                match s.appearance.format {
                    TimeFormat::Hour24 => "24H",
                    TimeFormat::Hour12 => "12H",
                }
            },
        ),
        2 => (
            Label::Language.text(locale),
            if !s.ready {
                Label::Pending.text(locale)
            } else {
                match locale {
                    Locale::Zh => "ZH",
                    Locale::En => "EN",
                }
            },
        ),
        3 => (
            Label::Brightness.text(locale),
            if !s.ready {
                Label::Pending.text(locale)
            } else {
                match s.appearance.brightness {
                    0 => "100",
                    1 => "200",
                    2 => "400",
                    3 => "700",
                    _ => "1000",
                }
            },
        ),
        4 => ("UTC", absent),
        5 => (Label::Heart.text(locale), absent),
        6 => (Label::Temperature.text(locale), absent),
        _ => (
            if s.fault {
                "FAULT"
            } else if s.pending {
                "SAVING"
            } else {
                "BLE"
            },
            match s.link {
                LinkStage::Initializing => "INIT",
                LinkStage::AdvertisingCommand => "ADV WAIT",
                LinkStage::Advertising => "ADV",
                LinkStage::Connected => "LINK",
                LinkStage::Disconnected => "IDLE",
                LinkStage::Fault => "FAULT",
            },
        ),
    }
}

/// Bounded numeric text for the existing glyph renderer; never includes result origin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HealthValue {
    bytes: [u8; 16],
    len: usize,
}
impl HealthValue {
    /// Formatted ASCII value.
    #[must_use]
    pub fn as_str(&self) -> &str {
        // Only ASCII literals and integer formatting reach this bounded buffer.
        core::str::from_utf8(&self.bytes[..self.len]).unwrap_or("")
    }
}
impl core::fmt::Write for HealthValue {
    fn write_str(&mut self, text: &str) -> core::fmt::Result {
        let end = self.len.checked_add(text.len()).ok_or(core::fmt::Error)?;
        let target = self.bytes.get_mut(self.len..end).ok_or(core::fmt::Error)?;
        target.copy_from_slice(text.as_bytes());
        self.len = end;
        Ok(())
    }
}
/// Format an already decoded typed reading, preserving Oxygen's whole-number contract.
/// # Errors
/// Wrong reading shapes, unsupported kinds and pressure beyond three digits fail.
pub fn health_value(kind: MeasureKind, reading: Reading) -> Result<HealthValue, PortError> {
    let mut text = HealthValue {
        bytes: [0; 16],
        len: 0,
    };
    let formatted = match (kind, reading) {
        (MeasureKind::Pressure | MeasureKind::Oxygen | MeasureKind::Glucose, Reading::Missing) => {
            text.write_str("--")
        }
        (MeasureKind::Pressure, Reading::Pressure { upper, lower })
            if upper <= 999 && lower <= 999 =>
        {
            write!(text, "{upper}/{lower}")
        }
        (MeasureKind::Oxygen, Reading::Number(value)) => write!(text, "{value}"),
        (MeasureKind::Glucose, Reading::Tenth(value)) => {
            write!(text, "{}.{}", value / 10, value % 10)
        }
        _ => return Err(PortError::InvalidInput),
    };
    formatted.map_err(|_| PortError::InvalidInput)?;
    Ok(text)
}
fn measurement_label(state: MeasureState) -> Label {
    match state {
        MeasureState::Idle => Label::NoData,
        MeasureState::Pending => Label::Pending,
        MeasureState::Measuring => Label::Measuring,
        MeasureState::Result => Label::Result,
        MeasureState::Error => Label::Error,
        MeasureState::Unavailable => Label::Unavailable,
    }
}
fn measuring(state: MeasureState) -> bool {
    matches!(state, MeasureState::Pending | MeasureState::Measuring)
}
/// Local controller over the same boot-owned property image used by BLE.
pub trait SettingsPort {
    /// Read confirmed values without progressing Flash.
    /// # Errors
    /// Unavailable/corrupt/denied values must fail.
    fn load(&self) -> Result<Appearance, PortError>;
    /// Admit one typed settings change, without claiming durable completion.
    /// # Errors
    /// Busy, validation or access failure is explicit.
    fn begin(&self, field: Field, value: u8) -> Result<(), PortError>;
    /// Inspect actual operation completion without driving the medium.
    /// # Errors
    /// Uncertain/failed commit is explicit.
    fn poll(&self) -> Result<bool, PortError>;
    /// Release successful local delivery after confirmed readback.
    /// # Errors
    /// Active work cannot finish early.
    fn finish(&self) -> Result<(), PortError>;
}
/// Short checked borrows; the independently scheduled owner performs accepted work.
pub struct LocalSettings<'a, 'c, M: AsyncMedium, const N: usize> {
    controller: RefCell<AsyncSettings<'a, 'c, M, N>>,
    access: &'a dyn PropertyAccess,
}
impl<M: AsyncMedium, const N: usize> core::fmt::Debug for LocalSettings<'_, '_, M, N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LocalSettings").finish_non_exhaustive()
    }
}
impl<'a, 'c, M: AsyncMedium, const N: usize> LocalSettings<'a, 'c, M, N> {
    /// Bind ready validated settings and independently supplied local access.
    /// # Errors
    /// Dictionary/schema/storage mismatch rejects binding.
    pub fn new(
        owner: &'a AsyncPropertyStore<'c, M, CodecSchemas<'c>, N>,
        dictionary: Dictionary<'c>,
        access: &'a dyn PropertyAccess,
    ) -> Result<Self, PortError> {
        Ok(Self {
            controller: RefCell::new(AsyncSettings::new(owner, dictionary)?),
            access,
        })
    }
}
impl<M: AsyncMedium, const N: usize> SettingsPort for LocalSettings<'_, '_, M, N> {
    fn load(&self) -> Result<Appearance, PortError> {
        self.controller
            .try_borrow()
            .map_err(|_| PortError::Fault)?
            .load(self.access, Appearance::default())
    }
    fn begin(&self, field: Field, value: u8) -> Result<(), PortError> {
        self.controller
            .try_borrow_mut()
            .map_err(|_| PortError::Fault)?
            .begin(field, value, self.access)
    }
    fn poll(&self) -> Result<bool, PortError> {
        self.controller
            .try_borrow_mut()
            .map_err(|_| PortError::Fault)?
            .poll(self.access)
    }
    fn finish(&self) -> Result<(), PortError> {
        self.controller
            .try_borrow_mut()
            .map_err(|_| PortError::Fault)?
            .finish()
    }
}
/// Physical settings/health view, one128-pixel row and one input slice per pass.
/// Providers remain independent of UI navigation and never run Flash work here.
pub struct CandidateUi<'a, D, I, L> {
    display: D,
    input: I,
    light: L,
    status: &'a Cell<Readiness>,
    gate: &'a ResetGate,
    settings: Option<&'a dyn SettingsPort>,
    health: Option<&'a dyn DeviceHealth>,
    live: UiSnapshot,
    snapshot: UiSnapshot,
    row: Option<u16>,
    again: bool,
    scratch: [Rgb565; 128],
}
impl<D, I, L> core::fmt::Debug for CandidateUi<'_, D, I, L> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CandidateUi")
            .field("row", &self.row)
            .field("snapshot", &self.snapshot)
            .finish_non_exhaustive()
    }
}
impl<'a, D: Display, I: InputSource, L: FnMut(u8) -> Result<(), PortError>>
    CandidateUi<'a, D, I, L>
{
    /// Bind the initialized physical panel/input only after the observed actual Host reset.
    /// # Errors
    /// Wrong dimensions or unavailable reset ownership fail.
    pub fn new(
        display: D,
        input: I,
        light: L,
        status: &'a Cell<Readiness>,
        gate: &'a ResetGate,
    ) -> Result<Self, PortError> {
        if display.dimensions() != Dimensions::new(128, 128) || !gate.ui_allowed() {
            return Err(PortError::InvalidInput);
        }
        let live = UiSnapshot::new(Appearance::default());
        Ok(Self {
            display,
            input,
            light,
            status,
            gate,
            settings: None,
            health: None,
            live,
            snapshot: live,
            row: Some(0),
            again: false,
            scratch: [Rgb565(0); 128],
        })
    }
    /// Publish ready settings only after real durable image recovery.
    /// # Errors
    /// Actual confirmed readback or backlight failure rejects binding.
    pub fn bind(&mut self, settings: &'a dyn SettingsPort) -> Result<(), PortError> {
        let actual = settings.load()?;
        self.apply_light(actual)?;
        self.live.appearance = actual;
        self.live.ready = true;
        self.settings = Some(settings);
        self.invalidate();
        Ok(())
    }
    /// Bind the same recovered health log used by BLE and history, without admitting work.
    /// # Errors
    /// Any unreadable health page rejects binding.
    pub fn bind_health(&mut self, health: &'a dyn DeviceHealth) -> Result<(), PortError> {
        for kind in [
            MeasureKind::Pressure,
            MeasureKind::Oxygen,
            MeasureKind::Glucose,
        ] {
            health_value(kind, health.page(kind)?.reading)?;
        }
        self.health = Some(health);
        self.live.health_bound = true;
        self.invalidate();
        Ok(())
    }
    /// Current requested view over latest observations; an in-flight row frame remains stable.
    #[must_use]
    pub const fn view(&self) -> UiSnapshot {
        self.live
    }
    fn apply_light(&mut self, appearance: Appearance) -> Result<(), PortError> {
        let duty = *([10, 20, 40, 70, 100]
            .get(usize::from(appearance.brightness))
            .ok_or(PortError::InvalidInput)?);
        (self.light)(duty)
    }
    fn invalidate(&mut self) {
        if self.row.is_some() {
            self.again = true
        } else {
            self.snapshot = self.live;
            self.row = Some(0)
        }
    }
    fn input(&mut self, event: Event) -> Result<(), PortError> {
        if self.health.is_some() {
            let page = match event {
                Event::Left | Event::Up => Some(self.live.page.next(true)),
                Event::Right | Event::Down => Some(self.live.page.next(false)),
                Event::KeyRelease => Some(UiPage::Settings),
                Event::Tap { y: 64..=79, .. } if self.live.page == UiPage::Settings => {
                    Some(UiPage::Pressure)
                }
                Event::Tap { y: 80..=95, .. } if self.live.page == UiPage::Settings => {
                    Some(UiPage::Oxygen)
                }
                Event::Tap { y: 96..=111, .. } if self.live.page == UiPage::Settings => {
                    Some(UiPage::Glucose)
                }
                Event::Tap { y: 112..=127, .. } if self.live.page != UiPage::Settings => {
                    Some(UiPage::Settings)
                }
                _ => None,
            };
            if let Some(page) = page {
                let leaving = if page != self.live.page {
                    if let Some(kind) = self.live.page.kind() {
                        let health = self.health.ok_or(PortError::Unavailable)?;
                        health.page(kind).and_then(|actual| {
                            if measuring(actual.state) {
                                health.request(kind, false)
                            } else {
                                Ok(())
                            }
                        })
                    } else {
                        Ok(())
                    }
                } else {
                    Ok(())
                };
                self.live.page = page;
                self.live.health_error = leaving.is_err();
                return leaving;
            }
            if let Some(kind) = self.live.page.kind() {
                if matches!(event, Event::Tap { y: 96..=111, .. }) {
                    // Consult the actual phase at action admission, not a prior rendered row.
                    let health = self.health.ok_or(PortError::Unavailable)?;
                    let request = health
                        .page(kind)
                        .and_then(|actual| health.request(kind, !measuring(actual.state)));
                    self.live.health_error = request.is_err();
                    return request;
                }
                return Ok(());
            }
        }
        if self.live.pending {
            return Ok(());
        }
        let Some(settings) = self.settings else {
            return Err(PortError::Unavailable);
        };
        let field = match event {
            Event::Tap { y: 16..=31, .. } => Field::Format,
            Event::Tap { y: 32..=47, .. } => Field::Language,
            Event::Tap { y: 48..=63, .. } => Field::Brightness,
            _ => return Ok(()),
        };
        let actual = self.live.appearance;
        let value = match field {
            Field::Format => u8::from(actual.format == TimeFormat::Hour24),
            Field::Language => u8::from(actual.locale == Locale::Zh),
            Field::Brightness => (actual.brightness + 1) % 5,
            _ => return Err(PortError::InvalidInput),
        };
        settings.begin(field, value)?;
        self.live.pending = true;
        Ok(())
    }
}
impl<D: Display, I: InputSource, L: FnMut(u8) -> Result<(), PortError>> Work
    for CandidateUi<'_, D, I, L>
{
    fn step(&mut self, now: u64) -> Result<(), PortError> {
        if !self.gate.ui_allowed() {
            return Err(PortError::Fault);
        }
        let report = self.input.sample(now.saturating_mul(1000));
        let mut result = report.fault.map_or(Ok(()), Err);
        if let Some(event) = report.event {
            result = result.and(self.input(event));
        }
        self.live.link = self.status.get().link();
        if let Some(settings) = self.settings {
            let refreshed = (|| {
                if !self.live.pending || settings.poll()? {
                    let actual = settings.load()?;
                    if actual != self.live.appearance {
                        self.apply_light(actual)?;
                        self.live.appearance = actual;
                    }
                    if self.live.pending {
                        settings.finish()?;
                        self.live.pending = false;
                    }
                }
                Ok(())
            })();
            if refreshed.is_err() {
                self.live.fault = true;
                self.live.pending = false;
            }
            result = result.and(refreshed);
        }
        if let Some(kind) = self.live.page.kind()
            && let Some(health) = self.health
        {
            let observed = health.page(kind).and_then(|actual| {
                health_value(kind, actual.reading)?;
                Ok(actual)
            });
            match observed {
                Ok(actual) => {
                    if actual != self.live.measurement {
                        self.live.measurement = actual;
                        self.live.health_error = false;
                    }
                }
                Err(error) => {
                    self.live.health_error = true;
                    result = result.and(Err(error));
                }
            }
        }
        if self.live != self.snapshot {
            self.invalidate();
        }
        if let Some(row) = self.row {
            self.scratch.fill(Rgb565(0));
            let gy = usize::from(row % 16);
            if gy < 12 {
                let n = usize::from(row / 16);
                if let Some(kind) = self.snapshot.page.kind() {
                    draw_health(&mut self.scratch, self.snapshot, kind, n, gy)?;
                } else {
                    let (label, value) = line(self.snapshot, n);
                    draw_text(&mut self.scratch, label, 1, gy)?;
                    draw_text(&mut self.scratch, value, 62, gy)?;
                }
            }
            self.display
                .write(Rect::new(0, row, 128, 1), &self.scratch)
                .map_err(|_| PortError::Fault)?;
            if row == 127 {
                if self.again {
                    self.snapshot = self.live;
                    self.row = Some(0);
                    self.again = false
                } else {
                    self.row = None
                }
            } else {
                self.row = Some(row + 1)
            }
        }
        result
    }
}
fn draw_health(
    out: &mut [Rgb565; 128],
    snapshot: UiSnapshot,
    kind: MeasureKind,
    line: usize,
    row: usize,
) -> Result<(), PortError> {
    let locale = snapshot.appearance.locale;
    let value = health_value(kind, snapshot.measurement.reading)?;
    let text = match line {
        0 => match kind {
            MeasureKind::Pressure => Label::Pressure,
            MeasureKind::Oxygen => Label::Oxygen,
            _ => Label::Glucose,
        }
        .text(locale),
        2 => value.as_str(),
        3 => match kind {
            MeasureKind::Pressure => "mmHg",
            MeasureKind::Oxygen => "%",
            _ => "mmol/L",
        },
        4 => measurement_label(snapshot.measurement.state).text(locale),
        5 if snapshot.health_error => Label::Unavailable.text(locale),
        6 => if measuring(snapshot.measurement.state) {
            Label::Stop
        } else {
            Label::Start
        }
        .text(locale),
        7 => Label::Back.text(locale),
        _ => "",
    };
    draw_text(out, text, 1, row)
}
fn draw_text(
    out: &mut [Rgb565; 128],
    text: &str,
    mut x: usize,
    row: usize,
) -> Result<(), PortError> {
    for ch in text.chars() {
        let g = glyph(ch).map_err(|_| PortError::Unavailable)?;
        let width = usize::from(g.width);
        if x + width > 128 {
            return Err(PortError::InvalidInput);
        }
        for bit in 0..width {
            if g.rows[row] & (1 << (width - 1 - bit)) != 0 {
                out[x + bit] = Rgb565(0xffff)
            }
        }
        x += width + 1;
    }
    Ok(())
}
