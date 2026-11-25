//! Controlled access to the frozen single-current engine and actual backend.
use crate::{Failure, MotionAccess, MotionBackend, Operation, Resource};
use device_motion::{MotionError, SportBackend, SportEngine, SportSession, SportState};

/// Maximum actual stopped-session proofs retained for one App connection. Handler watch capacity
/// cannot exceed this value. Full retention rejects a new START before physical work.
pub const SPORT_PROOF_CAPACITY: usize = 32;

pub(crate) struct SportRetention {
    proofs: [Option<SportSession>; SPORT_PROOF_CAPACITY],
    current: Option<u64>,
    consumed: bool,
    reservation: Option<(u64, usize)>,
}
impl SportRetention {
    pub(crate) const fn new() -> Self {
        Self {
            proofs: [None; SPORT_PROOF_CAPACITY],
            current: None,
            consumed: false,
            reservation: None,
        }
    }
    pub(crate) fn detach(&mut self) {
        self.proofs.fill(None);
        self.reservation = None;
        self.consumed = false;
    }
    pub(crate) fn admitted(&mut self, id: u64) {
        for proof in &mut self.proofs {
            if proof.is_some_and(|value| value.id() == id) {
                *proof = None;
            }
        }
        if self.current == Some(id) {
            self.consumed = true;
            self.reservation = None;
        }
    }
    fn record(&mut self, session: SportSession) -> Result<(), Failure> {
        if session.state().map_err(map)? != SportState::Stopped
            || self.current == Some(session.id()) && self.consumed
            || self.proofs.iter().flatten().any(|p| p.id() == session.id())
        {
            return Ok(());
        }
        let slot = self
            .reservation
            .filter(|(id, _)| *id == session.id())
            .map(|(_, slot)| slot)
            .or_else(|| {
                self.proofs.iter().enumerate().position(|(slot, value)| {
                    value.is_none()
                        && self
                            .reservation
                            .is_none_or(|(_, reserved)| reserved != slot)
                })
            })
            .ok_or(Failure::Capacity)?;
        self.proofs[slot] = Some(session);
        if self.reservation.is_some_and(|(id, _)| id == session.id()) {
            self.reservation = None;
        }
        Ok(())
    }
    fn prepare_start(&mut self, engine: &SportEngine) -> Result<usize, Failure> {
        if let Some(id) = self.current
            && let Ok(session) = engine.session(id)
        {
            self.record(*session)?;
        }
        self.proofs
            .iter()
            .position(Option::is_none)
            .ok_or(Failure::Capacity)
    }
}

