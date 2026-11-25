//! Existing V2 model with explicit observations, calendar, actions and display ports.
use super::{AsyncSettings, Settings};
use device_core::{Dimensions, Display, Rect, Rgb565 as Packed};
use device_runtime::{PortError, Work};
use device_services::{PropertyAccess, TimeAccess, UtcMapping};
use device_storage::incremental::AsyncMedium;
use device_storage::persistence::SnapshotStore;
use device_time_store::{TimeStore, TrustedClock};
use device_ui::graphics::{pixelcolor::Rgb565, prelude::*, primitives::Rectangle};
use wattch5_parity_ui_demo::{Effect, Event, Field, Model, Snapshot, UiRequest, Weekday};
/// One normalized event at most per logical slice.
pub trait TouchSource {
    /// Read one actual normalized touch/key event.
    /// # Errors
    /// Input unavailability, invalid input and device failure remain explicit.
    fn next_event(&mut self) -> Result<Option<Event>, PortError>;
}
/// Caller-owned actual observations and their monotonically changing revision.
#[derive(Debug)]
pub struct Observation<'a> {
    /// Nonzero version; change it whenever any observation changes.
    pub generation: u64,
    /// Fully explicit real observations; clock/date are replaced by actual calendar data.
    pub snapshot: Snapshot<'a>,
}
/// No default observations or successful fallback values.
pub trait ObservationSource {
    /// Borrow a full actual snapshot. Missing mandatory observations must return unavailable.
    /// # Errors
    /// Real unavailable/invalid/failed sources are explicit.
    fn capture(&mut self) -> Result<Observation<'_>, PortError>;
}
/// Actual calendar conversion result, in the UI's selected local calendar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CalendarDate {
    /// Local hour, 0..23.
    pub hour: u8,
    /// Local minute, 0..59.
    pub minute: u8,
    /// Actual month, 1..12.
    pub month: u8,
    /// Actual day, 1..31.
    pub day: u8,
    /// Actual weekday.
    pub weekday: Weekday,
}
/// Trusted product calendar/timezone policy, independent of phone session identity.
pub trait Calendar {
    /// Convert an actually valid durable UTC mapping.
    /// # Errors
    /// Unavailable calendar, timezone policy and invalid dates remain explicit.
    fn convert(&mut self, mapping: UtcMapping) -> Result<CalendarDate, PortError>;
}
/// Actual action executor; requests are never implicit acknowledgements.
pub trait Actions {
    /// Execute/acknowledge one supported action against its real owner.
    /// # Errors
    /// Unsupported or failed actions return an explicit error.
    fn perform(&mut self, request: UiRequest) -> Result<(), PortError>;
}
/// Explicitly unbound external actions; never a successful hardware backend.
#[derive(Debug)]
pub struct NoActions;
impl Actions for NoActions {
    fn perform(&mut self, _: UiRequest) -> Result<(), PortError> {
        Err(PortError::Unavailable)
    }
}
/// Explicit policy for V2 fields without a persistent protocol schema.
#[derive(Debug, Clone, Copy)]
pub enum RamFields {
    /// Face, theme and text size live only in this UI instance.
    RamOnly,
    /// Reject commits of those fields.
    Unavailable,
}
/// Local UI scopes, independent of BLE authorization.
#[derive(Clone, Copy)]
pub struct UiAuthority<'a> {
    /// Trusted local settings policy.
    pub properties: &'a dyn PropertyAccess,
    /// Trusted local time read policy.
    pub time: &'a dyn TimeAccess,
}
impl core::fmt::Debug for UiAuthority<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("UiAuthority").finish_non_exhaustive()
    }
}
/// Actual caller-supplied ports; no source/display is created by the product.
#[derive(Debug)]
pub struct UiPorts<T, O, C, D, A> {
    /// Normalized input.
    pub touch: T,
    /// Actual observations.
    pub observations: O,
    /// Trusted calendar policy.
    pub calendar: C,
    /// Actual initialized display.
    pub display: D,
    /// Actual executor, or explicitly unbound `NoActions`.
    pub actions: A,
}
/// One-row existing V2 UI over actual supplied settings, time and observation ports.
/// Asynchronous saves retain confirmed appearance/navigation until durable completion.
pub struct ProductUi<
    'a,
    'c,
    S: SnapshotStore,
    TS: SnapshotStore,
    K: TrustedClock,
    T,
    O,
    C,
    D,
    A,
    const N: usize,
