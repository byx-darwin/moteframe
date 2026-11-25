//! Persistent incremental history. No synchronous medium is used by this owner.
mod service;
use crate::paged::format::{Format, crc, u16at, u32at, u64at};
use crate::paged::{Lease, LogConfig, LogStats, State, metadata_bytes, slot_bytes};
use crate::repository::matches;
use crate::{AppendRecord, DomainConfig, HistoryClock, HistorySchema, Record};
use core::{
    cell::{Cell, RefCell},
    task::Poll,
};
use device_history_service::{
    HistoryAccess, HistoryFailure, HistoryOperation, Preparation, ReadPlan, StoreLease,
};
use device_protocol::{Crc32c, HistoryDomain, HistoryFilter, HistorySnapshot};
use device_storage::{
    incremental::{
        AsyncError, AsyncMedium, CommitId, Completion, IncrementalSnapshot, MediumOp, StartError,
    },
    persistence::{Error as StorageError, Geometry, Layout},
};
pub use service::PreparedHistory;

/// A token tied to the stable lifetime and address of its actual owner.
#[derive(Clone, Copy)]
pub struct JobToken<'a> {
    owner: &'a Cell<u64>,
    id: u64,
}
impl core::fmt::Debug for JobToken<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("JobToken")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}
impl PartialEq for JobToken<'_> {
    fn eq(&self, other: &Self) -> bool {
        core::ptr::eq(self.owner, other.owner) && self.id == other.id
    }
}
impl Eq for JobToken<'_> {}
/// Terminal results retained until the next successful admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobResult {
    /// Metadata and complete log recovery finished; false means confirmed blank.
    Recovered {
        /// Whether a validated initialized dataset exists.
        initialized: bool,
    },
    /// Explicit blank initialization became durable.
    Initialized,
    /// Complete record is durable and verified.
    Appended {
        /// Confirmed persistent domain sequence.
        id: u64,
    },
    /// Frozen lease installed.
    Opened(StoreLease),
    /// Actual ordered ID plan.
    Planned(ReadPlan),
    /// Actual epoch and matching count.
    Counted {
        /// Actual persistent domain epoch.
        epoch: u32,
        /// Actual eligible record count.
        count: u32,
    },
    /// New or existing durable acknowledgement.
    Acknowledged {
        /// Actual durable inclusive prefix watermark.
        upto: u64,
    },
    /// The single complete verified body is ready.
    RecordReady,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Recover,
    Initialize,
    Append,
    Prepare(Preparation),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Idle,
    RecoverSync,
    MetadataRecover,
    Blank,
    RecoverStart,
    ScanMarker,
    ScanHeader,
    ScanBody,
    ScanTorn,
    ScanFinish,
    ScanNext,
    RetireRecord,
    Extra,
    AppendBlank,
    AppendProgram,
    AppendSync,
    AppendMarker,
    AppendFinalSync,
    MetadataEncode,
    MetadataCommit,
    AckAuthorize,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Command {
    Read(u32, usize),
    Program(u32),
    Sync,
}
impl Command {
    fn completion(self) -> Completion {
        match self {
            Self::Read(_, n) => Completion::Read(n),
            Self::Program(_) => Completion::Programmed,
            Self::Sync => Completion::Synced,
        }
    }
}
#[allow(clippy::struct_excessive_bools)] // Health, delivery, occupancy, marker and ACK witness are independent state axes.
struct Engine<M, L, V, C, const D: usize, const R: usize, const B: usize, const N: usize> {
    snapshot: IncrementalSnapshot<M, N, 1>,
    log: L,
    schema: V,
    clock: C,
    config: LogConfig,
    geometry: Geometry,
    stride: u32,
    data: State<D>,
    work: State<D>,
    revision: u64,
    root: u32,
    metadata_length: usize,
    poisoned: bool,
    lease: LeaseSlot,
    lease_generation: u64,
    next_handle: u64,
    lease_s: u16,
    actual_time: Option<u64>,
    supplied_time: Option<u64>,
    kind: Kind,
    phase: Phase,
    command: Option<Command>,
    token: u64,
    generation: u64,
    detached: bool,
    terminal: Option<Result<JobResult, HistoryFailure>>,
    commit: Option<CommitId>,
    metadata_revision: u64,
    metadata_root: u32,
    metadata_len: usize,
    d: usize,
    slot: u32,
    cut: u32,
    cursor: u32,
    occupied: bool,
    present: bool,
    record: Record<B>,
    expected_crc: u32,
    log_crc: Crc32c,
    stored_crc: Crc32c,
    ack_found: bool,
    prefix_count: u32,
    prefix_first: u64,
    prefix_last: u64,
    high: u64,
    ordinal: usize,
    first: u64,
    last: u64,
    covered: usize,
    plan_count: u16,
    remaining: usize,
    found: Option<usize>,
    cache: Option<(u64, u64, usize)>,
    proof: Option<(u64, u64, usize)>,
}
// A released receipt is observable only until the next successful open.
// Retain it during failed opens, but never alongside an active lease.
#[derive(Clone, Copy)]
enum LeaseSlot {
    Empty,
    Active(Lease),
    Released(StoreLease),
}
impl LeaseSlot {
    fn active(&self) -> Option<&Lease> {
        if let Self::Active(lease) = self {
            Some(lease)
        } else {
            None
        }
    }
    fn active_mut(&mut self) -> Option<&mut Lease> {
        if let Self::Active(lease) = self {
            Some(lease)
        } else {
            None
        }
    }
    fn released(&self) -> Option<StoreLease> {
        if let Self::Released(lease) = self {
            Some(*lease)
        } else {
            None
        }
    }
}
// Geometry and layout validated once before moving an owned snapshot/medium.
#[derive(Debug, Clone, Copy)]
struct ValidatedLog {
    config: LogConfig,
    geometry: Geometry,
    stride: u32,
    lease_s: u16,
}
/// Boot-lifetime asynchronous owner, shared by connection adapters and the worker.
/// D bounds domains, R records per domain, B complete body bytes, N metadata bytes.
/// One job/cache is serialized; there is no resident record or ID directory.
pub struct AsyncPagedHistoryStore<
    M,
    L,
    V,
    C,
    const D: usize,
    const R: usize,
    const B: usize,
    const N: usize,
> {
    inner: RefCell<Engine<M, L, V, C, D, R, B, N>>,
    counter: Cell<u64>,
    generation: Cell<u64>,
}
impl<M, L, V, C, const D: usize, const R: usize, const B: usize, const N: usize> core::fmt::Debug
    for AsyncPagedHistoryStore<M, L, V, C, D, R, B, N>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("AsyncPagedHistoryStore")
            .field("last_token", &self.counter.get())
            .finish_non_exhaustive()
    }
}
impl<
    M: AsyncMedium,
    L: AsyncMedium,
    V: HistorySchema,
    C: HistoryClock,
    const D: usize,
    const R: usize,
    const B: usize,
    const N: usize,
