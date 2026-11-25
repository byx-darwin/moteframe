//! Sport session bookkeeping. No step, energy, distance or heart-rate algorithm lives here.
use crate::MotionError;
/// Frozen sport state dictionary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum SportState {
    /// Active sampling interval.
    Running = 1,
    /// Paused without ending the stable session.
    Paused = 2,
    /// Terminal session; it must never resume.
    Stopped = 3,
}
/// Real resource/recording adapter; production has no simulated implementation.
pub trait SportBackend {
    /// Whether this exact sport/environment combination is actually supported.
    fn supports(&self, sport: u16, environment: u8) -> bool;
    /// Start actual resources with the complete selected configuration; ID alone is insufficient.
    /// # Errors
    /// Unsupported/unavailable/uncertain failures are explicit and never imply a running session.
    fn start(&mut self, session: u64, sport: u16, environment: u8) -> Result<(), MotionError>;
    /// Whether a real detail-storage backend is currently available.
    fn supports_saving(&self) -> bool;
    /// Complete a transition for the stable session identity.
    /// Start must begin with detail saving disabled; stop must also close its detail writer. Error can mean unknown outcome; engine blocks further controls.
    /// # Errors
    /// Unavailable or actual/uncertain backend failure. Never return success for unsupported resources.
    fn transition(&mut self, session: u64, target: SportState) -> Result<(), MotionError>;
    /// Change future detail persistence only, leaving sport state/count/history unchanged.
    /// # Errors
    /// Missing storage or actual backend failure; return only the actual resulting writer state.
    fn set_saving(&mut self, session: u64, enabled: bool) -> Result<bool, MotionError>;
}
/// Device-owned session; fields are private so an App cannot forge a live context.
#[derive(Clone, Copy)]
pub struct SportSession {
    id: u64,
    state: SportState,
    active_ms: u32,
    duration_error: Option<MotionError>,
    sport_type: u16,
    environment: u8,
    active_since: Option<u64>,
    last: u64,
    saving: Option<bool>,
    uncertain: bool,
}
impl core::fmt::Debug for SportSession {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SportSession")
            .field("state", &self.state)
            .field("uncertain", &self.uncertain)
            .finish_non_exhaustive()
    }
}
impl SportSession {
    /// Stable device session identity, without truncation.
    #[must_use]
    pub const fn id(&self) -> u64 {
        self.id
    }
    /// Confirmed sport type, never inferred from the session ID.
    #[must_use]
    pub const fn sport_type(&self) -> u16 {
        self.sport_type
    }
    /// Confirmed environment dictionary0unknown/1indoor/2outdoor.
    #[must_use]
    pub const fn environment(&self) -> u8 {
        self.environment
    }
    /// Current confirmed state.
    /// # Errors
    /// An uncertain backend outcome must be inspected by integration before making state claims.
    pub fn state(&self) -> Result<SportState, MotionError> {
        if self.uncertain {
            Err(MotionError::Backend)
        } else {
            Ok(self.state)
        }
    }
}
/// One active session plus its terminal state retained until the next start.
#[derive(Debug)]
pub struct SportEngine {
    current: Option<SportSession>,
    next: u64,
    last: u64,
}
impl SportEngine {
    /// Start a boot's monotonic nonzero session-ID range. Adapter supplies boot-appropriate identity policy.
    /// # Errors
    /// Rejects zero IDs. This is not a durable multi-boot allocator.
    pub fn new(first_session: u64) -> Result<Self, MotionError> {
        if first_session == 0 {
            return Err(MotionError::Invalid);
        }
        Ok(Self {
            current: None,
            next: first_session,
            last: 0,
        })
    }
    /// Start a supported sport through its real backend.
    /// # Errors
    /// Rejects active/uncertain sessions, unsupported configuration, time reversal, overflow or backend failure.
    pub fn start(
        &mut self,
        sport: u16,
        environment: u8,
        now_ms: u64,
        backend: &mut impl SportBackend,
    ) -> Result<u64, MotionError> {
        if self
            .current
            .is_some_and(|s| s.uncertain || s.state != SportState::Stopped)
        {
            return Err(MotionError::Busy);
        }
        if now_ms < self.last {
            return Err(MotionError::Time);
        }
        if sport > 23 || environment > 2 {
            return Err(MotionError::Invalid);
        }
        if !backend.supports(sport, environment) {
            return Err(MotionError::Unavailable);
        }
        let id = self.next;
        let next = id.checked_add(1).ok_or(MotionError::Capacity)?;
        self.next = next;
        self.last = now_ms;
        self.current = Some(SportSession {
            id,
            state: SportState::Running,
            active_ms: 0,
            duration_error: None,
            sport_type: sport,
            environment,
            active_since: Some(now_ms),
            last: now_ms,
            saving: Some(false),
            uncertain: true,
        });
        backend.start(id, sport, environment)?;
        if let Some(s) = self.current.as_mut() {
            s.uncertain = false;
        }
        Ok(id)
    }
    /// Control1pause/2resume/3stop. Repeating the current target does not repeat backend side effects.
    /// # Errors
    /// Rejects unknown identity, terminal resume, invalid action/time or backend failure.
    pub fn control(
        &mut self,
        id: u64,
        action: u8,
        now_ms: u64,
        backend: &mut impl SportBackend,
    ) -> Result<SportState, MotionError> {
        let target = match action {
            1 => SportState::Paused,
            2 => SportState::Running,
            3 => SportState::Stopped,
            _ => return Err(MotionError::Invalid),
        };
        let s = self.session_mut(id)?;
        if now_ms < s.last && target != SportState::Stopped {
            return Err(MotionError::Time);
        }
        if target == s.state {
            s.last = s.last.max(now_ms);
            self.last = self.last.max(now_ms);
            return Ok(target);
        }
        if s.state == SportState::Stopped {
            return Err(MotionError::Conflict);
        }
        let measured_duration = duration(s, now_ms);
        if target != SportState::Stopped && measured_duration.is_err() {
            return measured_duration.map(|_| target);
        }
        s.uncertain = true;
        backend.transition(id, target)?;
        s.uncertain = false;
        match measured_duration {
            Ok(value) => s.active_ms = value,
            Err(error) => s.duration_error = Some(error),
        }
        s.active_since = if target == SportState::Running {
            Some(now_ms)
        } else {
            None
        };
        s.state = target;
        s.last = s.last.max(now_ms);
        if target == SportState::Stopped {
            s.saving = Some(false);
        }
        self.last = self.last.max(now_ms);
        Ok(target)
    }
    /// Read active monotonic duration excluding pauses; UTC adjustments have no effect.
    /// # Errors
    /// Rejects unknown session, uncertain backend, backwards time or wire-duration overflow.
    pub fn active_ms(&mut self, id: u64, now_ms: u64) -> Result<u32, MotionError> {
        let s = self.session_mut(id)?;
        let value = duration(s, now_ms)?;
        s.last = now_ms;
        self.last = now_ms;
        Ok(value)
    }
    /// Save action0stop/1start/2query; this does not pause or erase sport history.
    /// # Errors
    /// Missing storage, terminal start or real backend failure is explicit.
    pub fn save_control(
        &mut self,
        id: u64,
        action: u8,
        backend: &mut impl SportBackend,
    ) -> Result<bool, MotionError> {
        let s = self.session_mut(id)?;
        if !backend.supports_saving() {
            return Err(MotionError::Unavailable);
        }
        if action == 2 {
            return s.saving.ok_or(MotionError::Backend);
        }
        if action > 2 {
            return Err(MotionError::Invalid);
        }
        let enabled = action == 1;
        if s.state == SportState::Stopped && enabled {
            return Err(MotionError::Conflict);
        }
        if s.saving == Some(enabled) {
            return Ok(enabled);
        }
        s.saving = None;
        let actual = backend.set_saving(id, enabled)?;
        s.saving = Some(actual);
        if actual != enabled {
            return Err(MotionError::Backend);
        }
        Ok(actual)
    }
    /// Borrow the known session, including terminal state for queries.
    /// # Errors
    /// Unknown identity or uncertain transition requires a real inspection result first.
    pub fn session(&self, id: u64) -> Result<&SportSession, MotionError> {
        let s = self
            .current
            .as_ref()
            .filter(|s| s.id == id)
            .ok_or(MotionError::NotFound)?;
        if s.uncertain {
            return Err(MotionError::Backend);
        }
        Ok(s)
    }
    fn session_mut(&mut self, id: u64) -> Result<&mut SportSession, MotionError> {
        let s = self
            .current
            .as_mut()
            .filter(|s| s.id == id)
            .ok_or(MotionError::NotFound)?;
        if s.uncertain {
            return Err(MotionError::Backend);
        }
        Ok(s)
    }
}
fn duration(s: &SportSession, now: u64) -> Result<u32, MotionError> {
    if let Some(error) = s.duration_error {
        return Err(error);
    }
    if now < s.last {
        return Err(MotionError::Time);
    }
    let elapsed = if let Some(start) = s.active_since {
        now.checked_sub(start).ok_or(MotionError::Time)?
    } else {
        0
    };
    s.active_ms
        .checked_add(u32::try_from(elapsed).map_err(|_| MotionError::Capacity)?)
        .ok_or(MotionError::Capacity)
}
/// Connection-scoped u32 aliases for full u64 sessions. Never truncate a session into a context.
#[derive(Debug)]
pub struct ContextAliases<const N: usize> {
    entries: [Option<(u64, u32)>; N],
    next: u32,
    closed: bool,
}
impl<const N: usize> Default for ContextAliases<N> {
    fn default() -> Self {
        Self::new()
    }
}
impl<const N: usize> ContextAliases<N> {
    /// Start a new physical connection's context table.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            entries: [None; N],
            next: 1,
            closed: false,
        }
    }
    /// Assign/reuse an alias only for a confirmed active device-owned sport session.
    /// # Errors
    /// Rejects terminal/closed sessions, exhausted aliases or full static storage.
    pub fn assign(&mut self, session: &SportSession) -> Result<u32, MotionError> {
        if self.closed || session.state()? == SportState::Stopped {
            return Err(MotionError::Conflict);
        }
        if let Some((_, id)) = self.entries.iter().flatten().find(|s| s.0 == session.id) {
            return Ok(*id);
        }
        let slot = self
            .entries
            .iter_mut()
            .find(|e| e.is_none())
            .ok_or(MotionError::Capacity)?;
        let id = self.next;
        self.next = id.checked_add(1).ok_or(MotionError::Capacity)?;
        *slot = Some((session.id, id));
        Ok(id)
    }
    /// Resolve identity. Measurement integration must additionally call `resolve_active` to check lifetime.
    /// # Errors
    /// Unknown or ended alias is not found.
    pub fn resolve(&self, alias: u32) -> Result<u64, MotionError> {
        self.entries
            .iter()
            .flatten()
            .find(|e| e.1 == alias)
            .map(|e| e.0)
            .ok_or(MotionError::NotFound)
    }
    /// Resolve and recheck that the owning session still exists and is nonterminal.
    /// # Errors
    /// Rejects stale alias, replaced/terminal session or uncertain backend.
    pub fn resolve_active(&self, alias: u32, engine: &SportEngine) -> Result<u64, MotionError> {
        let id = self.resolve(alias)?;
        if engine.session(id)?.state()? == SportState::Stopped {
            return Err(MotionError::Conflict);
        }
        Ok(id)
    }
    /// Forget an ended session but permanently consume its old alias for this connection.
    pub fn session_ended(&mut self, session: u64) {
        for e in &mut self.entries {
            if e.is_some_and(|e| e.0 == session) {
                *e = None;
            }
        }
    }
    /// Invalidate the connection; this table cannot reopen/reuse IDs.
    pub fn disconnect(&mut self) {
        self.entries.fill(None);
        self.closed = true;
    }
}