/// The sole service-owned sport lifecycle API for App and trusted device/UI work. It exposes no
/// mutable engine or backend reference. Actual confirmed STOP evidence survives later STARTs;
/// summaries and history are still supplied by the real backend, never synthesized here.
pub struct SportControl<'a, B> {
    engine: &'a mut SportEngine,
    backend: &'a mut B,
    retention: &'a mut SportRetention,
    retain: bool,
}
impl<B> core::fmt::Debug for SportControl<'_, B> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SportControl").finish_non_exhaustive()
    }
}
impl<'a, B: MotionBackend> SportControl<'a, B> {
    pub(crate) fn new(
        engine: &'a mut SportEngine,
        backend: &'a mut B,
        retention: &'a mut SportRetention,
        retain: bool,
    ) -> Self {
        if !retain {
            retention.detach();
        }
        Self {
            engine,
            backend,
            retention,
            retain,
        }
    }
    /// Start actual device-owned work, reserving capacity for its future actual STOP proof while an
    /// App is attached. This trusted entry does not grant or implement App permissions.
    /// # Errors
    /// Invalid/active/uncertain state, full retention, actual backend failure or clock/ID exhaustion.
    pub fn start(&mut self, sport: u16, environment: u8, now: u64) -> Result<u64, Failure> {
        self.start_authorized(sport, environment, now, None)
    }
    pub(crate) fn start_authorized(
        &mut self,
        sport: u16,
        environment: u8,
        now: u64,
        access: Option<&dyn MotionAccess>,
    ) -> Result<u64, Failure> {
        let slot = if self.retain {
            Some(self.retention.prepare_start(self.engine)?)
        } else {
            None
        };
        let mut port = Relay::new(self.backend.sport_backend()?, access, Operation::Start);
        let id = self
            .engine
            .start(sport, environment, now, &mut port)
            .map_err(map)?;
        self.retention.current = Some(id);
        self.retention.consumed = false;
        self.retention.reservation = slot.map(|slot| (id, slot));
        Ok(id)
    }
    /// Apply a real pause/resume/stop. Confirmed STOP is always allowed to run even if App retention
    /// is full; the current engine itself then holds that actual proof and a new START is blocked.
    /// # Errors
    /// Wrong identity/action, uncertain backend, time/duration errors or actual transition failure.
    pub fn control(&mut self, id: u64, action: u8, now: u64) -> Result<SportState, Failure> {
        self.control_authorized(id, action, now, None)
    }
    pub(crate) fn control_authorized(
        &mut self,
        id: u64,
        action: u8,
        now: u64,
        access: Option<&dyn MotionAccess>,
    ) -> Result<SportState, Failure> {
        let mut port = Relay::new(self.backend.sport_backend()?, access, Operation::Control);
        let state = self
            .engine
            .control(id, action, now, &mut port)
            .map_err(map)?;
        self.retention.current = Some(id);
        if state == SportState::Stopped && self.retain {
            // If an externally supplied already-active engine had no reservation, STOP still must
            // run. Its current actual terminal remains queryable until prepare_start can retain it.
            let session = *self.engine.session(id).map_err(map)?;
            let _ = self.retention.record(session);
        }
        Ok(state)
    }
    /// Read a confirmed current session or a retained actual STOP proof. `NotFound` never creates a
    /// stopped state, and no new session state is attributed to an old identity.
    /// # Errors
    /// Unknown identity or uncertain engine transition remains explicit.
    pub fn session(&mut self, id: u64) -> Result<SportSession, Failure> {
        match self.engine.session(id) {
            Ok(session) => {
                let session = *session;
                if self.retention.current != Some(id) {
                    self.retention.current = Some(id);
                    self.retention.consumed = false;
                }
                if self.retain {
                    let _ = self.retention.record(session);
                }
                Ok(session)
            }
            Err(MotionError::NotFound) => self
                .retention
                .proofs
                .iter()
                .flatten()
                .find(|session| session.id() == id)
                .copied()
                .ok_or(Failure::NotFound),
            Err(error) => Err(map(error)),
        }
    }
    pub(crate) fn needs_watch(&mut self, id: u64) -> Result<bool, Failure> {
        if self.retention.current == Some(id) && self.retention.consumed {
            return Ok(false);
        }
        match self.session(id) {
            Ok(_) => Ok(true),
            Err(Failure::NotFound) => Ok(false),
            Err(error) => Err(error),
        }
    }
    /// Read actual current-engine active duration, excluding pauses.
    /// # Errors
    /// Unknown/replaced/uncertain identity, clock reversal or duration overflow.
    pub fn active_ms(&mut self, id: u64, now: u64) -> Result<u32, Failure> {
        self.engine.active_ms(id, now).map_err(map)
    }
    /// Control/query actual future detail persistence; no counter/history erasure.
    /// # Errors
    /// Wrong identity/action, unsupported storage or uncertain actual writer outcome.
    pub fn save_control(&mut self, id: u64, action: u8) -> Result<bool, Failure> {
        self.save_authorized(id, action, None)
    }
    pub(crate) fn save_authorized(
        &mut self,
        id: u64,
        action: u8,
        access: Option<&dyn MotionAccess>,
    ) -> Result<bool, Failure> {
        let mut port = Relay::new(self.backend.sport_backend()?, access, Operation::Save);
        self.engine.save_control(id, action, &mut port).map_err(map)
    }
}
fn map(error: MotionError) -> Failure {
    crate::handler::motion_failure(error)
}
struct Relay<'a> {
    port: &'a mut dyn SportBackend,
    access: Option<&'a dyn MotionAccess>,
    operation: Operation,
}
impl<'a> Relay<'a> {
    fn new(
        port: &'a mut dyn SportBackend,
        access: Option<&'a dyn MotionAccess>,
        operation: Operation,
    ) -> Self {
        Self {
            port,
            access,
            operation,
        }
    }
    fn check(&self, session: u64) -> Result<(), MotionError> {
        let resource = if self.operation == Operation::Start {
            Resource::Module
        } else {
            Resource::Session(session)
        };
        if self
            .access
            .is_none_or(|access| access.allowed(0x22, self.operation, resource))
        {
            Ok(())
        } else {
            Err(MotionError::Backend)
        }
    }
}
impl SportBackend for Relay<'_> {
    fn supports(&self, sport: u16, environment: u8) -> bool {
        self.port.supports(sport, environment)
    }
    fn start(&mut self, session: u64, sport: u16, environment: u8) -> Result<(), MotionError> {
        self.check(session)?;
        self.port.start(session, sport, environment)
    }
    fn supports_saving(&self) -> bool {
        self.port.supports_saving()
    }
    fn transition(&mut self, session: u64, target: SportState) -> Result<(), MotionError> {
        self.check(session)?;
        self.port.transition(session, target)
    }
    fn set_saving(&mut self, session: u64, enabled: bool) -> Result<bool, MotionError> {
        self.check(session)?;
        self.port.set_saving(session, enabled)
    }
}