> AsyncPagedHistoryStore<M, L, V, C, D, R, B, N>
{
    /// Validate configuration without I/O. Explicit recovery is required.
    /// # Errors
    /// Invalid geometry, namespace, quotas, metadata or arithmetic is rejected.
    pub fn new_unrecovered(
        snapshot: IncrementalSnapshot<M, N>,
        log: L,
        schema: V,
        clock: C,
        config: LogConfig,
        lease_s: u16,
    ) -> Result<Self, HistoryFailure> {
        let validated = Self::validate_log(&log, config, lease_s)?;
        let snapshot = snapshot
            .into_exclusive()
            .map_err(|(_, error)| failure(error))?;
        Ok(Self::from_validated(
            snapshot, log, schema, clock, validated,
        ))
    }
    /// Construct directly with one RAM image, avoiding a temporary two-image snapshot.
    /// This changes neither the physical two-bank format nor atomic commit/recovery.
    /// The owner keeps its confirmed directory separately; no image reads are made
    /// during an accepted commit. Explicit recovery is still required.
    /// # Errors
    /// Rejects invalid log configuration or metadata geometry/layout. No I/O is started.
    pub fn new_exclusive_unrecovered(
        metadata: (M, Layout),
        log: L,
        schema: V,
        clock: C,
        config: LogConfig,
        lease_s: u16,
    ) -> Result<Self, HistoryFailure> {
        let validated = Self::validate_log(&log, config, lease_s)?;
        // A fresh medium/layout avoids accepting (and accidentally dropping on
        // constructor rejection) a snapshot with an already admitted operation.
        let snapshot = IncrementalSnapshot::<_, N, 1>::new_unrecovered(metadata.0, metadata.1)
            .map_err(|error| match error {
                StorageError::InvalidLayout => HistoryFailure::Invalid,
                other => failure(AsyncError::Storage(other)),
            })?;
        Ok(Self::from_validated(
            snapshot, log, schema, clock, validated,
        ))
    }
    fn validate_log(
        log: &L,
        config: LogConfig,
        lease_s: u16,
    ) -> Result<ValidatedLog, HistoryFailure> {
        let geometry = log.geometry();
        let stride = slot_bytes::<B>(geometry)?;
        if D == 0
            || D > 255
            || R == 0
            || R > 255
            || B > 474
            || N < metadata_bytes::<D>()
            || lease_s == 0
            || config.namespace == [0; 16]
            || config.slots_per_domain < u32n(R)?
            || geometry.erase_size == 0
            || !geometry.erase_size.is_multiple_of(geometry.write_size)
            || !config.log_start.is_multiple_of(geometry.write_size)
        {
            return Err(HistoryFailure::Invalid);
        }
        Ok(ValidatedLog {
            config,
            geometry,
            stride,
            lease_s,
        })
    }
    fn from_validated(
        snapshot: IncrementalSnapshot<M, N, 1>,
        log: L,
        schema: V,
        clock: C,
        validated: ValidatedLog,
    ) -> Self {
        let ValidatedLog {
            config,
            geometry,
            stride,
            lease_s,
        } = validated;
        Self {
            counter: Cell::new(0),
            generation: Cell::new(0),
            inner: RefCell::new(Engine {
                snapshot,
                log,
                schema,
                clock,
                config,
                geometry,
                stride,
                data: State::EMPTY,
                work: State::EMPTY,
                revision: 0,
                root: 0,
                metadata_length: 0,
                poisoned: true,
                lease: LeaseSlot::Empty,
                lease_generation: 0,
                next_handle: 1,
                lease_s,
                actual_time: None,
                supplied_time: None,
                kind: Kind::Recover,
                phase: Phase::Idle,
                command: None,
                token: 0,
                generation: 0,
                detached: false,
                terminal: None,
                commit: None,
                metadata_revision: 0,
                metadata_root: 0,
                metadata_len: 0,
                d: 0,
                slot: 0,
                cut: 0,
                cursor: 0,
                occupied: false,
                present: false,
                record: Record::EMPTY,
                expected_crc: 0,
                log_crc: Crc32c::new(),
                stored_crc: Crc32c::new(),
                ack_found: false,
                prefix_count: 0,
                prefix_first: 0,
                prefix_last: 0,
                high: 0,
                ordinal: 0,
                first: 0,
                last: 0,
                covered: 0,
                plan_count: 0,
                remaining: 0,
                found: None,
                cache: None,
                proof: None,
            }),
        }
    }
    fn admit(
        &self,
        e: &mut Engine<M, L, V, C, D, R, B, N>,
        kind: Kind,
        generation: u64,
    ) -> Result<JobToken<'_>, HistoryFailure> {
        if e.phase != Phase::Idle || e.cache.is_some() {
            return Err(HistoryFailure::Busy);
        }
        let id = self
            .counter
            .get()
            .checked_add(1)
            .ok_or(HistoryFailure::Capacity)?;
        self.counter.set(id);
        e.token = id;
        e.kind = kind;
        e.generation = generation;
        e.detached = false;
        e.terminal = None;
        Ok(JobToken {
            owner: &self.counter,
            id,
        })
    }
    /// Begin ordered metadata and complete log recovery. Performs no I/O.
    /// # Errors
    /// Active work/cache or exhausted token space is explicit.
    pub fn begin_recover(&self) -> Result<JobToken<'_>, HistoryFailure> {
        let mut e = self
            .inner
            .try_borrow_mut()
            .map_err(|_| HistoryFailure::Busy)?;
        // Recovery revokes the cache and lease, but never cancels accepted work.
        if e.phase != Phase::Idle {
            return Err(HistoryFailure::Busy);
        }
        e.cache = None;
        let token = self.admit(&mut e, Kind::Recover, 0)?;
        e.release();
        e.poisoned = true;
        e.phase = Phase::RecoverSync;
        Ok(token)
    }
    /// Initialize explicit identities after a confirmed blank recovery and full FF scan.
    /// # Errors
    /// Nonblank metadata/log, invalid identities, quota or active work is rejected.
    pub fn begin_initialize(
        &self,
        dataset: [u8; 16],
        domains: &[DomainConfig],
    ) -> Result<JobToken<'_>, HistoryFailure> {
        let mut e = self
            .inner
            .try_borrow_mut()
            .map_err(|_| HistoryFailure::Busy)?;
        if !matches!(
            e.terminal,
            Some(Ok(JobResult::Recovered { initialized: false }))
        ) {
            return Err(HistoryFailure::InvalidState);
        }
        if dataset == [0; 16] || domains.is_empty() || domains.len() > D {
            return Err(HistoryFailure::Invalid);
        }
        let mut state = State::EMPTY;
        state.dataset = dataset;
        state.len = domains.len();
        for (i, d) in domains.iter().enumerate() {
            if d.domain == 255 || d.epoch == 0 || i > 0 && domains[i - 1].domain >= d.domain {
                return Err(HistoryFailure::Invalid);
            }
            state.domains[i].domain = d.domain;
            state.domains[i].epoch = d.epoch;
        }
        e.space(&state)?;
        let token = self.admit(&mut e, Kind::Initialize, 0)?;
        e.work = state;
        e.cursor = 0;
        e.phase = Phase::Blank;
        Ok(token)
    }
    /// Accept a complete producer body, without I/O. Accepted work survives detach.
    /// # Errors
    /// Storage uncertainty, schema, quota, finite spare or ID exhaustion is explicit.
    pub fn begin_append(&self, input: AppendRecord<'_>) -> Result<JobToken<'_>, HistoryFailure> {
        let mut e = self
            .inner
            .try_borrow_mut()
            .map_err(|_| HistoryFailure::Busy)?;
        e.ready()?;
        let d = e.data.domain(input.domain)?;
        if input.payload.len() > B
            || e.data.count[d] >= u32n(R)?
            || e.data.tail[d] >= e.config.slots_per_domain
        {
            return Err(HistoryFailure::Capacity);
        }
        if e.phase != Phase::Idle || e.cache.is_some() {
            return Err(HistoryFailure::Busy);
        }
        e.record.domain = input.domain;
        e.record.epoch = e.data.domains[d].epoch;
        e.record.id = e.data.domains[d]
            .newest_id
            .checked_add(1)
            .ok_or(HistoryFailure::Capacity)?;
        e.record.kind = input.record_type;
        e.record.schema = input.schema;
        e.record.utc = input.utc_ms;
        e.record.flags = input.flags;
        e.record.parent = input.parent_session;
        e.record.len = input.payload.len();
        e.record.body[..input.payload.len()].copy_from_slice(input.payload);
        e.record.crc = e
            .record
            .stored()
            .crc()
            .map_err(|_| HistoryFailure::Invalid)?;
        e.schema.validate(e.record.stored(), e.record.parent)?;
        let token = self.admit(&mut e, Kind::Append, 0)?;
        e.d = d;
        e.slot = e.data.tail[d];
        // Header bytes are derived from this accepted owned record on demand.
        e.format().check_header(&e.record, d, e.slot)?;
        e.cursor = 0;
        e.phase = Phase::AppendBlank;
        Ok(token)
    }
    /// Begin an exact typed query/preparation with current trusted permissions.
    /// # Errors
    /// Invalid request, expired lease, denied scope or another job/cache is explicit.
    pub fn begin_prepare(
        &self,
        request: Preparation,
        now_ms: u64,
        access: &dyn HistoryAccess,
    ) -> Result<JobToken<'_>, HistoryFailure> {
        self.prepare_generation(&request, now_ms, access, 0)
    }
    fn prepare_generation(
        &self,
        request: &Preparation,
        now_ms: u64,
        access: &dyn HistoryAccess,
        generation: u64,
    ) -> Result<JobToken<'_>, HistoryFailure> {
        let mut e = self
            .inner
            .try_borrow_mut()
            .map_err(|_| HistoryFailure::Busy)?;
        e.ready()?;
        if generation != 0
            && matches!(
                request,
                Preparation::Plan { .. } | Preparation::Record { .. } | Preparation::Ack { .. }
            )
            && e.lease_generation != generation
        {
            return Err(HistoryFailure::Identity);
        }
        let (d, cut) = e.preflight(request, now_ms, access)?;
        let token = self.admit(&mut e, Kind::Prepare(*request), generation)?;
        e.d = d;
        e.cut = cut;
        e.slot = 0;
        e.ordinal = 0;
        e.high = 0;
        e.first = 0;
        e.last = 0;
        e.covered = 0;
        e.plan_count = 0;
        e.remaining = 0;
        e.found = None;
        if let Preparation::Plan { after_id, .. } = request {
            e.terminal = Some(Ok(JobResult::Planned(ReadPlan {
                ids: [0; 16],
                count: 0,
                end_after_id: *after_id,
                eof: false,
            })));
        }
        e.phase = Phase::ScanMarker;
        Ok(token)
    }
    /// Drive one bounded CPU/I/O slice: at most one medium begin OR poll.
    /// # Errors
    /// Reentrant access is rejected without changing accepted work.
    pub fn step(&self) -> Result<(), HistoryFailure> {
        self.step_with_scratch(&mut [0; 256])
    }
    /// Advance one slice with a transfer page shared by the scheduler's owners.
    /// Nothing in the page is retained between calls, including Pending/Busy.
    /// # Errors
    /// Reentrant owner access is rejected without changing accepted work.
    pub fn step_with_scratch(&self, scratch: &mut [u8; 256]) -> Result<(), HistoryFailure> {
        let mut e = self
            .inner
            .try_borrow_mut()
            .map_err(|_| HistoryFailure::Busy)?;
        if e.phase != Phase::Idle
            && let Err(error) = e.step(scratch)
        {
            e.fail(error);
        }
        Ok(())
    }
    /// Inspect retained result without I/O; stale/foreign tokens are rejected.
    /// # Errors
    /// A token from another owner or superseded job returns `Identity`.
    pub fn result(
        &self,
        token: JobToken<'_>,
    ) -> Result<Poll<Result<JobResult, HistoryFailure>>, HistoryFailure> {
        if !core::ptr::eq(token.owner, &raw const self.counter) {
            return Err(HistoryFailure::Identity);
        }
        let e = self.inner.try_borrow().map_err(|_| HistoryFailure::Busy)?;
        if token.id != e.token || e.detached {
            return Err(HistoryFailure::Identity);
        }
        Ok(if e.phase == Phase::Idle {
            e.terminal.map_or(Poll::Pending, Poll::Ready)
        } else {
            Poll::Pending
        })
    }
    /// Inspect a terminal result without copying a largest-variant job buffer.
    /// The callback cannot retain a reference into the owner. Partial READ plans
    /// remain private until the job is terminal.
    /// # Errors
    /// Foreign/stale tokens and reentrant access fail as in `result`.
    pub fn inspect_result<T>(
        &self,
        token: JobToken<'_>,
        inspect: impl for<'r> FnOnce(&'r JobResult) -> T,
    ) -> Result<Poll<Result<T, HistoryFailure>>, HistoryFailure> {
        if !core::ptr::eq(token.owner, &raw const self.counter) {
            return Err(HistoryFailure::Identity);
        }
        let e = self.inner.try_borrow().map_err(|_| HistoryFailure::Busy)?;
        if token.id != e.token || e.detached {
            return Err(HistoryFailure::Identity);
        }
        if e.phase != Phase::Idle {
            return Ok(Poll::Pending);
        }
        Ok(match e.terminal.as_ref() {
            None => Poll::Pending,
            Some(Ok(result)) => Poll::Ready(Ok(inspect(result))),
            Some(Err(error)) => Poll::Ready(Err(*error)),
        })
    }
    /// Suppress delivery only; an accepted append/ACK always keeps progressing.
    /// # Errors
    /// Stale/foreign token or reentry is explicit.
    pub fn detach(&self, token: JobToken<'_>) -> Result<(), HistoryFailure> {
        if !core::ptr::eq(token.owner, &raw const self.counter) {
            return Err(HistoryFailure::Identity);
        }
        let mut e = self
            .inner
            .try_borrow_mut()
            .map_err(|_| HistoryFailure::Busy)?;
        if e.token != token.id {
            return Err(HistoryFailure::Identity);
        }
        e.detached = true;
        if e.phase == Phase::AckAuthorize {
            e.fail(HistoryFailure::Closed);
        }
        Ok(())
    }
    /// Healthy retained/consumed counts, with no I/O.
    /// # Errors
    /// Unrecovered or uncertain state is unavailable.
    pub fn stats(&self) -> Result<LogStats<D>, HistoryFailure> {
        let e = self.inner.try_borrow().map_err(|_| HistoryFailure::Busy)?;
        e.ready()?;
        Ok(LogStats {
            retained: e.data.count,
            consumed: e.data.tail,
            slots_per_domain: e.config.slots_per_domain,
            slot_bytes: e.stride,
        })
    }
}
fn u32n(n: usize) -> Result<u32, HistoryFailure> {
    u32::try_from(n).map_err(|_| HistoryFailure::Invalid)
}
fn failure(e: AsyncError) -> HistoryFailure {
    use device_storage::persistence::Error;
    match e {
        AsyncError::Busy => HistoryFailure::Busy,
        AsyncError::Storage(Error::Capacity | Error::RevisionExhausted) => HistoryFailure::Capacity,
        AsyncError::Storage(Error::Conflict) => HistoryFailure::Conflict,
        AsyncError::Storage(Error::Corrupt) => HistoryFailure::StoredIntegrity,
        _ => HistoryFailure::Storage,
    }
}
impl<
    M: AsyncMedium,
    L: AsyncMedium,
    V: HistorySchema,
    C: HistoryClock,
    const D: usize,
    const R: usize,
    const B: usize,
    const N: usize,