> {
    core: UiCore<'a, Settings<'a, 'c, S, N>, TS, K, T, O, C, D, A>,
}
impl<S: SnapshotStore, TS: SnapshotStore, K: TrustedClock, T, O, C, D, A, const N: usize>
    core::fmt::Debug for ProductUi<'_, '_, S, TS, K, T, O, C, D, A, N>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ProductUi")
            .field("model", &self.core.model)
            .field("pending_row", &self.core.row)
            .finish_non_exhaustive()
    }
}
impl<
    'a,
    'c,
    S: SnapshotStore,
    TS: SnapshotStore,
    K: TrustedClock,
    T: TouchSource,
    O: ObservationSource,
    C: Calendar,
    D: Display,
    A: Actions,
    const N: usize,
> ProductUi<'a, 'c, S, TS, K, T, O, C, D, A, N>
{
    /// Bind actual ports and explicit initial RAM appearance/navigation.
    /// # Errors
    /// Reject incompatible display geometry or unready durable appearance.
    pub fn new(
        settings: Settings<'a, 'c, S, N>,
        time: &'a TimeStore<TS, K>,
        authority: UiAuthority<'a>,
        model: Model,
        ram: RamFields,
        ports: UiPorts<T, O, C, D, A>,
    ) -> Result<Self, PortError> {
        Ok(Self {
            core: UiCore::new(settings, time, authority, model, ram, ports)?,
        })
    }
    /// Current confirmed model; pending or failed saves never replace it early.
    #[must_use]
    pub const fn model(&self) -> &Model {
        &self.core.model
    }
    /// Borrow actual supplied ports for event delivery or recovery.
    pub fn ports_mut(&mut self) -> &mut UiPorts<T, O, C, D, A> {
        &mut self.core.ports
    }
    /// Next pending row; None means every row was confirmed by Display.
    #[must_use]
    pub const fn pending_row(&self) -> Option<u16> {
        self.core.row
    }
}
impl<
    S: SnapshotStore,
    TS: SnapshotStore,
    K: TrustedClock,
    T: TouchSource,
    O: ObservationSource,
    C: Calendar,
    D: Display,
    A: Actions,
    const N: usize,
> Work for ProductUi<'_, '_, S, TS, K, T, O, C, D, A, N>
{
    fn step(&mut self, now: u64) -> Result<(), PortError> {
        self.core.step(now)
    }
}
/// One-row existing V2 UI over actual supplied settings, time and observation ports.
/// Asynchronous saves retain confirmed appearance/navigation until durable completion.
pub struct AsyncProductUi<
    'a,
    'c,
    M: AsyncMedium,
    TS: SnapshotStore,
    K: TrustedClock,
    T,
    O,
    C,
    D,
    A,
    const N: usize,
> {
    core: UiCore<'a, AsyncSettings<'a, 'c, M, N>, TS, K, T, O, C, D, A>,
}
impl<M: AsyncMedium, TS: SnapshotStore, K: TrustedClock, T, O, C, D, A, const N: usize>
    core::fmt::Debug for AsyncProductUi<'_, '_, M, TS, K, T, O, C, D, A, N>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("AsyncProductUi")
            .field("model", &self.core.model)
            .field("pending_row", &self.core.row)
            .finish_non_exhaustive()
    }
}
impl<
    'a,
    'c,
    M: AsyncMedium,
    TS: SnapshotStore,
    K: TrustedClock,
    T: TouchSource,
    O: ObservationSource,
    C: Calendar,
    D: Display,
    A: Actions,
    const N: usize,
