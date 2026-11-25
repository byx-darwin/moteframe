use crate::{
    AccessPort, BackendTicket, Error, Operation, Scope,
    wire::{Reader, Writer, ensure},
};
/// Candidate schema2 GIMBAL request, exactly 11B.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct GimbalRequest {
    /// Nonzero connection-scoped increasing action identity supplied by the initiator.
    pub action_id: u32,
    /// 0 stop, 1 start.
    pub action: u8,
    /// 0 counterclockwise, 1 clockwise.
    pub direction: u8,
    /// 0 duration milliseconds, 1 angle centidegrees.
    pub mode: u8,
    /// Actual duration/angle inside backend-supplied limits. Stop requires canonical zero parameters.
    pub value: u32,
}
impl core::fmt::Debug for GimbalRequest {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("GimbalRequest")
            .field("encoded_length", &11)
            .finish_non_exhaustive()
    }
}
/// Actual versioned gimbal backend parameter limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GimbalCapability {
    /// Nonzero criterion/backend manifest revision.
    pub revision: u16,
    /// Supported mode bits0 duration,1 angle.
    pub modes: u8,
    /// Supported direction bits0 counterclockwise,1 clockwise.
    pub directions: u8,
    /// Inclusive minimum duration milliseconds.
    pub min_duration_ms: u32,
    /// Inclusive maximum duration milliseconds.
    pub max_duration_ms: u32,
    /// Inclusive minimum angle centidegrees.
    pub min_angle_centi_deg: u32,
    /// Inclusive maximum angle centidegrees.
    pub max_angle_centi_deg: u32,
}
/// Candidate E82 terminal result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum GimbalResult {
    /// Real backend confirmed completion.
    Completed = 1,
    /// Actual backend failure.
    Failed = 2,
    /// Actual backend confirmed cancellation.
    Cancelled = 3,
}
/// Real backend observation, never inferred from request/response delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GimbalObservation {
    /// Actual operation remains in progress.
    Running,
    /// Actual immutable terminal outcome and backend-specific reason.
    Terminal {
        /// Confirmed terminal result.
        result: GimbalResult,
        /// Backend-specific diagnostic reason.
        reason: u16,
    },
}
/// Gimbal action lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GimbalState {
    /// Waiting for successful response admission.
    Prepared,
    /// Real backend ticket is outstanding.
    Running,
    /// Confirmed terminal observation.
    Terminal {
        /// Actual terminal result.
        result: GimbalResult,
        /// Backend reason.
        reason: u16,
    },
    /// Scope ended before actual outcome was confirmed.
    Unknown,
}
/// Selected real gimbal controller.
pub trait GimbalPort: core::fmt::Debug {
    /// Actual supported units and ranges.
    /// # Errors
    /// Returns Unavailable when no real controller is selected.
    fn capability(&self) -> Result<GimbalCapability, Error>;
    /// Submit one admitted operation and return its actual backend ticket.
    /// # Errors
    /// Returns an actual controller/backend error.
    fn submit(&mut self, scope: Scope, request: GimbalRequest) -> Result<BackendTicket, Error>;
    /// Observe actual ticket completion.
    /// # Errors
    /// Returns actual observation/backend failure.
    fn poll(&mut self, scope: Scope, ticket: BackendTicket) -> Result<GimbalObservation, Error>;
    /// Request cancellation during source-scope cleanup; not confirmation of physical stop.
    /// # Errors
    /// Returns an actual cancellation error.
    fn cancel(&mut self, scope: Scope, ticket: BackendTicket) -> Result<(), Error>;
}
#[derive(Debug, Clone, Copy)]
struct Entry {
    request: GimbalRequest,
    ticket: Option<BackendTicket>,
    state: GimbalState,
}
/// Bounded same-connection gimbal CAS ledger.
pub struct GimbalService<const N: usize> {
    scope: Scope,
    highwater: u32,
    entries: [Option<Entry>; N],
}
impl<const N: usize> core::fmt::Debug for GimbalService<N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("GimbalService")
            .field("length", &self.entries.iter().flatten().count())
            .finish_non_exhaustive()
    }
}
impl<const N: usize> GimbalService<N> {
    /// Empty authenticated action ledger.
    #[must_use]
    pub const fn new(scope: Scope) -> Self {
        Self {
            scope,
            highwater: 0,
            entries: [None; N],
        }
    }
    /// Reserve a validated identity and parameters without performing a physical action.
    /// # Errors
    /// Returns denial, invalid range, missing backend, conflicting identity or exhausted capacity.
    pub fn prepare(
        &mut self,
        request: GimbalRequest,
        access: &impl AccessPort,
        port: &impl GimbalPort,
    ) -> Result<GimbalState, Error> {
        access.authorize(self.scope, Operation::Submit)?;
        request.syntax()?;
        if let Some(entry) = self
            .entries
            .iter()
            .flatten()
            .find(|entry| entry.request.action_id == request.action_id)
        {
            if entry.request != request {
                return Err(Error::Conflict);
            }
            return Ok(entry.state);
        }
        request.validate(port.capability()?)?;
        if request.action_id <= self.highwater {
            return Err(Error::Conflict);
        }
        let slot = self
            .entries
            .iter_mut()
            .find(|entry| entry.is_none())
            .ok_or(Error::Capacity)?;
        *slot = Some(Entry {
            request,
            ticket: None,
            state: GimbalState::Prepared,
        });
        self.highwater = request.action_id;
        Ok(GimbalState::Prepared)
    }
    /// Submit the real operation only after the complete success response was admitted.
    /// # Errors
    /// Returns denial, missing/range-limited backend, unknown ID or actual submission failure.
    pub fn response_admitted(
        &mut self,
        id: u32,
        access: &impl AccessPort,
        port: &mut impl GimbalPort,
    ) -> Result<GimbalState, Error> {
        access.authorize(self.scope, Operation::Submit)?;
        let scope = self.scope;
        let entry = self.entry_mut(id)?;
        if entry.state != GimbalState::Prepared {
            return Ok(entry.state);
        }
        entry.request.validate(port.capability()?)?;
        entry.state = GimbalState::Unknown;
        match port.submit(scope, entry.request) {
            Ok(ticket) => {
                entry.ticket = Some(ticket);
                entry.state = GimbalState::Running;
                Ok(entry.state)
            }
            Err(error) => {
                entry.state = GimbalState::Terminal {
                    result: GimbalResult::Failed,
                    reason: 0,
                };
                Err(error)
            }
        }
    }
    /// Observe the actual backend ticket; accepted responses never imply physical completion.
    /// # Errors
    /// Returns unknown identity, invalid lifecycle, denied access or actual backend failure.
    pub fn poll(
        &mut self,
        id: u32,
        access: &impl AccessPort,
        port: &mut impl GimbalPort,
    ) -> Result<GimbalState, Error> {
        access.authorize(self.scope, Operation::Submit)?;
        let scope = self.scope;
        let entry = self.entry_mut(id)?;
        match entry.state {
            GimbalState::Terminal { .. } | GimbalState::Unknown => return Ok(entry.state),
            GimbalState::Prepared => return Err(Error::State),
            GimbalState::Running => {}
        }
        let ticket = entry.ticket.ok_or(Error::State)?;
        match port.poll(scope, ticket) {
            Ok(GimbalObservation::Running) => {}
            Ok(GimbalObservation::Terminal { result, reason }) => {
                entry.state = GimbalState::Terminal { result, reason }
            }
            Err(error) => {
                entry.state = GimbalState::Terminal {
                    result: GimbalResult::Failed,
                    reason: 0,
                };
                return Err(error);
            }
        }
        Ok(entry.state)
    }
    /// Query current state.
    /// # Errors
    /// Unknown IDs return `NotFound`.
    pub fn state(&self, id: u32) -> Result<GimbalState, Error> {
        Ok(self.entry(id)?.state)
    }
    /// Encode E82 only after a real terminal result: `action_id:u32,result:u8,reason:u16`.
    /// # Errors
    /// Returns unconfirmed/unknown identity or insufficient output capacity.
    pub fn result_event(&self, id: u32, out: &mut [u8]) -> Result<usize, Error> {
        let GimbalState::Terminal { result, reason } = self.entry(id)?.state else {
            return Err(Error::State);
        };
        let mut writer = Writer::new(out);
        writer.u32(id)?;
        writer.u8(result as u8)?;
        writer.u16(reason)?;
        Ok(writer.len())
    }
    /// Clear pending scope/CCCD work and request real cancellation; unconfirmed outcomes become Unknown.
    /// # Errors
    /// Returns Backend if cancellation failed, while affected actions still become Unknown.
    pub fn invalidate(&mut self, port: &mut impl GimbalPort) -> Result<usize, Error> {
        let mut count = 0;
        let mut failed = false;
        for entry in self.entries.iter_mut().flatten() {
            if matches!(entry.state, GimbalState::Prepared | GimbalState::Running) {
                if let Some(ticket) = entry.ticket
                    && port.cancel(self.scope, ticket).is_err()
                {
                    failed = true;
                }
                entry.state = GimbalState::Unknown;
                count += 1;
            }
        }
        if failed {
            Err(Error::Backend)
        } else {
            Ok(count)
        }
    }
    /// Replace a connection after handling old Unknown/cleanup outcomes.
    /// # Errors
    /// Rejects same scope or unresolved active work.
    pub fn replace_scope(&mut self, scope: Scope) -> Result<(), Error> {
        if scope == self.scope {
            return Err(Error::Conflict);
        }
        if self
            .entries
            .iter()
            .flatten()
            .any(|entry| matches!(entry.state, GimbalState::Prepared | GimbalState::Running))
        {
            return Err(Error::State);
        }
        self.scope = scope;
        self.highwater = 0;
        self.entries = [None; N];
        Ok(())
    }
    fn entry(&self, id: u32) -> Result<&Entry, Error> {
        self.entries
            .iter()
            .flatten()
            .find(|entry| entry.request.action_id == id)
            .ok_or(Error::NotFound)
    }
    fn entry_mut(&mut self, id: u32) -> Result<&mut Entry, Error> {
        self.entries
            .iter_mut()
            .flatten()
            .find(|entry| entry.request.action_id == id)
            .ok_or(Error::NotFound)
    }
}
impl GimbalRequest {
    /// Decode candidate schema2 exact 11B parameters without performing an action.
    /// # Errors
    /// Returns malformed length, zero identity or invalid syntax. Real range validation occurs at prepare.
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        let mut reader = Reader::new(bytes);
        let value = Self {
            action_id: reader.u32()?,
            action: reader.u8()?,
            direction: reader.u8()?,
            mode: reader.u8()?,
            value: reader.u32()?,
        };
        reader.end()?;
        value.syntax()?;
        Ok(value)
    }
    /// Encode exact candidate schema2 request bytes.
    /// # Errors
    /// Returns invalid syntax or insufficient output capacity.
    pub fn encode(self, out: &mut [u8]) -> Result<usize, Error> {
        self.syntax()?;
        let mut writer = Writer::new(out);
        writer.u32(self.action_id)?;
        writer.u8(self.action)?;
        writer.u8(self.direction)?;
        writer.u8(self.mode)?;
        writer.u32(self.value)?;
        Ok(writer.len())
    }
    fn syntax(self) -> Result<(), Error> {
        ensure(
            self.action_id > 0
                && self.action <= 1
                && self.direction <= 1
                && self.mode <= 1
                && (self.action != 0 || (self.direction == 0 && self.mode == 0 && self.value == 0)),
        )
    }
    fn validate(self, cap: GimbalCapability) -> Result<(), Error> {
        self.syntax()?;
        ensure(
            cap.revision > 0
                && cap.modes > 0
                && cap.modes & !3 == 0
                && cap.directions > 0
                && cap.directions & !3 == 0,
        )?;
        if self.action == 0 {
            return Ok(());
        }
        ensure(cap.modes & (1 << self.mode) != 0 && cap.directions & (1 << self.direction) != 0)?;
        let (min, max) = if self.mode == 0 {
            (cap.min_duration_ms, cap.max_duration_ms)
        } else {
            (cap.min_angle_centi_deg, cap.max_angle_centi_deg)
        };
        ensure(min > 0 && min <= max && self.value >= min && self.value <= max)
    }
}