> Engine<M, L, V, C, D, R, B, N>
{
    fn format(&self) -> Format<'_, D, R, B, N> {
        Format {
            config: self.config,
            geometry: self.geometry,
            stride: self.stride,
            data: if matches!(self.kind, Kind::Recover) && self.phase != Phase::Idle {
                &self.work
            } else {
                &self.data
            },
        }
    }
    fn ready(&self) -> Result<(), HistoryFailure> {
        if self.poisoned {
            Err(HistoryFailure::Storage)
        } else {
            Ok(())
        }
    }
    fn release(&mut self) {
        if let Some(l) = self.lease.active() {
            self.lease = LeaseSlot::Released(l.public);
        }
        self.cache = None;
        self.proof = None;
        self.lease_generation = 0;
    }
    fn space(&self, s: &State<D>) -> Result<(), HistoryFailure> {
        if self
            .config
            .slots_per_domain
            .checked_mul(self.stride)
            .and_then(|n| n.checked_mul(u32n(s.len).ok()?))
            .ok_or(HistoryFailure::Invalid)?
            > self.geometry.capacity
        {
            Err(HistoryFailure::Capacity)
        } else {
            Ok(())
        }
    }
    fn offset(&self) -> Result<u32, HistoryFailure> {
        u32n(self.d)?
            .checked_mul(self.config.slots_per_domain)
            .and_then(|n| n.checked_add(self.slot))
            .and_then(|n| n.checked_mul(self.stride))
            .ok_or(HistoryFailure::Invalid)
    }
    #[inline]
    fn finish(&mut self, result: JobResult) {
        self.phase = Phase::Idle;
        self.terminal = Some(Ok(result));
        if self.detached {
            self.release();
        }
    }
    fn fail(&mut self, error: HistoryFailure) {
        if matches!(
            error,
            HistoryFailure::Storage | HistoryFailure::StoredIntegrity
        ) {
            self.poisoned = true;
            self.release();
        }
        self.phase = Phase::Idle;
        self.terminal = Some(Err(error));
        if self.detached {
            self.release();
        }
    }
    fn io(&mut self, command: Command, scratch: &mut [u8; 256]) -> Result<bool, HistoryFailure> {
        if let Some(active) = self.command {
            if active != command {
                return Err(HistoryFailure::StoredIntegrity);
            }
            match self.log.poll(scratch) {
                Poll::Pending => Ok(false),
                Poll::Ready(Ok(done)) if done == command.completion() => {
                    self.command = None;
                    Ok(true)
                }
                Poll::Ready(_) => {
                    self.command = None;
                    Err(HistoryFailure::Storage)
                }
            }
        } else {
            let op = match command {
                Command::Read(offset, length) => MediumOp::Read { offset, length },
                Command::Program(offset) => MediumOp::Program {
                    offset,
                    bytes: &scratch[..self.geometry.write_size as usize],
                },
                Command::Sync => MediumOp::Sync,
            };
            match self.log.begin(op) {
                Ok(()) => {
                    self.command = Some(command);
                    Ok(false)
                }
                Err(StartError::Busy) => Ok(false),
                Err(StartError::Io) => Err(HistoryFailure::Storage),
            }
        }
    }
    #[inline(never)]
    fn metadata_begin(&mut self) -> Result<(), HistoryFailure> {
        let work = &self.work;
        let context = Format::<D, R, B, N> {
            config: self.config,
            geometry: self.geometry,
            stride: self.stride,
            data: work,
        };
        let revision = self
            .revision
            .checked_add(1)
            .ok_or(HistoryFailure::Capacity)?;
        let mut root = 0;
        let mut length = 0;
        let commit = if B >= N {
            let bytes = &mut self.record.body;
            length = context.encode_slice(work, bytes)?;
            root = crc(&[&bytes[..length]]);
            self.snapshot
                .begin_commit(self.revision, &bytes[..length])
                .map_err(failure)?
        } else {
            self.snapshot
                .begin_commit_encoded(self.revision, |bytes| {
                    let n = context.encode(work, bytes)?;
                    root = crc(&[&bytes[..n]]);
                    length = n;
                    Ok(n)
                })
                .map_err(|error| match error {
                    device_storage::incremental::CommitEncodeError::Storage(error) => {
                        failure(error)
                    }
                    device_storage::incremental::CommitEncodeError::Encoding(error) => error,
                })?
        };
        self.metadata_root = root;
        self.metadata_len = length;
        self.metadata_revision = revision;
        self.commit = Some(commit);
        self.phase = Phase::MetadataCommit;
        Ok(())
    }
    #[allow(clippy::too_many_lines)] // Explicit finite phase dispatch; each arm performs one bounded operation and returns.
    fn step(&mut self, scratch: &mut [u8; 256]) -> Result<(), HistoryFailure> {
        match self.phase {
            Phase::Idle | Phase::AckAuthorize => {}
            // Authorization already admitted the durable ACK. Encoding belongs
            // to this boot-owned worker, outside the CONTROL dispatch stack.
            Phase::MetadataEncode => self.metadata_begin()?,
            Phase::RetireRecord => {
                if let Some(command) = self.command {
                    if !matches!(command, Command::Read(..)) {
                        return Err(HistoryFailure::StoredIntegrity);
                    }
                    if !self.io(command, scratch)? {
                        return Ok(());
                    }
                }
                self.cache = None;
                self.fail(HistoryFailure::Closed);
            }
            Phase::RecoverSync => {
                if self.io(Command::Sync, scratch)? {
                    self.snapshot.begin_recover().map_err(failure)?;
                    self.phase = Phase::MetadataRecover;
                }
            }
            Phase::MetadataRecover => self.step_metadata_recover(scratch)?,
            Phase::Blank => {
                if self.cursor == self.geometry.capacity {
                    if matches!(self.kind, Kind::Initialize) {
                        self.metadata_begin()?;
                    } else {
                        self.finish(JobResult::Recovered { initialized: false });
                    }
                } else {
                    let n = (self.geometry.capacity - self.cursor).min(256) as usize;
                    if self.io(Command::Read(self.cursor, n), scratch)? {
                        if scratch[..n].iter().any(|x| *x != 255) {
                            return Err(HistoryFailure::StoredIntegrity);
                        }
                        self.cursor += u32n(n)?;
                    }
                }
            }
            Phase::RecoverStart => {
                self.slot = 0;
                self.cut = self.config.slots_per_domain;
                self.high = 0;
                self.work.domains[self.d].newest_id = 0;
                self.work.count[self.d] = 0;
                self.work.tail[self.d] = 0;
                self.work.domains[self.d].oldest_id = 0;
                self.work.domains[self.d].newest_id = 0;
                self.ack_found = self.work.ack[self.d] == 0;
                self.prefix_count = 0;
                self.prefix_first = 0;
                self.prefix_last = 0;
                self.phase = Phase::ScanMarker;
            }
            Phase::ScanMarker => {
                if self.slot == self.cut {
                    self.scan_done()?;
                } else {
                    let n = self.geometry.write_size as usize;
                    let at = self.offset()? + self.stride - self.geometry.write_size;
                    if self.io(Command::Read(at, n), scratch)? {
                        self.occupied = scratch[..n].iter().any(|x| *x != 255);
                        self.present = scratch[..n].iter().all(|x| *x == 0xa5);
                        self.cursor = 0;
                        self.phase = if self.present {
                            Phase::ScanHeader
                        } else if matches!(self.kind, Kind::Recover) {
                            Phase::ScanTorn
                        } else {
                            Phase::ScanFinish
                        };
                    }
                }
            }
            Phase::ScanHeader => {
                let n = (self.stride - self.geometry.write_size).min(256) as usize;
                if self.io(Command::Read(self.offset()?, n), scratch)? {
                    self.accept_header(n, scratch)?;
                }
            }
            Phase::ScanBody => {
                let end = self.stride - self.geometry.write_size;
                if self.cursor == end {
                    self.verify()?;
                    self.phase = Phase::ScanFinish;
                } else {
                    let n = (end - self.cursor).min(256) as usize;
                    if self.io(Command::Read(self.offset()? + self.cursor, n), scratch)? {
                        self.scan_chunk(n, scratch)?;
                    }
                }
            }
            Phase::ScanTorn => {
                let end = self.stride - self.geometry.write_size;
                if self.cursor == end {
                    self.phase = Phase::ScanFinish;
                } else {
                    let n = (end - self.cursor).min(256) as usize;
                    if self.io(Command::Read(self.offset()? + self.cursor, n), scratch)? {
                        self.occupied |= scratch[..n].iter().any(|x| *x != 255);
                        self.cursor += u32n(n)?;
                    }
                }
            }
            Phase::ScanFinish => {
                self.visit()?;
                if self.phase == Phase::ScanFinish {
                    self.phase = Phase::ScanNext;
                }
            }
            Phase::ScanNext => {
                self.slot += 1;
                self.phase = Phase::ScanMarker;
            }
            Phase::Extra => {
                if self.cursor == self.geometry.capacity {
                    self.data = self.work;
                    self.revision = self.metadata_revision;
                    self.root = self.metadata_root;
                    self.metadata_length = self.metadata_len;
                    self.poisoned = false;
                    self.finish(JobResult::Recovered { initialized: true });
                } else {
                    let n = (self.geometry.capacity - self.cursor).min(256) as usize;
                    if self.io(Command::Read(self.cursor, n), scratch)? {
                        if scratch[..n].iter().any(|x| *x != 255) {
                            return Err(HistoryFailure::StoredIntegrity);
                        }
                        self.cursor += u32n(n)?;
                    }
                }
            }
            Phase::AppendBlank => {
                if self.cursor == self.stride {
                    self.cursor = 0;
                    self.phase = Phase::AppendProgram;
                } else {
                    let n = (self.stride - self.cursor).min(256) as usize;
                    if self.io(Command::Read(self.offset()? + self.cursor, n), scratch)? {
                        if scratch[..n].iter().any(|x| *x != 255) {
                            return Err(HistoryFailure::StoredIntegrity);
                        }
                        self.cursor += u32n(n)?;
                    }
                }
            }
            Phase::AppendProgram => {
                let end = self.stride - self.geometry.write_size;
                if self.cursor == end {
                    self.phase = Phase::AppendSync;
                } else {
                    if self.command.is_none() {
                        self.fill_append_page(scratch)?;
                    }
                    if self.io(Command::Program(self.offset()? + self.cursor), scratch)? {
                        self.cursor += self.geometry.write_size;
                    }
                }
            }
            Phase::AppendSync => {
                if self.io(Command::Sync, scratch)? {
                    self.phase = Phase::AppendMarker;
                }
            }
            Phase::AppendMarker => {
                if self.command.is_none() {
                    scratch[..self.geometry.write_size as usize].fill(0xa5);
                }
                if self.io(
                    Command::Program(self.offset()? + self.stride - self.geometry.write_size),
                    scratch,
                )? {
                    self.phase = Phase::AppendFinalSync;
                }
            }
            Phase::AppendFinalSync => {
                if self.io(Command::Sync, scratch)? {
                    self.cut = self.slot + 1;
                    self.phase = Phase::ScanMarker;
                }
            }
            Phase::MetadataCommit => self.step_metadata_commit(scratch)?,
        }
        Ok(())
    }
    fn step_metadata_recover(&mut self, scratch: &mut [u8; 256]) -> Result<(), HistoryFailure> {
        match self.snapshot.poll_recover_with_scratch(scratch) {
            Poll::Pending => {}
            Poll::Ready(result) => {
                result.map_err(failure)?;
                let format = Format::<D, R, B, N> {
                    config: self.config,
                    geometry: self.geometry,
                    stride: self.stride,
                    data: &self.data,
                };
                let decoded = self
                    .snapshot
                    .inspect_committed(|meta, bytes| -> Result<_, HistoryFailure> {
                        if meta.is_some() {
                            format.decode_into(bytes, &mut self.work)?;
                        }
                        Ok((meta, crc(&[bytes]), bytes.len()))
                    })
                    .map_err(|_| HistoryFailure::Storage)??;
                if let Some(meta) = decoded.0 {
                    if meta.revision == 0
                        || meta.revision < self.revision
                        || self.revision == meta.revision
                            && (decoded.1 != self.root || decoded.2 != self.metadata_length)
                    {
                        return Err(HistoryFailure::StoredIntegrity);
                    }
                    self.space(&self.work)?;
                    if self.revision != 0
                        && (self.work.dataset != self.data.dataset
                            || self.work.len != self.data.len
                            || self.work.domains[..self.work.len]
                                .iter()
                                .zip(&self.data.domains)
                                .any(|(a, b)| a.domain != b.domain || a.epoch != b.epoch))
                    {
                        return Err(HistoryFailure::StoredIntegrity);
                    }
                    self.metadata_revision = meta.revision;
                    self.metadata_root = decoded.1;
                    self.metadata_len = decoded.2;
                    self.d = 0;
                    self.phase = Phase::RecoverStart;
                } else {
                    if self.revision != 0 {
                        return Err(HistoryFailure::StoredIntegrity);
                    }
                    self.cursor = 0;
                    self.phase = Phase::Blank;
                }
            }
        }
        Ok(())
    }
    fn step_metadata_commit(&mut self, scratch: &mut [u8; 256]) -> Result<(), HistoryFailure> {
        let id = self.commit.ok_or(HistoryFailure::InvalidState)?;
        match self.snapshot.poll_commit_with_scratch(id, scratch) {
            Poll::Pending => {}
            Poll::Ready(result) => {
                let receipt = result.map_err(failure)?;
                if receipt.snapshot.revision != self.metadata_revision
                    || receipt.snapshot.length != self.metadata_len
                {
                    return Err(HistoryFailure::StoredIntegrity);
                }
                let good = self
                    .snapshot
                    .inspect_committed(|m, b| {
                        m == Some(receipt.snapshot) && crc(&[b]) == self.metadata_root
                    })
                    .map_err(|_| HistoryFailure::Storage)?;
                if !good {
                    return Err(HistoryFailure::StoredIntegrity);
                }
                self.data = self.work;
                self.revision = self.metadata_revision;
                self.root = self.metadata_root;
                self.metadata_length = self.metadata_len;
                self.poisoned = false;
                let result = match self.kind {
                    Kind::Initialize => JobResult::Initialized,
                    Kind::Prepare(Preparation::Ack { upto, .. }) => {
                        JobResult::Acknowledged { upto }
                    }
                    _ => return Err(HistoryFailure::InvalidState),
                };
                self.finish(result);
            }
        }

        Ok(())
    }
    // Consume one contiguous data-area page, including any body/padding boundary.
    // The existing scratch and record buffers suffice; marker verification stays separate.
    #[inline(never)]
    fn scan_chunk(&mut self, n: usize, scratch: &mut [u8; 256]) -> Result<(), HistoryFailure> {
        let start = self.cursor as usize;
        let first = start.max(80);
        let last = (start + n).min(80 + self.record.len);
        if first < last {
            let at = first - 80;
            let bytes = &scratch[first - start..last - start];
            if matches!(self.kind, Kind::Append) && self.record.body[at..at + bytes.len()] != *bytes
            {
                return Err(HistoryFailure::StoredIntegrity);
            }
            self.record.body[at..at + bytes.len()].copy_from_slice(bytes);
            self.log_crc.update(bytes);
            self.stored_crc.update(bytes);
        }
        let padding = (80 + self.record.len).saturating_sub(start).min(n);
        if scratch[padding..n].iter().any(|x| *x != 255) {
            return Err(HistoryFailure::StoredIntegrity);
        }
        self.cursor += u32n(n)?;
        Ok(())
    }
    #[inline(never)]
    fn accept_header(&mut self, n: usize, scratch: &mut [u8; 256]) -> Result<(), HistoryFailure> {
        if matches!(self.kind, Kind::Append)
            && self.format().header(&self.record, self.d, self.slot)? != scratch[..80]
        {
            return Err(HistoryFailure::StoredIntegrity);
        }
        self.parse_header(scratch)?;
        self.cursor = 0;
        self.scan_chunk(n, scratch)?;
        self.phase = Phase::ScanBody;
        Ok(())
    }
    #[inline(never)]
    fn fill_append_page(&self, scratch: &mut [u8; 256]) -> Result<(), HistoryFailure> {
        let unit = self.geometry.write_size as usize;
        scratch[..unit].fill(255);
        let header = self.format().header(&self.record, self.d, self.slot)?;
        for (i, byte) in scratch[..unit].iter_mut().enumerate() {
            let k = self.cursor as usize + i;
            if k < 80 {
                *byte = header[k];
            } else if k - 80 < self.record.len {
                *byte = self.record.body[k - 80];
            }
        }
        Ok(())
    }
    #[inline(never)]
    fn parse_header(&mut self, scratch: &[u8; 256]) -> Result<(), HistoryFailure> {
        let h = &scratch[..80];
        let data = if matches!(self.kind, Kind::Recover) {
            &self.work
        } else {
            &self.data
        };
        if &h[..8] != b"HPGREC01"
            || h[8..24] != data.dataset
            || h[24] != data.domains[self.d].domain
            || h[25..28] != [0; 3]
            || u32at(h, 28) != data.domains[self.d].epoch
            || u32at(h, 68) != u32n(self.d)? * self.config.slots_per_domain + self.slot
            || u32at(h, 76) != crc(&[&self.format().config_bytes()?])
        {
            return Err(HistoryFailure::StoredIntegrity);
        }
        let len = u16at(h, 54) as usize;
        if len > B {
            return Err(HistoryFailure::StoredIntegrity);
        }
        self.record.domain = h[24];
        self.record.epoch = u32at(h, 28);
        self.record.id = u64at(h, 32);
        self.record.kind = u16at(h, 40);
        self.record.schema = u16at(h, 42);
        self.record.utc = u64at(h, 44).cast_signed();
        self.record.flags = u16at(h, 52);
        self.record.len = len;
        self.record.parent = u64at(h, 56);
        self.record.crc = u32at(h, 64);
        self.expected_crc = u32at(h, 72);
        self.log_crc = Crc32c::new();
        self.log_crc.update(b"HPG1");
        self.log_crc.update(&h[..72]);
        self.log_crc.update(&h[76..80]);
        self.stored_crc = Crc32c::new();
        self.stored_crc.update(b"WREC");
        self.stored_crc.update(&1u16.to_le_bytes());
        self.stored_crc.update(&[h[24]]);
        self.stored_crc.update(&h[28..56]);
        Ok(())
    }
    fn verify(&self) -> Result<(), HistoryFailure> {
        let r = &self.record;
        if r.epoch == 0
            || r.id == 0
            || r.kind == 0
            || r.schema == 0
            || r.flags & !1 != 0
            || (r.flags & 1 == 0 && r.utc != 0)
            || self.expected_crc != self.log_crc.finish()
            || r.crc != self.stored_crc.finish()
        {
            return Err(HistoryFailure::StoredIntegrity);
        }
        self.schema
            .validate(r.stored(), r.parent)
            .map_err(|_| HistoryFailure::StoredIntegrity)
    }
    fn visit(&mut self) -> Result<(), HistoryFailure> {
        if matches!(self.kind, Kind::Recover) {
            return self.recover_visit();
        }

        if matches!(self.kind, Kind::Append) {
            if !self.present {
                return Err(HistoryFailure::StoredIntegrity);
            }
            let id = self.record.id;
            if id != self.data.domains[self.d].newest_id + 1 {
                return Err(HistoryFailure::StoredIntegrity);
            }
            self.data.domains[self.d].newest_id = id;
            self.data.count[self.d] += 1;
            self.data.tail[self.d] += 1;
            if self.data.domains[self.d].oldest_id == 0 {
                self.data.domains[self.d].oldest_id = id;
            }
            self.data.domains[self.d].newest_id = id;
            self.poisoned = false;
            self.finish(JobResult::Appended { id });
            return Ok(());
        }
        if !self.present {
            return Ok(());
        }
        if self.record.id <= self.high {
            return Err(HistoryFailure::StoredIntegrity);
        }
        self.high = self.record.id;
        let Kind::Prepare(request) = self.kind else {
            return Err(HistoryFailure::InvalidState);
        };
        let filter = match request {
            Preparation::Open { filter, .. }
            | Preparation::Count { filter }
            | Preparation::Ack { filter, .. } => filter,
            Preparation::Plan { .. } | Preparation::Record { .. } => {
                self.lease
                    .active()
                    .ok_or(HistoryFailure::CursorExpired)?
                    .filter
            }
        };
        if !matches(&self.record, filter) {
            return Ok(());
        }
        let id = self.record.id;
        if self.first == 0 {
            self.first = id;
        }
        self.last = id;
        match request {
            Preparation::Open { .. } => {
                if filter.filter_flags == 0 && id <= self.data.ack[self.d] {
                    self.covered += 1;
                }
            }
            Preparation::Plan { after_id, max, .. } => {
                if id > after_id {
                    self.remaining += 1;
                    if self.plan_count < max {
                        let Some(Ok(JobResult::Planned(plan))) = self.terminal.as_mut() else {
                            return Err(HistoryFailure::InvalidState);
                        };
                        plan.ids[self.plan_count as usize] = id;
                        self.plan_count += 1;
                    }
                }
            }
            Preparation::Ack { upto, .. } => {
                if id == upto {
                    self.found = Some(self.ordinal);
                }
            }
            Preparation::Record { lease, id: target } => {
                if id == target {
                    self.cache = Some((lease.handle, id, self.ordinal));
                    self.finish(JobResult::RecordReady);
                    return Ok(());
                }
            }
            Preparation::Count { .. } => {}
        }
        self.ordinal += 1;
        Ok(())
    }
    fn recover_visit(&mut self) -> Result<(), HistoryFailure> {
        if self.occupied {
            self.work.tail[self.d] = self.slot + 1;
        }
        if self.present {
            let r = &self.record;
            if r.id <= self.work.domains[self.d].newest_id || self.work.count[self.d] >= u32n(R)? {
                return Err(HistoryFailure::StoredIntegrity);
            }
            self.work.domains[self.d].newest_id = r.id;
            self.work.count[self.d] += 1;
            if self.work.domains[self.d].oldest_id == 0 {
                self.work.domains[self.d].oldest_id = r.id;
            }
            self.work.domains[self.d].newest_id = r.id;
            self.ack_found |= r.id == self.work.ack[self.d];
            let checkpoint_tail = self
                .snapshot
                .inspect_committed(|_, bytes| u32at(bytes, 80 + self.d * 64 + 28))
                .map_err(|_| HistoryFailure::Storage)?;
            if self.slot < checkpoint_tail {
                self.prefix_count += 1;
                if self.prefix_first == 0 {
                    self.prefix_first = r.id;
                }
                self.prefix_last = r.id;
            }
        }
        Ok(())
    }
    #[inline(never)]
    fn scan_done(&mut self) -> Result<(), HistoryFailure> {
        if matches!(self.kind, Kind::Recover) {
            return self.recover_scan_done();
        }

        let Kind::Prepare(request) = self.kind else {
            return Err(HistoryFailure::InvalidState);
        };
        if !matches!(
            request,
            Preparation::Open { .. } | Preparation::Count { .. }
        ) && self.ordinal
            != self
                .lease
                .active()
                .ok_or(HistoryFailure::CursorExpired)?
                .count
        {
            return Err(HistoryFailure::StoredIntegrity);
        }
        let result = match request {
            Preparation::Open { snapshot, filter } => {
                let now = self.observe(None)?;
                let handle = self.next_handle;
                self.next_handle = handle.checked_add(1).ok_or(HistoryFailure::Capacity)?;
                let public = StoreLease {
                    handle,
                    snapshot: HistorySnapshot {
                        snapshot,
                        epoch: self.data.domains[self.d].epoch,
                        first_id: self.first,
                        last_id: self.last,
                        count: u32n(self.ordinal)?,
                        lease_s: self.lease_s,
                    },
                    committed_id: self.data.ack[self.d],
                };
                let deadline = now
                    .checked_add(u64::from(self.lease_s) * 1000)
                    .ok_or(HistoryFailure::Clock)?;
                self.lease_generation = self.generation;
                self.lease = LeaseSlot::Active(Lease {
                    public,
                    filter,
                    cut: self.cut,
                    count: self.ordinal,
                    admitted: [0; 32],
                    covered: self.covered,
                    deadline,
                });
                JobResult::Opened(public)
            }
            Preparation::Count { .. } => JobResult::Counted {
                epoch: self.data.domains[self.d].epoch,
                count: u32n(self.ordinal)?,
            },
            Preparation::Plan { after_id, .. } => {
                let Some(Ok(JobResult::Planned(mut plan))) = self.terminal else {
                    return Err(HistoryFailure::InvalidState);
                };
                plan.count = self.plan_count;
                plan.end_after_id = if self.plan_count == 0 {
                    after_id
                } else {
                    plan.ids[self.plan_count as usize - 1]
                };
                plan.eof = self.remaining == self.plan_count as usize;
                JobResult::Planned(plan)
            }
            Preparation::Record { .. } => return Err(HistoryFailure::NotFound),
            Preparation::Ack { upto, .. } => {
                let index = self.found.ok_or(HistoryFailure::Invalid)?;
                if upto <= self.data.ack[self.d] {
                    JobResult::Acknowledged {
                        upto: self.data.ack[self.d],
                    }
                } else {
                    if index
                        >= self
                            .lease
                            .active()
                            .ok_or(HistoryFailure::CursorExpired)?
                            .covered
                    {
                        return Err(HistoryFailure::InvalidState);
                    }
                    self.phase = Phase::AckAuthorize;
                    return Ok(());
                }
            }
        };
        self.finish(result);
        Ok(())
    }
    fn recover_scan_done(&mut self) -> Result<(), HistoryFailure> {
        let d = self.d;
        let next = &self.work;
        let old = &self.data;
        let valid_checkpoint = self
            .snapshot
            .inspect_committed(|_, bytes| {
                let at = 80 + d * 64;
                u32at(bytes, at + 28) <= next.tail[d]
                    && self.prefix_count == u32at(bytes, at + 24)
                    && self.prefix_first == u64at(bytes, at + 32)
                    && self.prefix_last == u64at(bytes, at + 40)
                    && u64at(bytes, at + 8) == u64at(bytes, at + 40)
            })
            .map_err(|_| HistoryFailure::Storage)?;
        if !self.ack_found
            || next.ack[d] > next.domains[d].newest_id
            || !valid_checkpoint
            || self.revision != 0
                && (next.domains[d].newest_id < old.domains[d].newest_id
                    || next.ack[d] < old.ack[d]
                    || next.count[d] < old.count[d]
                    || next.tail[d] < old.tail[d])
        {
            return Err(HistoryFailure::StoredIntegrity);
        }
        self.d += 1;
        if self.d < self.work.len {
            self.phase = Phase::RecoverStart;
        } else {
            self.cursor = self.config.slots_per_domain * self.stride * u32n(self.work.len)?;
            self.phase = Phase::Extra;
        }
        Ok(())
    }
    fn authorize(
        &self,
        domain: u8,
        operation: HistoryOperation,
        access: &dyn HistoryAccess,
    ) -> Result<(), HistoryFailure> {
        if access.module_allowed()
            && access.allowed(self.data.dataset, None, operation)
            && access.allowed(self.data.dataset, Some(domain), operation)
        {
            Ok(())
        } else {
            Err(HistoryFailure::Denied)
        }
    }
    #[allow(
        clippy::inline_always,
        reason = "Share checked time temporaries with the lease validation frame"
    )]
    #[inline(always)]
    fn observe(&mut self, supplied: Option<u64>) -> Result<u64, HistoryFailure> {
        let actual = match self.clock.now_ms() {
            Ok(n) => n,
            Err(e) => {
                self.release();
                return Err(e);
            }
        };
        if self.actual_time.is_some_and(|old| actual < old) {
            self.release();
            return Err(HistoryFailure::Clock);
        }
        self.actual_time = Some(actual);
        if supplied
            .is_some_and(|now| now > actual || self.supplied_time.is_some_and(|old| now < old))
        {
            self.release();
            return Err(HistoryFailure::Clock);
        }
        if let Some(now) = supplied {
            self.supplied_time = Some(now);
        }
        Ok(actual)
    }
    fn bound(
        &mut self,
        public: &StoreLease,
        supplied: Option<u64>,
        access: Option<&dyn HistoryAccess>,
    ) -> Result<&Lease, HistoryFailure> {
        self.ready()?;
        let now = self.observe(supplied)?;
        let l = self.lease.active().ok_or(HistoryFailure::CursorExpired)?;
        if l.public != *public {
            return Err(HistoryFailure::Identity);
        }
        if now >= l.deadline {
            self.release();
            return Err(HistoryFailure::CursorExpired);
        }
        if let Some(access) = access {
            self.authorize(l.filter.domain, HistoryOperation::Read, access)?;
        }
        self.lease.active().ok_or(HistoryFailure::CursorExpired)
    }
    fn check_filter(
        &mut self,
        filter: HistoryFilter,
        access: &dyn HistoryAccess,
    ) -> Result<usize, HistoryFailure> {
        self.ready()?;
        filter.validate(true).map_err(|_| HistoryFailure::Invalid)?;
        self.authorize(filter.domain, HistoryOperation::Read, access)?;
        self.data.domain(filter.domain)
    }
    fn preflight(
        &mut self,
        request: &Preparation,
        now_ms: u64,
        access: &dyn HistoryAccess,
    ) -> Result<(usize, u32), HistoryFailure> {
        match request {
            Preparation::Open { snapshot, filter } => {
                let d = self.check_filter(*filter, access)?;
                let now = self.observe(Some(now_ms))?;
                if *snapshot == 0 {
                    return Err(HistoryFailure::Invalid);
                }
                if self.lease.active().is_some_and(|l| now < l.deadline) {
                    return Err(HistoryFailure::Busy);
                }
                self.next_handle
                    .checked_add(1)
                    .ok_or(HistoryFailure::Capacity)?;
                now.checked_add(u64::from(self.lease_s) * 1000)
                    .ok_or(HistoryFailure::Clock)?;
                if self.phase == Phase::Idle && self.cache.is_none() {
                    self.release();
                }
                Ok((d, self.data.tail[d]))
            }
            Preparation::Count { filter } => {
                let d = self.check_filter(*filter, access)?;
                Ok((d, self.data.tail[d]))
            }
            Preparation::Plan {
                lease,
                after_id,
                max,
            } => {
                let l = self.bound(lease, Some(now_ms), Some(access))?;
                if !(1..=16).contains(max) || *after_id > l.public.snapshot.last_id {
                    return Err(HistoryFailure::Invalid);
                }
                let (domain, cut) = (l.filter.domain, l.cut);
                Ok((self.data.domain(domain)?, cut))
            }
            Preparation::Record { lease, id } => {
                let l = self.bound(lease, None, Some(access))?;
                if *id == 0 || *id < l.public.snapshot.first_id || *id > l.public.snapshot.last_id {
                    return Err(HistoryFailure::Invalid);
                }
                let (domain, cut) = (l.filter.domain, l.cut);
                Ok((self.data.domain(domain)?, cut))
            }
            Preparation::Ack {
                lease,
                filter,
                epoch,
                upto,
            } => {
                let l = self.bound(lease, Some(now_ms), Some(access))?;
                let (leased_filter, covered, cut) = (l.filter, l.covered, l.cut);
                self.authorize(filter.domain, HistoryOperation::Acknowledge, access)?;
                if leased_filter != *filter
                    || filter.filter_flags != 0
                    || filter.start_utc_ms != 0
                    || filter.end_utc_ms != 0
                    || filter.record_type != 0
                    || filter.parent_session != 0
                {
                    return Err(HistoryFailure::Invalid);
                }
                if *epoch != lease.snapshot.epoch {
                    return Err(HistoryFailure::Conflict);
                }
                if *upto == 0 || *upto > lease.snapshot.last_id {
                    return Err(HistoryFailure::Invalid);
                }
                if *upto > self.data.ack[self.data.domain(filter.domain)?]
                    && self.cache.is_some_and(|(cached, id, index)| {
                        cached == lease.handle && id == *upto && index >= covered
                    })
                {
                    return Err(HistoryFailure::InvalidState);
                }
                Ok((self.data.domain(filter.domain)?, cut))
            }
        }
    }
}