> AsyncProductUi<'a, 'c, M, TS, K, T, O, C, D, A, N>
{
    /// Bind actual ports and explicit initial RAM appearance/navigation.
    /// # Errors
    /// Reject incompatible display geometry or unready durable appearance.
    pub fn new(
        settings: AsyncSettings<'a, 'c, M, N>,
        time: &'a TimeStore<TS, K>,
        authority: UiAuthority<'a>,
        model: Model,
        ram: RamFields,
        ports: UiPorts<T, O, C, D, A>,
    ) -> Result<Self, PortError> {
        Ok(Self {
            core: UiCore::new(settings, time, authority, model, ram, ports)?,
        })
    }
    /// Current confirmed model; pending or failed saves never replace it early.
    #[must_use]
    pub const fn model(&self) -> &Model {
        &self.core.model
    }
    /// Borrow actual supplied ports for event delivery or recovery.
    pub fn ports_mut(&mut self) -> &mut UiPorts<T, O, C, D, A> {
        &mut self.core.ports
    }
    /// Next pending row; None means every row was confirmed by Display.
    #[must_use]
    pub const fn pending_row(&self) -> Option<u16> {
        self.core.row
    }
    /// Whether a durable save still owns delayed appearance/navigation publication.
    #[must_use]
    pub const fn save_pending(&self) -> bool {
        self.core.pending.is_some()
    }
}
impl<
    M: AsyncMedium,
    TS: SnapshotStore,
    K: TrustedClock,
    T: TouchSource,
    O: ObservationSource,
    C: Calendar,
    D: Display,
    A: Actions,
    const N: usize,
> Work for AsyncProductUi<'_, '_, M, TS, K, T, O, C, D, A, N>
{
    fn step(&mut self, now: u64) -> Result<(), PortError> {
        self.core.step(now)
    }
}
// Private common event/render path keeps the original synchronous API intact.
trait PendingModel: core::fmt::Debug + Default {
    fn model(&self) -> Option<&Model>;
    fn store(&mut self, model: Model) -> Result<(), PortError>;
    fn clear(&mut self);
}
impl PendingModel for Option<core::convert::Infallible> {
    fn model(&self) -> Option<&Model> {
        match self {
            Some(never) => match *never {},
            None => None,
        }
    }
    fn store(&mut self, _: Model) -> Result<(), PortError> {
        Err(PortError::Fault)
    }
    fn clear(&mut self) {}
}
impl PendingModel for Option<Model> {
    fn model(&self) -> Option<&Model> {
        self.as_ref()
    }
    fn store(&mut self, model: Model) -> Result<(), PortError> {
        *self = Some(model);
        Ok(())
    }
    fn clear(&mut self) {
        *self = None;
    }
}
trait SettingsPort {
    type Pending: PendingModel;
    fn load(
        &self,
        access: &dyn PropertyAccess,
        ram: wattch5_parity_ui_demo::Appearance,
    ) -> Result<wattch5_parity_ui_demo::Appearance, PortError>;
    fn begin(
        &mut self,
        field: Field,
        value: u8,
        access: &dyn PropertyAccess,
    ) -> Result<(), PortError>;
    fn pending(&self) -> bool;
    fn poll(&mut self, access: &dyn PropertyAccess) -> Result<bool, PortError>;
    fn finish(&mut self) -> Result<(), PortError>;
    fn detach(&mut self);
}
impl<S: SnapshotStore, const N: usize> SettingsPort for Settings<'_, '_, S, N> {
    type Pending = Option<core::convert::Infallible>;
    fn load(
        &self,
        a: &dyn PropertyAccess,
        r: wattch5_parity_ui_demo::Appearance,
    ) -> Result<wattch5_parity_ui_demo::Appearance, PortError> {
        self.load(a, r)
    }
    fn begin(&mut self, f: Field, v: u8, a: &dyn PropertyAccess) -> Result<(), PortError> {
        self.commit(f, v, a)
    }
    fn pending(&self) -> bool {
        false
    }
    fn poll(&mut self, _: &dyn PropertyAccess) -> Result<bool, PortError> {
        Ok(true)
    }
    fn finish(&mut self) -> Result<(), PortError> {
        Ok(())
    }
    fn detach(&mut self) {}
}
impl<M: AsyncMedium, const N: usize> SettingsPort for AsyncSettings<'_, '_, M, N> {
    type Pending = Option<Model>;
    fn load(
        &self,
        a: &dyn PropertyAccess,
        r: wattch5_parity_ui_demo::Appearance,
    ) -> Result<wattch5_parity_ui_demo::Appearance, PortError> {
        self.load(a, r)
    }
    fn begin(&mut self, f: Field, v: u8, a: &dyn PropertyAccess) -> Result<(), PortError> {
        self.begin(f, v, a)
    }
    fn pending(&self) -> bool {
        self.pending()
    }
    fn poll(&mut self, a: &dyn PropertyAccess) -> Result<bool, PortError> {
        self.poll(a)
    }
    fn finish(&mut self) -> Result<(), PortError> {
        self.finish()
    }
    fn detach(&mut self) {
        self.detach();
    }
}
#[derive(Debug)]
struct UiCore<'a, P: SettingsPort, TS: SnapshotStore, K: TrustedClock, T, O, C, D, A> {
    settings: P,
    time: &'a TimeStore<TS, K>,
    authority: UiAuthority<'a>,
    model: Model,
    // Synchronous Settings uses Option<Infallible> (zero bytes), while only
    // AsyncSettings reserves a candidate Model for delayed publication.
    pending: P::Pending,
    ram: RamFields,
    ports: UiPorts<T, O, C, D, A>,
    row: Option<u16>,
    followup: bool,
    generation: u64,
    date: Option<CalendarDate>,
}
impl<
    'a,
    P: SettingsPort,
    TS: SnapshotStore,
    K: TrustedClock,
    T: TouchSource,
    O: ObservationSource,
    C: Calendar,
    D: Display,
    A: Actions,
