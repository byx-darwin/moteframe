use ble_controller::{Advertisement, LinkBackend, LinkEvent, LinkFrame, TxAdmission};
use core::cell::Cell;
use device_runtime::PortError;
/// Serializes radio reset completion and permanent UI retirement.
#[derive(Debug, Default)]
pub struct ResetGate {
    done: Cell<bool>,
    armed: Cell<bool>,
    retired: Cell<bool>,
}
impl ResetGate {
    /// No radio reset has completed.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            done: Cell::new(false),
            armed: Cell::new(false),
            retired: Cell::new(false),
        }
    }
    /// Begin reset only before UI ownership; later attempts retire UI before radio access.
    /// # Errors
    /// Any reset after UI arming permanently poisons the composition.
    pub fn begin(&self) -> Result<(), PortError> {
        self.done.set(false);
        if self.armed.get() || self.retired.get() {
            self.retired.set(true);
            return Err(PortError::Fault);
        }
        Ok(())
    }
    /// Record only successful actual backend reset.
    pub fn completed(&self) {
        self.done.set(true);
    }
    /// Actual completed reset observation, not full Host initialization.
    #[must_use]
    pub fn reset_completed(&self) -> bool {
        self.done.get() && !self.retired.get()
    }
    /// Permanently hand physical pin/clock ownership to UI after Host reset.
    /// # Errors
    /// A missing actual reset or poisoned boundary is rejected.
    pub fn arm_ui(&self) -> Result<(), PortError> {
        if !self.reset_completed() {
            return Err(PortError::Fault);
        }
        self.armed.set(true);
        Ok(())
    }
    /// UI must check before each physical slice.
    #[must_use]
    pub fn ui_allowed(&self) -> bool {
        self.armed.get() && !self.retired.get()
    }
}
/// Explicit reset rejection or underlying native backend failure.
#[derive(Debug)]
pub enum ResetError<E> {
    /// Physical ownership was already handed to UI.
    Retired,
    /// Actual radio operation failed.
    Backend(E),
}
/// Forwarding native backend, with actual reset observation and late-reset rejection.
#[derive(Debug)]
pub struct ObservedReset<'a, B> {
    backend: B,
    gate: &'a ResetGate,
}
impl<'a, B> ObservedReset<'a, B> {
    /// Bind the same foreground gate used by the physical UI.
    pub const fn new(backend: B, gate: &'a ResetGate) -> Self {
        Self { backend, gate }
    }
}
impl<B: LinkBackend> LinkBackend for ObservedReset<'_, B> {
    type Error = ResetError<B::Error>;
    fn reset(&mut self) -> Result<(), Self::Error> {
        self.gate.begin().map_err(|_| ResetError::Retired)?;
        self.backend.reset().map_err(ResetError::Backend)?;
        self.gate.completed();
        Ok(())
    }
    fn advertise(&mut self, a: &Advertisement) -> Result<(), Self::Error> {
        self.backend.advertise(a).map_err(ResetError::Backend)
    }
    fn stop(&mut self) -> Result<(), Self::Error> {
        self.backend.stop().map_err(ResetError::Backend)
    }
    fn poll(&mut self) -> Result<Option<LinkEvent>, Self::Error> {
        self.backend.poll().map_err(ResetError::Backend)
    }
    fn transmit(&mut self, t: u32, p: LinkFrame) -> Result<TxAdmission, Self::Error> {
        self.backend.transmit(t, p).map_err(ResetError::Backend)
    }
    fn connection_update(&mut self, p: &[u8]) -> Result<(), Self::Error> {
        self.backend
            .connection_update(p)
            .map_err(ResetError::Backend)
    }
    fn channel_map(&mut self, p: &[u8]) -> Result<(), Self::Error> {
        self.backend.channel_map(p).map_err(ResetError::Backend)
    }
}