#[cfg(test)]
#[path = "../../tests/async_paged_support/mod.rs"]
mod test_medium;
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::test_medium::*;
    use super::*;
    use device_history_service::RecordStore;
    #[test]
    fn test_should_bind_compact_cache_and_proof_to_the_complete_active_lease() {
        let meta = Control::new(8192, 256, 4096);
        let log = Control::new(8192, 256, 4096);
        let s: Store<1, 4, 16, 144> = owner(&meta, &log, Clock::new(), 5);
        initialize(
            &s,
            &meta,
            &log,
            &[DomainConfig {
                domain: 0,
                epoch: 8,
            }],
        );
        run(
            &s,
            s.begin_append(input(&[42; 16], 0)).unwrap(),
            &[&meta, &log],
        )
        .unwrap();
        let access = Access::yes();
        let mut adapter = s.adapter().unwrap();
        let f = filter(0);
        prepare(
            &s,
            &mut adapter,
            Preparation::Open {
                snapshot: 1,
                filter: f,
            },
            &access,
            &[&meta, &log],
        );
        let lease = adapter.open(1, f, 100, &access).unwrap();
        while !adapter.prefetch(lease, 1, &access).unwrap() {
            s.step().unwrap();
        }
        let mut forged = lease;
        forged.snapshot.epoch += 1;
        let mut bytes = [9; 16];
        assert_eq!(
            adapter.read(forged, 1, &mut bytes, &access),
            Err(HistoryFailure::Identity)
        );
        assert_eq!(bytes, [9; 16]);
        assert_eq!(
            adapter.record_admitted(forged, 1, &access),
            Err(HistoryFailure::Identity)
        );
        // Cancellation cannot discard another snapshot's cached record.
        assert_eq!(adapter.retire_record(forged, 1, &access), Ok(true));
        assert_eq!(
            adapter
                .read(lease, 1, &mut bytes, &access)
                .unwrap()
                .record_id,
            1
        );
        assert_eq!(bytes, [42; 16]);
        adapter.record_admitted(lease, 1, &access).unwrap();
        assert_eq!(
            adapter.record_admitted(forged, 1, &access),
            Err(HistoryFailure::Identity)
        );
        adapter.record_admitted(lease, 1, &access).unwrap();
        adapter.close(lease).unwrap();
        adapter.close(lease).unwrap();
        assert_eq!(adapter.close(forged), Err(HistoryFailure::Identity));
        prepare(
            &s,
            &mut adapter,
            Preparation::Open {
                snapshot: 2,
                filter: f,
            },
            &access,
            &[&meta, &log],
        );
        let next = adapter.open(2, f, 100, &access).unwrap();
        assert_ne!(next.handle, lease.handle);
        assert_eq!(adapter.close(lease), Err(HistoryFailure::Identity));
        adapter.close(next).unwrap();
        adapter.close(next).unwrap();
        assert_eq!(adapter.close(lease), Err(HistoryFailure::Identity));
    }
    #[test]
    fn test_should_authorize_ack_without_starting_snapshot_encoding_on_control_stack() {
        let meta = Control::new(8192, 256, 4096);
        let log = Control::new(8192, 256, 4096);
        let s: Store<1, 4, 16, 144> = owner(&meta, &log, Clock::new(), 5);
        initialize(
            &s,
            &meta,
            &log,
            &[DomainConfig {
                domain: 0,
                epoch: 8,
            }],
        );
        run(
            &s,
            s.begin_append(input(&[42; 16], 0)).unwrap(),
            &[&meta, &log],
        )
        .unwrap();
        let access = Access::yes();
        let mut adapter = s.adapter().unwrap();
        let f = filter(0);
        prepare(
            &s,
            &mut adapter,
            Preparation::Open {
                snapshot: 1,
                filter: f,
            },
            &access,
            &[&meta, &log],
        );
        let lease = adapter.open(1, f, 100, &access).unwrap();
        while !adapter.prefetch(lease, 1, &access).unwrap() {
            s.step().unwrap();
        }
        adapter.record_admitted(lease, 1, &access).unwrap();
        let request = Preparation::Ack {
            lease,
            filter: f,
            epoch: 8,
            upto: 1,
        };
        let token = s.begin_prepare(request, 100, &access).unwrap();
        for _ in 0..1000 {
            if s.inner.borrow().phase == Phase::AckAuthorize {
                break;
            }
            s.step().unwrap();
        }
        assert_eq!(s.inner.borrow().phase, Phase::AckAuthorize);
        let calls = meta.calls.get();
        assert_eq!(s.preparation_ready(token, 100, &access), Ok(false));
        assert_eq!(meta.calls.get(), calls);
        assert!(
            !s.inner.borrow().snapshot.is_busy(),
            "CONTROL must not retain the snapshot encoder frame"
        );
        s.step().unwrap();
        assert!(s.inner.borrow().snapshot.is_busy());
        s.detach(token).unwrap();
        for _ in 0..10000 {
            if s.inner.borrow().phase == Phase::Idle {
                break;
            }
            s.step().unwrap();
        }
        assert_eq!(s.inner.borrow().phase, Phase::Idle);
        assert_eq!(s.inner.borrow().data.ack[0], 1);
        drop(adapter);
        drop(s);
        meta.power_loss();
        log.power_loss();
        let restored: Store<1, 4, 16, 144> = owner(&meta, &log, Clock::new(), 5);
        assert_eq!(
            run(&restored, restored.begin_recover().unwrap(), &[&meta, &log]),
            Ok(JobResult::Recovered { initialized: true })
        );
        assert_eq!(restored.inner.borrow().data.ack[0], 1);
    }
}