> UiCore<'a, P, TS, K, T, O, C, D, A>
{
    fn new(
        settings: P,
        time: &'a TimeStore<TS, K>,
        authority: UiAuthority<'a>,
        mut model: Model,
        ram: RamFields,
        ports: UiPorts<T, O, C, D, A>,
    ) -> Result<Self, PortError> {
        if ports.display.dimensions() != Dimensions::new(128, 128) {
            return Err(PortError::InvalidInput);
        }
        model
            .set_appearance(settings.load(authority.properties, model.appearance())?)
            .map_err(|_| PortError::InvalidInput)?;
        Ok(Self {
            settings,
            time,
            authority,
            model,
            pending: P::Pending::default(),
            ram,
            ports,
            row: Some(0),
            followup: false,
            generation: 0,
            date: None,
        })
    }
}
impl<
    P: SettingsPort,
    TS: SnapshotStore,
    K: TrustedClock,
    T: TouchSource,
    O: ObservationSource,
    C: Calendar,
    D: Display,
    A: Actions,
> Work for UiCore<'_, P, TS, K, T, O, C, D, A>
{
    fn step(&mut self, _: u64) -> Result<(), PortError> {
        // Delivery cleanup must precede unavailable time/observation early returns.
        let held = self.pending.model().is_some();
        let completed = held
            && poll_save(
                &mut self.settings,
                &mut self.pending,
                self.authority.properties,
            )?;
        if held {
            let _ = self.ports.touch.next_event()?;
        }
        let mapping = self
            .time
            .read(self.authority.time)
            .map_err(|_| PortError::Unavailable)?;
        if !mapping.valid {
            return Err(PortError::Unavailable);
        }
        let date = self.ports.calendar.convert(mapping)?;
        let observation = self.ports.observations.capture()?;
        if observation.generation == 0 || observation.generation < self.generation {
            return Err(PortError::InvalidInput);
        }
        let mut snapshot = observation.snapshot;
        snapshot.hour = date.hour;
        snapshot.minute = date.minute;
        snapshot.month = date.month;
        snapshot.day = date.day;
        snapshot.weekday = date.weekday;
        snapshot.validate().map_err(|_| PortError::InvalidInput)?;
        let mut candidate = if completed {
            *self.pending.model().ok_or(PortError::Fault)?
        } else {
            self.model
        };
        candidate
            .set_appearance(
                self.settings
                    .load(self.authority.properties, candidate.appearance())?,
            )
            .map_err(|_| PortError::InvalidInput)?;
        candidate
            .inject(&snapshot)
            .map_err(|_| PortError::InvalidInput)?;
        let before_event = candidate;
        if !held && let Some(event) = self.ports.touch.next_event()? {
            let effect = candidate
                .handle(event, &snapshot)
                .map_err(|_| PortError::InvalidInput)?;
            if let Effect::Request(request) = effect {
                match request {
                    UiRequest::AppearanceCommit {
                        field: Field::Face | Field::Theme | Field::Textsize,
                        ..
                    } => {
                        if matches!(self.ram, RamFields::Unavailable) {
                            return Err(PortError::Unavailable);
                        }
                    }
                    UiRequest::AppearanceCommit { field, value } => {
                        self.settings
                            .begin(field, value, self.authority.properties)?;
                        if self.settings.pending() {
                            self.pending.store(candidate)?;
                            candidate = before_event;
                        }
                    }
                    request => self.ports.actions.perform(request)?,
                }
            }
        }
        let scene = candidate
            .scene(&snapshot)
            .map_err(|_| PortError::InvalidInput)?;
        if candidate != self.model
            || self.generation != observation.generation
            || self.date != Some(date)
        {
            update_damage(&scene, &mut self.row, &mut self.followup);
        }
        if completed {
            // Calendar/observation/readback callbacks may change live local policy.
            // Recheck SET scope immediately before publishing saved navigation.
            poll_save(
                &mut self.settings,
                &mut self.pending,
                self.authority.properties,
            )?;
            self.settings.finish()?;
            self.pending.clear();
        }
        self.model = candidate;
        self.generation = observation.generation;
        self.date = Some(date);
        render_row(
            &scene,
            &mut self.ports.display,
            &mut self.row,
            &mut self.followup,
        )
    }
}

