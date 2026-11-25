//! Commercial02 jobs: independent of every engineering/factory implementation.
use crate::common::{Guard, entry, failure, frame, id16, put16, put32, status};
use crate::{Clock, Error, NamedRequest};
use core::cell::{Cell, RefCell};
use device_protocol::{Frame, ModuleEntry};
use device_services::{Reason, ServiceHandler, SessionError, Status};
/// Trusted actual implementation assertion, never peer-provided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Durable {
    Unavailable,
    Implemented,
}
/// Mandatory real isolated-ledger and recovery guarantees.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceJobGuarantees {
    pub atomic_identity: Durable,
    pub durable_identity: Durable,
    pub durable_terminal: Durable,
    pub current_boot_recovered: Durable,
    pub reset_preserves_ledger: Durable,
    pub retained_terminal: u16,
}
/// V06 persistent states, distinct from71 self-test states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DeviceJobState {
    Accepted = 1,
    Running = 2,
    Succeeded = 3,
    Failed = 4,
    Interrupted = 5,
}
/// Actual persisted immutable binding and actual state. Debug hides all private bindings.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DeviceJobRecord {
    pub operation_id: [u8; 16],
    pub operation: u16,
    pub job: u32,
    pub principal: [u8; 16],
    pub policy_revision: u16,
    pub state: DeviceJobState,
    pub result: u16,
    pub reason: u16,
}
impl core::fmt::Debug for DeviceJobRecord {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DeviceJobRecord")
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}
/// Actual durable claim, including exact replay indication.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceJobClaim {
    pub record: DeviceJobRecord,
    pub replayed: bool,
}
/// Independent real authentication and per-operation scopes. Destructive operation permission and
/// dangerous permission must both be granted. Station recovery may explicitly read an older owner.
pub trait DeviceJobAccess {
    fn identity(&self) -> Option<[u8; 16]>;
    fn start_allowed(&self, operation: u16) -> bool;
    fn dangerous_allowed(&self) -> bool;
    fn read_scope(&self) -> bool;
    fn may_read(&self, record: DeviceJobRecord) -> bool;
}
/// Durable execution-claim result. Running/terminal replays never perform physical work again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionClaim {
    Acquired(DeviceJobRecord),
    AlreadyStarted(DeviceJobRecord),
}
/// Real isolated persistent ledger. `claim_atomic` compares `operation_id,operation,principal` and all
/// parameter bytes; persists a nonreusing job before acceptance, and never executes the operation.
/// `begin_execution_atomic` atomically changes Accepted to Running at actual response admission.
/// Reboot recovery persists Interrupted for every unresolved old job; it never replays physical work.
/// At least32 unconfirmed terminal records+dedup tombstones survive `DATA_CLEAR/FACTORY_RESET`.
pub trait DeviceJobLedger {
    fn guarantees(&self) -> DeviceJobGuarantees;
    fn available(&self, operation: u16) -> bool;
    /// # Errors
    /// Exact binding conflicts, storage-full, authorization and unknown commit outcomes remain errors.
    fn claim_atomic(
        &mut self,
        request: NamedRequest<'_>,
        access: &dyn DeviceJobAccess,
    ) -> Result<DeviceJobClaim, Error>;
    /// # Errors
    /// Rechecks actual read scope/binding inside the same serialized consistent snapshot.
    fn query(&self, id: [u8; 16], access: &dyn DeviceJobAccess) -> Result<DeviceJobRecord, Error>;
    /// # Errors
    /// Rechecks live operation+D permission, original full binding and Accepted state atomically.
    fn begin_execution_atomic(
        &mut self,
        record: DeviceJobRecord,
        access: &dyn DeviceJobAccess,
    ) -> Result<ExecutionClaim, Error>;
    /// Trusted runtime-only read for completion driving, never an unscoped wire entry point.
    /// # Errors
    /// Unknown identity or storage/integrity failure remains explicit.
    fn execution_record(&self, id: [u8; 16]) -> Result<DeviceJobRecord, Error>;
    /// Trusted lookup under the complete immutable principal/job binding.
    /// Existing single-domain ledgers may use this compatibility default.
    /// # Errors
    /// Unknown, ambiguous or changed bindings remain explicit failures.
    fn execution_record_bound(&self, record: DeviceJobRecord) -> Result<DeviceJobRecord, Error> {
        let current = self.execution_record(record.operation_id)?;
        if !same(record, current) {
            return Err(Error::Conflict);
        }
        Ok(current)
    }
    /// Persist actual terminal outcome before exposing it or permitting a reset. No current App
    /// authentication is needed to record a real already-authorized physical outcome after unbinding.
    /// # Errors
    /// Must atomically match original job/binding and preserve old state on known failure.
    fn finish_atomic(
        &mut self,
        record: DeviceJobRecord,
        terminal: DeviceJobTerminal,
    ) -> Result<DeviceJobRecord, Error>;
}
/// Actual verified terminal operation outcome. Nonzero existing errors required for failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceJobTerminal {
    pub state: DeviceJobState,
    pub result: u16,
    pub reason: u16,
}
/// Deferred is actual ongoing work, never successful terminal completion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceJobProgress {
    Deferred,
    Terminal(DeviceJobTerminal),
}
/// Actual `DATA_CLEAR/FACTORY_RESET` execution. Clear stops acquisition and atomically rotates the
/// dataset while clearing the documented owner domains; reset additionally restores trusted defaults
/// and binding. Identity, calibration and isolated dedup ledger survive. No partial success.
pub trait DeviceJobExecutor {
    fn available(&self, operation: u16) -> bool;
    /// # Errors
    /// Recheck the supplied live access at the actual physical commit. An uncertain physical
    /// outcome returns an error and remains Running for explicit recovery.
    fn begin(
        &mut self,
        record: DeviceJobRecord,
        access: &dyn DeviceJobAccess,
    ) -> Result<DeviceJobProgress, Error>;
    /// # Errors
    /// Poll only actual started work; no invented elapsed-time or zero successful outcome.
    fn poll(&mut self, record: DeviceJobRecord) -> Result<DeviceJobProgress, Error>;
    /// Final reset/reboot may occur only after the actual terminal record was durably stored.
    /// # Errors
    /// An uncertain final action remains explicit; never rerun the already-persisted job.
    fn terminal_persisted(&mut self, record: DeviceJobRecord) -> Result<(), Error>;
}
struct State<L, X> {
    ledger: L,
    executor: X,
}
/// Boot-global ledger/executor owner, usable by a trusted background completion driver after CCCD
/// disable/disconnect. No physical execution is hidden in claim or query.
pub struct DeviceJobsOwner<L, X> {
    state: RefCell<State<L, X>>,
}
impl<L, X> core::fmt::Debug for DeviceJobsOwner<L, X> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DeviceJobsOwner").finish_non_exhaustive()
    }
}
impl<L: DeviceJobLedger, X: DeviceJobExecutor> DeviceJobsOwner<L, X> {
    pub const fn new(ledger: L, executor: X) -> Self {
        Self {
            state: RefCell::new(State { ledger, executor }),
        }
    }
    /// Release the actual ledger and executor after connection handlers have dropped.
    /// The caller can explicitly recover storage and rebuild the owner without executing work.
    pub fn into_parts(self) -> (L, X) {
        let state = self.state.into_inner();
        (state.ledger, state.executor)
    }
    /// Poll actual work under its full immutable principal/job binding.
    /// # Errors
    /// Reject invalid/stale bindings, non-Running work and actual execution/storage failures.
    pub fn poll_bound(&self, record: DeviceJobRecord) -> Result<(), Error> {
        valid(record)?;
        let mut state = self.state.try_borrow_mut().map_err(|_| Error::Busy)?;
        guarantees(state.ledger.guarantees())?;
        let current = state.ledger.execution_record_bound(record)?;
        valid(current)?;
        if !same(record, current) {
            return Err(Error::Storage);
        }
        if current.state != DeviceJobState::Running {
            return Err(Error::Conflict);
        }
        let progress = state.executor.poll(current)?;
        finish(&mut state, current, progress)
    }
    /// # Errors
    /// Actual execution/storage failures remain explicit; only an already Running actual job is polled.
    pub fn poll(&self, id: [u8; 16]) -> Result<(), Error> {
        let mut s = self.state.try_borrow_mut().map_err(|_| Error::Busy)?;
        if id == [0; 16] {
            return Err(Error::Invalid);
        }
        guarantees(s.ledger.guarantees())?;
        let r = s.ledger.execution_record(id)?;
        valid(r)?;
        if r.operation_id != id {
            return Err(Error::Storage);
        }
        if r.state != DeviceJobState::Running {
            return Err(Error::Conflict);
        }
        let progress = s.executor.poll(r)?;
        finish(&mut s, r, progress)
    }
    fn check(&self) -> Result<(), Error> {
        let s = self.state.try_borrow().map_err(|_| Error::Busy)?;
        guarantees(s.ledger.guarantees())
    }
}
/// Connection-local02.C40/C41 handler. Body schema1 means exactly[01,00] for `DATA_CLEAR0001` and
/// `FACTORY_RESET0002`. Commercial operation has no dependency on maintenance72/73 or station profiles.
pub struct DeviceJobsHandler<'a, L, X> {
    owner: &'a DeviceJobsOwner<L, X>,
    metadata: ModuleEntry,
    access: &'a dyn DeviceJobAccess,
    guard: Guard<'a>,
    identity: Option<[u8; 16]>,
    pending: Cell<Option<(u16, DeviceJobRecord)>>,
    context: Cell<Option<DeviceJobRecord>>,
    last_admitted: Cell<Option<u16>>,
}
impl<L, X> core::fmt::Debug for DeviceJobsHandler<'_, L, X> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DeviceJobsHandler").finish_non_exhaustive()
    }
}
impl<'a, L: DeviceJobLedger, X: DeviceJobExecutor> DeviceJobsHandler<'a, L, X> {
    /// # Errors
    /// Rejects invalid02/v1.0 metadata or missing actual durable/recovery guarantees.
    pub fn new(
        owner: &'a DeviceJobsOwner<L, X>,
        metadata: ModuleEntry,
        access: &'a dyn DeviceJobAccess,
        clock: &'a dyn Clock,
    ) -> Result<Self, Error> {
        entry(metadata, 2, 1, 0)?;
        owner.check()?;
        Ok(Self {
            owner,
            metadata,
            access,
            guard: Guard::new(clock),
            identity: access.identity(),
            pending: Cell::new(None),
            context: Cell::new(None),
            last_admitted: Cell::new(None),
        })
    }
    fn start_role(&self, op: u16) -> bool {
        self.access.identity().is_some_and(|id| id != [0; 16]) && self.access.start_allowed(op)
    }
    fn start_scope(&self, op: u16) -> bool {
        self.access.identity().is_some_and(|id| id != [0; 16])
            && self.access.start_allowed(op)
            && self.access.dangerous_allowed()
    }
    fn close(&self) {
        self.guard.close();
        self.pending.set(None);
        self.context.set(None);
    }
    fn execute(&mut self, f: Frame<'_>, out: &mut [u8]) -> Result<usize, Error> {
        match f.header.opcode {
            0x40 => {
                let r = NamedRequest::decode(f.payload)?;
                if !matches!(r.operation, 1 | 2) || r.params != [1, 0] {
                    return Err(Error::Invalid);
                }
                if out.len() < 8 {
                    return Err(Error::Capacity);
                }
                if !self.start_role(r.operation) {
                    return Err(Error::Denied);
                }
                if !self.access.dangerous_allowed() {
                    return Err(Error::DangerousDenied);
                }
                let mut s = self.owner.state.try_borrow_mut().map_err(|_| Error::Busy)?;
                guarantees(s.ledger.guarantees())?;
                if !s.ledger.available(r.operation) || !s.executor.available(r.operation) {
                    return Err(Error::Unsupported);
                }
                let claim = s.ledger.claim_atomic(r, self.access)?;
                let record = claim.record;
                valid(record)?;
                if record.operation_id != r.operation_id
                    || record.operation != r.operation
                    || Some(record.principal) != self.access.identity()
                    || (!claim.replayed && record.state != DeviceJobState::Accepted)
                {
                    return Err(Error::Storage);
                }
                self.pending.set(Some((f.header.message_id, record)));
                self.context.set(Some(record));
                put32(out, 4, record.job);
                status(out, Status::Ok, Reason::None);
                Ok(8)
            }
            0x41 => {
                let id = id16(f.payload)?;
                if out.len() < 9 {
                    return Err(Error::Capacity);
                }
                let s = self.owner.state.try_borrow().map_err(|_| Error::Busy)?;
                guarantees(s.ledger.guarantees())?;
                let r = s.ledger.query(id, self.access)?;
                valid(r)?;
                if r.operation_id != id {
                    return Err(Error::Storage);
                }
                if !self.access.may_read(r) {
                    return Err(Error::Denied);
                }
                self.context.set(Some(r));
                out[4] = r.state as u8;
                put16(out, 5, r.result);
                put16(out, 7, r.reason);
                status(out, Status::Ok, Reason::None);
                Ok(9)
            }
            _ => Err(Error::Unsupported),
        }
    }
}
impl<L: DeviceJobLedger, X: DeviceJobExecutor> ServiceHandler for DeviceJobsHandler<'_, L, X> {
    fn required_features(&self, e: &ModuleEntry, op: u8) -> Option<u32> {
        (*e == self.metadata && matches!(op, 0x40 | 0x41)).then_some(0)
    }
    fn authorized(&self, f: Frame<'_>) -> bool {
        !self.guard.closed()
            && match f.header.opcode {
                0x40 => {
                    NamedRequest::decode(f.payload).map_or(true, |r| self.start_role(r.operation))
                }
                0x41 => {
                    self.access.identity().is_some_and(|id| id != [0; 16])
                        && self.access.read_scope()
                }
                _ => true,
            }
    }
    fn must_end_session(&self) -> bool {
        if self.identity != self.access.identity()
            || self
                .pending
                .get()
                .is_some_and(|(_, r)| !self.start_scope(r.operation))
        {
            self.close();
        }
        self.guard.closed()
    }
    fn replay_valid(&self, f: Frame<'_>, response: &[u8]) -> bool {
        if self.guard.closed() || response.len() < 4 {
            return false;
        }
        if response[..2] != [0, 0] {
            return true;
        }
        let Some(old) = self.context.get() else {
            return false;
        };
        let id = match f.header.opcode {
            0x40 => NamedRequest::decode(f.payload).and_then(|r| {
                if r.operation_id == old.operation_id
                    && r.operation == old.operation
                    && r.params == [1, 0]
                {
                    Ok(r.operation_id)
                } else {
                    Err(Error::Conflict)
                }
            }),
            0x41 => id16(f.payload),
            _ => return false,
        };
        let Ok(id) = id else { return false };
        let Ok(s) = self.owner.state.try_borrow() else {
            return false;
        };
        if guarantees(s.ledger.guarantees()).is_err() {
            return false;
        }
        let result = if f.header.opcode == 0x40 {
            if !self.start_scope(old.operation) {
                return false;
            }
            s.ledger.execution_record_bound(old)
        } else {
            s.ledger.query(id, self.access)
        };
        result.is_ok_and(|r| {
            valid(r).is_ok() && same(r, old) && (f.header.opcode == 0x40 || self.access.may_read(r))
        })
    }
    fn response_admitted(&mut self, f: Frame<'_>, response: &[u8]) -> Result<(), SessionError> {
        if self.must_end_session() {
            return Err(SessionError::AuthorizationRevoked);
        }
        self.guard.now().map_err(crate::common::local)?;
        if f.header.opcode != 0x40 {
            return Ok(());
        }
        if self.last_admitted.get() == Some(f.header.message_id) {
            return Ok(());
        }
        let Some((request, record)) = self.pending.get() else {
            return Err(SessionError::InvalidHandlerResponse);
        };
        if request != f.header.message_id
            || response.len() != 8
            || response[..4] != [0, 0, 0, 0]
            || response[4..8] != record.job.to_le_bytes()
            || !self.replay_valid(f, response)
        {
            self.close();
            return Err(SessionError::AuthorizationRevoked);
        }
        let result = (|| {
            let mut s = self.owner.state.try_borrow_mut().map_err(|_| Error::Busy)?;
            guarantees(s.ledger.guarantees())?;
            if !self.start_scope(record.operation) {
                return Err(Error::Denied);
            }
            if record.state == DeviceJobState::Accepted {
                match s.ledger.begin_execution_atomic(record, self.access)? {
                    ExecutionClaim::AlreadyStarted(r) => {
                        valid(r)?;
                        if !same(record, r) || r.state == DeviceJobState::Accepted {
                            return Err(Error::Backend);
                        }
                    }
                    ExecutionClaim::Acquired(r) => {
                        valid(r)?;
                        if !same(record, r) || r.state != DeviceJobState::Running {
                            return Err(Error::Backend);
                        }
                        let progress = s.executor.begin(r, self.access)?;
                        finish(&mut s, r, progress)?;
                    }
                }
            }
            Ok(())
        })();
        if let Err(error) = result {
            self.close();
            return Err(crate::common::local(error));
        }
        self.last_admitted.set(Some(f.header.message_id));
        self.pending.set(None);
        Ok(())
    }
    fn connection_closed(&mut self) -> Result<(), SessionError> {
        self.close();
        Ok(())
    }
    fn dispatch(&mut self, f: Frame<'_>, out: &mut [u8]) -> Result<usize, SessionError> {
        frame(f, self.metadata)?;
        if self.must_end_session() {
            out.fill(0);
            return Err(SessionError::AuthorizationRevoked);
        }
        self.guard.now().map_err(crate::common::local)?;
        let required = match f.header.opcode {
            0x40 => 8,
            0x41 => 9,
            _ => 4,
        };
        if out.len() < required {
            return Err(SessionError::Capacity);
        }
        if !self.authorized(f) {
            return Ok(status(out, Status::AccessDenied, Reason::RoleRequired));
        }
        let result = self.execute(f, out);
        if !self.authorized(f) || self.must_end_session() {
            self.close();
            out.fill(0);
            return Err(SessionError::AuthorizationRevoked);
        }
        match result {
            Ok(n) => Ok(n),
            Err(e) => Ok(failure(out, e)),
        }
    }
}
fn guarantees(g: DeviceJobGuarantees) -> Result<(), Error> {
    if [
        g.atomic_identity,
        g.durable_identity,
        g.durable_terminal,
        g.current_boot_recovered,
        g.reset_preserves_ledger,
    ]
    .iter()
    .any(|v| *v != Durable::Implemented)
        || g.retained_terminal < 32
    {
        Err(Error::Unavailable)
    } else {
        Ok(())
    }
}
fn valid(r: DeviceJobRecord) -> Result<(), Error> {
    if r.operation_id == [0; 16]
        || r.job == 0
        || r.principal == [0; 16]
        || r.policy_revision == 0
        || !matches!(r.operation, 1 | 2)
        || r.result > 0x12
        || r.result == 0x10
    {
        return Err(Error::Backend);
    }
    let ok = match r.state {
        DeviceJobState::Accepted | DeviceJobState::Running => r.result == 0,
        DeviceJobState::Succeeded => r.result == 0 && r.reason == 0,
        DeviceJobState::Failed | DeviceJobState::Interrupted => r.result != 0,
    };
    if ok { Ok(()) } else { Err(Error::Backend) }
}
fn same(a: DeviceJobRecord, b: DeviceJobRecord) -> bool {
    a.operation_id == b.operation_id
        && a.operation == b.operation
        && a.job == b.job
        && a.principal == b.principal
        && a.policy_revision == b.policy_revision
}
fn finish<L: DeviceJobLedger, X: DeviceJobExecutor>(
    s: &mut State<L, X>,
    r: DeviceJobRecord,
    p: DeviceJobProgress,
) -> Result<(), Error> {
    if let DeviceJobProgress::Terminal(t) = p {
        let mut expected = r;
        expected.state = t.state;
        expected.result = t.result;
        expected.reason = t.reason;
        valid(expected)?;
        if matches!(t.state, DeviceJobState::Accepted | DeviceJobState::Running) {
            return Err(Error::Backend);
        }
        let stored = s.ledger.finish_atomic(r, t)?;
        valid(stored)?;
        if !same(stored, expected) || stored != expected {
            return Err(Error::Backend);
        }
        s.executor.terminal_persisted(stored)?;
    }
    Ok(())
}
