use crate::{
    AccessPort, Error, Operation, Scope,
    wire::{Reader, Writer, ensure},
};
/// Frozen FIND target dictionary, also used by candidate E80.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FindTarget {
    /// This device.
    Device = 0,
    /// Authenticated source phone.
    Phone = 1,
}
/// FIND action parameters, exactly 4B.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FindRequest {
    /// Target 0 device, 1 phone.
    pub target: FindTarget,
    /// 1 vibration, 2 screen flash, 3 both.
    pub pattern: u8,
    /// Duration in seconds, constrained by actual backend limits.
    pub duration_s: u16,
}
impl FindRequest {
    /// Decode exact frozen FIND parameters.
    /// # Errors
    /// Rejects target, pattern, zero duration or wrong length.
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        let mut r = Reader::new(bytes);
        let target = match r.u8()? {
            0 => FindTarget::Device,
            1 => FindTarget::Phone,
            _ => return Err(Error::Invalid),
        };
        let value = Self {
            target,
            pattern: r.u8()?,
            duration_s: r.u16()?,
        };
        r.end()?;
        value.validate()?;
        Ok(value)
    }
    fn validate(self) -> Result<(), Error> {
        ensure((1..=3).contains(&self.pattern) && self.duration_s > 0)
    }
}
/// Immutable terminal result. E81 uses candidate 1/2/3/4; C32 has its frozen mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FindResult {
    /// User found the target.
    Found = 1,
    /// User stopped finding.
    UserStop = 2,
    /// Actual external monotonic deadline expired.
    Timeout = 3,
    /// Real backend failed.
    BackendFailure = 4,
}
impl FindResult {
    /// Decode frozen C32: 1 found, 2 timeout, 3 cancelled.
    /// # Errors
    /// Rejects other result values.
    pub fn from_command(value: u8) -> Result<Self, Error> {
        match value {
            1 => Ok(Self::Found),
            2 => Ok(Self::Timeout),
            3 => Ok(Self::UserStop),
            _ => Err(Error::Invalid),
        }
    }
}
/// Finding lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FindState {
    /// Waiting for transport admission.
    Prepared,
    /// Real backend started finding.
    Active,
    /// STOP accepted but not admitted.
    StopPrepared,
    /// Actual immutable terminal result.
    Terminal(FindResult),
    /// Connection ended without a confirmed result.
    Unknown,
}
/// Actual target capability and bounded duration supplied by product backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FindCapability {
    /// Supported patterns encoded as bits 0 vibration, 1 flash.
    pub patterns: u8,
    /// Minimum positive duration.
    pub min_duration_s: u16,
    /// Maximum duration.
    pub max_duration_s: u16,
}
/// Real find backend. The state machine never provides a production simulation.
pub trait FindPort: core::fmt::Debug {
    /// Return actual capability for a target.
    /// # Errors
    /// Returns `Unavailable` for a missing backend.
    fn capability(&self, target: FindTarget) -> Result<FindCapability, Error>;
    /// Start admitted finding with its actual deadline.
    /// # Errors
    /// Returns actual backend failure.
    fn start(
        &mut self,
        scope: Scope,
        id: u32,
        request: FindRequest,
        deadline_ms: u64,
    ) -> Result<(), Error>;
    /// Stop actual finding before a terminal event is emitted.
    /// # Errors
    /// Returns actual backend failure.
    fn stop(&mut self, scope: Scope, id: u32) -> Result<(), Error>;
}
#[derive(Debug, Clone, Copy)]
struct Entry {
    id: u32,
    request: FindRequest,
    state: FindState,
    deadline: u64,
}
/// Bounded finding ledger with external monotonic time and explicit transport admission.
pub struct FindService<const N: usize> {
    scope: Scope,
    next: u32,
    last_ms: u64,
    entries: [Option<Entry>; N],
}
impl<const N: usize> core::fmt::Debug for FindService<N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FindService")
            .field("length", &self.entries.iter().flatten().count())
            .finish_non_exhaustive()
    }
}
impl<const N: usize> FindService<N> {
    /// Start an empty authenticated connection ledger.
    #[must_use]
    pub const fn new(scope: Scope) -> Self {
        Self {
            scope,
            next: 1,
            last_ms: 0,
            entries: [None; N],
        }
    }
    /// Reserve a FIND identity; does not start an actuator or send a phone event.
    /// # Errors
    /// Rejects malformed/range-limited input, unavailable target, denied access or full ledger.
    pub fn prepare(
        &mut self,
        request: FindRequest,
        access: &impl AccessPort,
        port: &impl FindPort,
    ) -> Result<u32, Error> {
        request.validate()?;
        access.authorize(self.scope, Operation::Find)?;
        let cap = port.capability(request.target)?;
        ensure(
            cap.patterns & !3 == 0
                && cap.min_duration_s > 0
                && cap.min_duration_s <= cap.max_duration_s
                && request.pattern & !cap.patterns == 0
                && request.duration_s >= cap.min_duration_s
                && request.duration_s <= cap.max_duration_s,
        )?;
        if self.entries.iter().flatten().any(|e| {
            matches!(
                e.state,
                FindState::Prepared | FindState::Active | FindState::StopPrepared
            )
        }) {
            return Err(Error::Conflict);
        }
        let slot = self
            .entries
            .iter_mut()
            .find(|e| e.is_none())
            .ok_or(Error::Capacity)?;
        let id = self.next;
        let next = id.checked_add(1).ok_or(Error::Capacity)?;
        *slot = Some(Entry {
            id,
            request,
            state: FindState::Prepared,
            deadline: 0,
        });
        self.next = next;
        Ok(id)
    }
    /// Start finding after the response was admitted; duration begins at actual backend submission.
    /// # Errors
    /// Returns unknown ID, denied access, clock reversal/overflow, unavailable backend or execution error.
    pub fn response_admitted(
        &mut self,
        id: u32,
        now_ms: u64,
        access: &impl AccessPort,
        port: &mut impl FindPort,
    ) -> Result<FindState, Error> {
        self.clock(now_ms)?;
        access.authorize(self.scope, Operation::Find)?;
        let scope = self.scope;
        let e = self.entry_mut(id)?;
        if e.state != FindState::Prepared {
            return Ok(e.state);
        }
        let cap = port.capability(e.request.target)?;
        ensure(
            e.request.pattern & !cap.patterns == 0
                && e.request.duration_s >= cap.min_duration_s
                && e.request.duration_s <= cap.max_duration_s,
        )?;
        let deadline = now_ms
            .checked_add(u64::from(e.request.duration_s) * 1000)
            .ok_or(Error::State)?;
        e.deadline = deadline;
        e.state = FindState::Active;
        if let Err(error) = port.start(scope, id, e.request, deadline) {
            e.state = FindState::Terminal(FindResult::BackendFailure);
            return Err(error);
        }
        Ok(e.state)
    }
    /// Accept STOP without touching the backend. Known terminal IDs are idempotent.
    /// # Errors
    /// Returns unknown ID, denial, or `State` for an unstarted/unknown operation.
    pub fn prepare_stop(&mut self, id: u32, access: &impl AccessPort) -> Result<(), Error> {
        access.authorize(self.scope, Operation::Find)?;
        let e = self.entry_mut(id)?;
        match e.state {
            FindState::Active => {
                e.state = FindState::StopPrepared;
                Ok(())
            }
            FindState::StopPrepared | FindState::Terminal(_) => Ok(()),
            FindState::Prepared | FindState::Unknown => Err(Error::State),
        }
    }
    /// Stop an actuator only after STOP's successful response entered the queue.
    /// # Errors
    /// Returns ordering errors, denial, unknown identity or actual backend error.
    pub fn stop_response_admitted(
        &mut self,
        id: u32,
        access: &impl AccessPort,
        port: &mut impl FindPort,
    ) -> Result<FindState, Error> {
        access.authorize(self.scope, Operation::Find)?;
        let scope = self.scope;
        let e = self.entry_mut(id)?;
        if matches!(e.state, FindState::Terminal(_)) {
            return Ok(e.state);
        }
        if e.state != FindState::StopPrepared {
            return Err(Error::State);
        }
        match port.stop(scope, id) {
            Ok(()) => {
                e.state = FindState::Terminal(FindResult::UserStop);
                Ok(e.state)
            }
            Err(error) => {
                e.state = FindState::Terminal(FindResult::BackendFailure);
                Err(error)
            }
        }
    }
    /// Poll one actual monotonic deadline; receiving a command never implies finding succeeded.
    /// # Errors
    /// Returns reversed clock, unknown identity or actual stop failure.
    pub fn poll(
        &mut self,
        id: u32,
        now_ms: u64,
        port: &mut impl FindPort,
    ) -> Result<FindState, Error> {
        self.clock(now_ms)?;
        let scope = self.scope;
        let e = self.entry_mut(id)?;
        if matches!(e.state, FindState::Active | FindState::StopPrepared) && now_ms >= e.deadline {
            match port.stop(scope, id) {
                Ok(()) => e.state = FindState::Terminal(FindResult::Timeout),
                Err(error) => {
                    e.state = FindState::Terminal(FindResult::BackendFailure);
                    return Err(error);
                }
            }
        }
        Ok(e.state)
    }
    /// Settle an actual phone result using frozen C32 values 1 found, 2 timeout, 3 cancelled.
    /// # Errors
    /// Requires trusted source-phone authority, a phone-target action and an immutable matching result.
    pub fn phone_result(
        &mut self,
        id: u32,
        result: u8,
        access: &impl AccessPort,
    ) -> Result<(), Error> {
        access.authorize(self.scope, Operation::Settle)?;
        let result = FindResult::from_command(result)?;
        let e = self.entry_mut(id)?;
        if e.request.target != FindTarget::Phone {
            return Err(Error::Denied);
        }
        match e.state {
            FindState::Active | FindState::StopPrepared => {
                e.state = FindState::Terminal(result);
                Ok(())
            }
            FindState::Terminal(old) if old == result => Ok(()),
            FindState::Terminal(_) => Err(Error::Conflict),
            FindState::Prepared | FindState::Unknown => Err(Error::State),
        }
    }
    /// Confirm a real device-target user/backend result after stopping its actuator.
    /// # Errors
    /// Rejects wrong target/order, unknown identity, or actual backend cleanup failure.
    pub fn device_result(
        &mut self,
        id: u32,
        result: FindResult,
        port: &mut impl FindPort,
    ) -> Result<(), Error> {
        let scope = self.scope;
        let e = self.entry_mut(id)?;
        if e.request.target != FindTarget::Device {
            return Err(Error::State);
        }
        match e.state {
            FindState::Terminal(old) if old == result => return Ok(()),
            FindState::Terminal(_) => return Err(Error::Conflict),
            FindState::Active | FindState::StopPrepared => {}
            FindState::Prepared | FindState::Unknown => return Err(Error::State),
        }
        if let Err(error) = port.stop(scope, id) {
            e.state = FindState::Terminal(FindResult::BackendFailure);
            return Err(error);
        }
        e.state = FindState::Terminal(result);
        Ok(())
    }
    /// Encode candidate E80 as ID plus exact C30 target/pattern/duration parameters (8B).
    /// # Errors
    /// Returns unknown identity or insufficient output capacity.
    pub fn request_event(&self, id: u32, out: &mut [u8]) -> Result<usize, Error> {
        let e = self.entry(id)?;
        if e.state != FindState::Prepared {
            return Err(Error::State);
        }
        let mut w = Writer::new(out);
        w.u32(id)?;
        w.u8(e.request.target as u8)?;
        w.u8(e.request.pattern)?;
        w.u16(e.request.duration_s)?;
        Ok(w.len())
    }
    /// Encode E81 only after an actual terminal result (5B).
    /// # Errors
    /// Returns missing/unterminated identity or insufficient capacity.
    pub fn result_event(&self, id: u32, out: &mut [u8]) -> Result<usize, Error> {
        let e = self.entry(id)?;
        let FindState::Terminal(result) = e.state else {
            return Err(Error::State);
        };
        let mut w = Writer::new(out);
        w.u32(id)?;
        w.u8(result as u8)?;
        Ok(w.len())
    }
    /// Query current lifecycle.
    /// # Errors
    /// Returns `NotFound` for an unknown ID.
    pub fn state(&self, id: u32) -> Result<FindState, Error> {
        Ok(self.entry(id)?.state)
    }
    /// End pending scope/CCCD work and stop real actuators; unconfirmed results become Unknown.
    /// # Errors
    /// Returns `Backend` if any actuator cleanup failed, while all affected states still become Unknown.
    pub fn invalidate(&mut self, port: &mut impl FindPort) -> Result<usize, Error> {
        let mut count = 0;
        let mut failed = false;
        for e in self.entries.iter_mut().flatten() {
            if matches!(
                e.state,
                FindState::Prepared | FindState::Active | FindState::StopPrepared
            ) {
                if e.state != FindState::Prepared && port.stop(self.scope, e.id).is_err() {
                    failed = true;
                }
                e.state = FindState::Unknown;
                count += 1;
            }
        }
        if failed {
            Err(Error::Backend)
        } else {
            Ok(count)
        }
    }
    /// Begin a different connection scope; caller must handle old Unknown/cleanup outcomes first.
    /// # Errors
    /// Rejects same scope and unresolved active work.
    pub fn replace_scope(&mut self, scope: Scope) -> Result<(), Error> {
        if scope == self.scope {
            return Err(Error::Conflict);
        }
        if self.entries.iter().flatten().any(|e| {
            matches!(
                e.state,
                FindState::Prepared | FindState::Active | FindState::StopPrepared
            )
        }) {
            return Err(Error::State);
        }
        self.scope = scope;
        self.next = 1;
        self.last_ms = 0;
        self.entries = [None; N];
        Ok(())
    }
    fn clock(&mut self, now: u64) -> Result<(), Error> {
        if now < self.last_ms {
            return Err(Error::State);
        }
        self.last_ms = now;
        Ok(())
    }
    fn entry(&self, id: u32) -> Result<&Entry, Error> {
        self.entries
            .iter()
            .flatten()
            .find(|e| e.id == id)
            .ok_or(Error::NotFound)
    }
    fn entry_mut(&mut self, id: u32) -> Result<&mut Entry, Error> {
        self.entries
            .iter_mut()
            .flatten()
            .find(|e| e.id == id)
            .ok_or(Error::NotFound)
    }
}

impl core::fmt::Debug for FindRequest {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FindRequest")
            .field("encoded_length", &4)
            .finish_non_exhaustive()
    }
}