fn poll_save<P: SettingsPort>(
    settings: &mut P,
    pending: &mut P::Pending,
    access: &dyn PropertyAccess,
) -> Result<bool, PortError> {
    match settings.poll(access) {
        Ok(ready) => Ok(ready),
        Err(error) => {
            settings.detach();
            pending.clear();
            Err(error)
        }
    }
}

fn update_damage(
    scene: &wattch5_parity_ui_demo::Scene<'_, '_>,
    row: &mut Option<u16>,
    followup: &mut bool,
) {
    match *row {
        None => *row = scene.damage_from(None).map(|r| r.y),
        // Continue the current pass so changing sources cannot starve lower rows.
        // Rows already written may contain older data; one coalesced followup
        // pass converges them after changes stop, without retaining a snapshot.
        Some(y) if y > 0 => *followup = true,
        Some(_) => {}
    }
}

fn render_row<D: Display>(
    scene: &wattch5_parity_ui_demo::Scene<'_, '_>,
    display: &mut D,
    row: &mut Option<u16>,
    followup: &mut bool,
) -> Result<(), PortError> {
    if let Some(y) = *row {
        let mut target = Row {
            y,
            pixels: [Packed(0); 128],
        };
        scene
            .draw(&mut target)
            .map_err(|_| PortError::InvalidInput)?;
        display
            .write(Rect::new(0, y, 128, 1), &target.pixels)
            .map_err(|_| PortError::Fault)?;
        if y == 127 {
            *row = followup.then_some(0);
            *followup = false;
        } else {
            *row = Some(y + 1);
        }
    }
    Ok(())
}

#[derive(Debug)]
struct Row {
    y: u16,
    pixels: [Packed; 128],
}
impl device_ui::graphics::geometry::Dimensions for Row {
    fn bounding_box(&self) -> Rectangle {
        Rectangle::new(Point::new(0, i32::from(self.y)), Size::new(128, 1))
    }
}
impl DrawTarget for Row {
    type Color = Rgb565;
    type Error = core::convert::Infallible;
    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Rgb565>>,
    {
        for Pixel(p, color) in pixels {
            if p.y == i32::from(self.y)
                && let Ok(x) = usize::try_from(p.x)
                && let Some(pixel) = self.pixels.get_mut(x)
            {
                *pixel = Packed(color.into_storage());
            }
        }
        Ok(())
    }
}
