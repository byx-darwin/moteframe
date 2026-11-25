//! Persistent history command service and DATA admission scheduling; no physical backend.
#![no_std]
#![forbid(unsafe_code)]
mod joined;
use core::cell::{Cell, RefCell};
use device_protocol::{
    DataKind, DataLease, Frame, Header, HistoryDomain, HistoryDone, HistoryFilter, HistoryPage,
    HistoryRead, HistoryRecord, HistoryRequest, HistoryResponse, HistorySnapshot, Kind,
    LOGICAL_MAX, LaneError, ModuleEntry, SharedDataLane,
};
use device_services::{Reason, ServiceHandler, SessionError, Status};
use device_storage::{CommitState, RecordIntegrity, StoredRecord};
pub use joined::JoinedHistory;
// Largest CONTROL reply: directory header plus eight encoded domains. DATA
// records retain their independent LOGICAL_MAX buffer and negotiated capacity.
const CONTROL_REPLY_MAX: usize = 22 + 8 * HistoryDomain::LENGTH;
/// Independently granted dataset/domain operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryOperation {
    /// Dataset identity lookup.
    Identity,
    /// Connection-static domain directory.
    Enumerate,
    /// Snapshot, count and records.
    Read,
    /// Atomic persistent prefix acknowledgement.
    Acknowledge,
}
/// Trusted live role/dataset/domain permission; no request may grant it.
pub trait HistoryAccess {
    /// Initial module gate before decoding or disclosing metadata.
    fn module_allowed(&self) -> bool;
    /// None checks dataset scope; Some additionally binds a concrete domain.
    fn allowed(&self, dataset: [u8; 16], domain: Option<u8>, operation: HistoryOperation) -> bool;
}
/// Real backend or lifecycle failure, mapped to existing frozen statuses only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryFailure {
    /// Invalid fields, body schema or cursor range.
    Invalid,
    /// Incompatible stream/snapshot lifecycle.
    InvalidState,
    /// Trusted scope absent or revoked.
    Denied,
    /// Real retention or shared DATA lane is busy.
    Busy,
    /// Fixed capacity/identifier space exhausted.
    Capacity,
    /// Actual domain or record absent.
    NotFound,
    /// Actual persistent storage failed.
    Storage,
    /// Expected epoch or cumulative grant conflicts.
    Conflict,
    /// Snapshot range/epoch/lease expired.
    CursorExpired,
    /// Stored commit/integrity check failed.
    StoredIntegrity,
    /// Wrong stream or in-flight message identity.
    Identity,
    /// Actor/physical connection ended.
    Closed,
    /// Trusted monotonic time regressed/overflowed.
    Clock,
    /// Cancel drain exceeded its fixed10-second deadline.
    Timeout,
    /// Ordered queue/notification prerequisite is unavailable.
    TransportUnavailable,
}
/// Actual immutable directory view pinned for this connection.
pub struct Directory<'a> {
    /// Persistent dataset identity, independent of radio address.
    pub dataset: [u8; 16],
    /// Sorted unique static domain entries.
    pub domains: &'a [HistoryDomain],
}
impl core::fmt::Debug for Directory<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Directory")
            .field("length", &self.domains.len())
            .finish_non_exhaustive()
    }
}
/// Real retention lease, bound to the requested filter and fixed upper range.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct StoreLease {
    /// Opaque nonzero backend handle; never a phone-granted identity.
    pub handle: u64,
    /// Actual frozen OPEN result, including granted lease duration.
    pub snapshot: HistorySnapshot,
    /// Actual durable domain watermark at opening.
    pub committed_id: u64,
}
impl core::fmt::Debug for StoreLease {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("StoreLease").finish_non_exhaustive()
    }
}
/// Actual immutable eligible IDs planned for one bounded READ.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ReadPlan {
    /// Sorted actual eligible IDs; only the first count are initialized.
    pub ids: [u64; 16],
    /// Count0..16, no more than requested maximum.
    pub count: u16,
    /// Last planned record, or unchanged after cursor for empty EOF.
    pub end_after_id: u64,
    /// True when no further eligible records remain in the lease.
    pub eof: bool,
}
impl core::fmt::Debug for ReadPlan {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ReadPlan")
            .field("length", &self.count)
            .finish_non_exhaustive()
    }
}
/// Actual canonical stored metadata for bytes copied into caller-owned scratch.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RecordContent {
    /// Original domain.
    pub domain: u8,
    /// Actual persistent epoch.
    pub epoch: u32,
    /// Actual persistent record identity.
    pub record_id: u64,
    /// Domain-owned nonzero record type.
    pub record_type: u16,
    /// Actual nonzero body schema.
    pub schema: u16,
    /// UTC milliseconds; zero when timestamp is invalid.
    pub utc_ms: i64,
    /// Timestamp-valid bit0; other bits forbidden.
    pub flags: u16,
    /// Exact copied body length.
    pub length: usize,
    /// Trusted actual storage commit state.
    pub commit: CommitState,
    /// Stored WREC CRC, checked before constructing any event.
    pub stored_crc: u32,
}
impl core::fmt::Debug for RecordContent {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RecordContent")
            .field("length", &self.length)
            .finish_non_exhaustive()
    }
}
/// Typed storage prerequisite; contains the exact logical operation and lease.
/// Readiness does not reserve a DATA identifier or grant transmission credit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preparation {
    /// Frozen snapshot scan.
    Open {
        /// Actor-reserved snapshot identity.
        snapshot: u32,
        /// Canonical complete filter.
        filter: HistoryFilter,
    },
    /// Bounded actual ID plan.
    Plan {
        /// Full actual lease identity.
        lease: StoreLease,
        /// Exclusive ID cursor.
        after_id: u64,
        /// Requested bounded count, 1 through 16.
        max: u16,
    },
    /// Actual filtered count.
    Count {
        /// Canonical complete filter.
        filter: HistoryFilter,
    },
    /// Durable acknowledgement after complete-prefix proof.
    Ack {
        /// Full actual lease identity.
        lease: StoreLease,
        /// Exact unfiltered OPEN filter.
        filter: HistoryFilter,
        /// Expected persistent domain epoch.
        epoch: u32,
        /// Inclusive actual admitted prefix endpoint.
        upto: u64,
    },
    /// Complete verified body prefetch, outside the FIFO reservation.
    Record {
        /// Full actual lease identity.
        lease: StoreLease,
        /// Actual record identity inside the frozen lease.
        id: u64,
    },
}
/// Mandatory real logical `RecordStore`; no production memory implementation exists.
pub trait RecordStore {
    /// Open into caller-owned workspace, avoiding large return-value temporaries.
    /// The destination is usable only after `Ok(())` and must then contain a lease.
    /// # Errors
    /// Same admission, retention and scope failures as the legacy `open` entry.
    fn open_into(
        &mut self,
        snapshot: u32,
        filter: HistoryFilter,
        now_ms: u64,
        access: &dyn HistoryAccess,
        out: &mut Option<StoreLease>,
    ) -> Result<(), HistoryFailure> {
        *out = Some(self.open(snapshot, filter, now_ms, access)?);
        Ok(())
    }
    /// Copy the exact prepared plan into caller-owned workspace, avoiding large
    /// return-value staging. The legacy `plan` entry remains available.
    /// # Errors
    /// Same real lease, cursor, scope and backend failures as `plan`.
    #[allow(clippy::too_many_arguments)] // Existing plan scope plus explicit destination.
    fn plan_into(
        &mut self,
        lease: StoreLease,
        after_id: u64,
        max: u16,
        now_ms: u64,
        access: &dyn HistoryAccess,
        out: &mut ReadPlan,
    ) -> Result<(), HistoryFailure> {
        *out = self.plan(lease, after_id, max, now_ms, access)?;
        Ok(())
    }
    /// Borrow one prerequisite, avoiding transient copies in asynchronous adapters.
    /// Legacy stores retain their existing by-value `prepare` behavior.
    /// # Errors
    /// Same admission/lifecycle failures as `prepare`; the request remains immutable.
    fn prepare_ref(
        &mut self,
        request: &Preparation,
        now_ms: u64,
        access: &dyn HistoryAccess,
    ) -> Result<bool, HistoryFailure> {
        self.prepare(*request, now_ms, access)
    }
    /// Admit/inspect an exact typed prerequisite without synchronous I/O.
    /// Existing synchronous stores are ready immediately.
    /// # Errors
    /// Real admission, lifecycle, validation and backend failures are explicit.
    fn prepare(
        &mut self,
        _request: Preparation,
        _now_ms: u64,
        _access: &dyn HistoryAccess,
    ) -> Result<bool, HistoryFailure> {
        Ok(true)
    }
    /// Prefetch the complete next body without consuming DATA credit or message IDs.
    /// # Errors
    /// Real lease, scope and storage failures are explicit.
    fn prefetch(
        &self,
        _lease: StoreLease,
        _id: u64,
        _access: &dyn HistoryAccess,
    ) -> Result<bool, HistoryFailure> {
        Ok(true)
    }
    /// Retire only an unstarted body's exact typed prefetch, preserving its lease/proof.
    /// Return false while an accepted physical read still needs worker draining.
    /// The actor calls this only when no DATA flight needs final admission.
    /// Existing synchronous stores have no pending prefetch and are ready immediately.
    /// # Errors
    /// Matching terminal failures and incorrect preparation identity remain explicit.
    fn retire_record(
        &mut self,
        _lease: StoreLease,
        _id: u64,
        _access: &dyn HistoryAccess,
    ) -> Result<bool, HistoryFailure> {
        Ok(true)
    }
    /// Detach connection delivery while accepted durable work keeps running.
    fn detach_preparation(&mut self) {}
    /// Return the actual connection-pinned dataset/domain directory.
    /// Identity, epoch or directory changes require terminating the old session.
    /// # Errors
    /// Actual persistent I/O or unavailable directory must be explicit.
    fn directory(&self) -> Result<Directory<'_>, HistoryFailure>;
    /// Pin actual eligible records and fixed upper bound for the full granted lease.
    /// No capacity guarantee means Busy, never a synthesized successful snapshot.
    /// # Errors
    /// Actual authorization/filter/retention/storage failures.
    fn open(
        &mut self,
        snapshot: u32,
        filter: HistoryFilter,
        now_ms: u64,
        access: &dyn HistoryAccess,
    ) -> Result<StoreLease, HistoryFailure>;
    /// Actual consistent plan of the next eligible IDs, ordered by ID rather than UTC.
    /// Must preserve the fixed upper bound and reject expired/stale leases.
    /// # Errors
    /// Actual cursor, epoch, retention or scope failure.
    fn plan(
        &mut self,
        lease: StoreLease,
        after_id: u64,
        max: u16,
        now_ms: u64,
        access: &dyn HistoryAccess,
    ) -> Result<ReadPlan, HistoryFailure>;
    /// Copy the entire actual stored body or return capacity; never silently truncate.
    /// # Errors
    /// Actual record, lease, scope or storage failure.
    fn read(
        &self,
        lease: StoreLease,
        id: u64,
        out: &mut [u8],
        access: &dyn HistoryAccess,
    ) -> Result<RecordContent, HistoryFailure>;
    /// Validate the real domain-specific body type/schema after storage integrity.
    /// # Errors
    /// Unknown or invalid actual schema is explicit, never permissive.
    fn validate_record(
        &self,
        lease: StoreLease,
        record: StoredRecord<'_>,
    ) -> Result<(), HistoryFailure>;
    /// Register one whole event's actual final-fragment queue admission.
    /// This is ephemeral sent-prefix evidence, never proof of phone persistence.
    /// Valid query cursors may omit earlier records; track each actual admission.
    /// Only a complete actual prefix grants new ACK coverage. Numeric ID gaps
    /// differ from unsent retained records; duplicate admissions are idempotent.
    /// # Errors
    /// Actual proof/scope/backend failure; never invent no-omission coverage.
    fn record_admitted(
        &mut self,
        lease: StoreLease,
        id: u64,
        access: &dyn HistoryAccess,
    ) -> Result<(), HistoryFailure>;
    /// Atomically recheck live dataset/domain scope, epoch, all filter bits, current
    /// durable watermark and actual no-omission admitted prefix, then persist ACK.
    /// Peer must verify CRC and commit its database before making this request.
    /// Existing watermark idempotence is permitted; queue admission alone never ACKs.
    /// # Errors
    /// Validation/scope failures preserve the watermark. An uncertain storage
    /// failure must block successful access until recovery; query the recovered
    /// watermark before retrying, because the complete new ACK may be durable.
    fn ack_atomic(
        &mut self,
        lease: StoreLease,
        filter: HistoryFilter,
        epoch: u32,
        upto: u64,
        now_ms: u64,
        access: &dyn HistoryAccess,
    ) -> Result<u64, HistoryFailure>;
    /// Actual matching count without creating an ACK proof or opening a lease.
    /// # Errors
    /// Actual scope/filter/epoch/storage failure.
    fn count(
        &self,
        filter: HistoryFilter,
        access: &dyn HistoryAccess,
    ) -> Result<(u32, u32), HistoryFailure>;
    /// Renew an actually successful READ/ACK lease using trusted monotonic time.
    /// # Errors
    /// Failed retention extension must not be reported as a successful renewal.
    fn renew(
        &mut self,
        lease: StoreLease,
        now_ms: u64,
        access: &dyn HistoryAccess,
    ) -> Result<(), HistoryFailure>;
    /// Release the actual retention/proof handle after drain, without an implicit ACK.
    /// # Errors
    /// Actual cleanup failure remains explicit.
    fn close(&mut self, lease: StoreLease) -> Result<(), HistoryFailure>;
}
impl<T: RecordStore + ?Sized> RecordStore for &mut T {
    fn open_into(
        &mut self,
        snapshot: u32,
        filter: HistoryFilter,
        now_ms: u64,
        access: &dyn HistoryAccess,
        out: &mut Option<StoreLease>,
    ) -> Result<(), HistoryFailure> {
        (**self).open_into(snapshot, filter, now_ms, access, out)
    }
    fn plan_into(
        &mut self,
        lease: StoreLease,
        after_id: u64,
        max: u16,
        now_ms: u64,
        access: &dyn HistoryAccess,
        out: &mut ReadPlan,
    ) -> Result<(), HistoryFailure> {
        (**self).plan_into(lease, after_id, max, now_ms, access, out)
    }
    fn prepare_ref(
        &mut self,
        request: &Preparation,
        now_ms: u64,
        access: &dyn HistoryAccess,
    ) -> Result<bool, HistoryFailure> {
        (**self).prepare_ref(request, now_ms, access)
    }
    fn prepare(
        &mut self,
        r: Preparation,
        n: u64,
        a: &dyn HistoryAccess,
    ) -> Result<bool, HistoryFailure> {
        (**self).prepare(r, n, a)
    }
    fn prefetch(
        &self,
        l: StoreLease,
        id: u64,
        a: &dyn HistoryAccess,
    ) -> Result<bool, HistoryFailure> {
        (**self).prefetch(l, id, a)
    }
    fn retire_record(
        &mut self,
        lease: StoreLease,
        id: u64,
        access: &dyn HistoryAccess,
    ) -> Result<bool, HistoryFailure> {
        (**self).retire_record(lease, id, access)
    }
    fn detach_preparation(&mut self) {
        (**self).detach_preparation();
    }
    fn directory(&self) -> Result<Directory<'_>, HistoryFailure> {
        (**self).directory()
    }
    fn open(
        &mut self,
        s: u32,
        f: HistoryFilter,
        n: u64,
        a: &dyn HistoryAccess,
    ) -> Result<StoreLease, HistoryFailure> {
        (**self).open(s, f, n, a)
    }
    fn plan(
        &mut self,
        l: StoreLease,
        id: u64,
        m: u16,
        n: u64,
        a: &dyn HistoryAccess,
    ) -> Result<ReadPlan, HistoryFailure> {
        (**self).plan(l, id, m, n, a)
    }
    fn read(
        &self,
        l: StoreLease,
        id: u64,
        o: &mut [u8],
        a: &dyn HistoryAccess,
    ) -> Result<RecordContent, HistoryFailure> {
        (**self).read(l, id, o, a)
    }
    fn validate_record(&self, l: StoreLease, r: StoredRecord<'_>) -> Result<(), HistoryFailure> {
        (**self).validate_record(l, r)
    }
    fn record_admitted(
        &mut self,
        l: StoreLease,
        id: u64,
        a: &dyn HistoryAccess,
    ) -> Result<(), HistoryFailure> {
        (**self).record_admitted(l, id, a)
    }
    fn ack_atomic(
        &mut self,
        lease: StoreLease,
        filter: HistoryFilter,
        epoch: u32,
        upto: u64,
        now_ms: u64,
        access: &dyn HistoryAccess,
    ) -> Result<u64, HistoryFailure> {
        (**self).ack_atomic(lease, filter, epoch, upto, now_ms, access)
    }
    fn count(&self, f: HistoryFilter, a: &dyn HistoryAccess) -> Result<(u32, u32), HistoryFailure> {
        (**self).count(f, a)
    }
    fn renew(
        &mut self,
        l: StoreLease,
        n: u64,
        a: &dyn HistoryAccess,
    ) -> Result<(), HistoryFailure> {
        (**self).renew(l, n, a)
    }
    fn close(&mut self, l: StoreLease) -> Result<(), HistoryFailure> {
        (**self).close(l)
    }
}
/// Side-effect-free next-event metadata; no credit or message ID is consumed by this query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryPendingEvent {
    /// A complete record may be prepared only inside an actual first-frame FIFO reservation.
    Record {
        /// Current connection-nonreused stream identity.
        stream_id: u16,
    },
    /// The fixed control DONE is ready; release occurs only after its complete FIFO admission.
    Done {
        /// Current stream to release after actual DONE admission.
        stream_id: u16,
    },
}
/// Complete pinned logical DATA event metadata, excluding the transport header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataEvent {
    /// Nonzero external connection-owner message ID.
    pub message_id: u16,
    /// Exact RECORD logical body length including `C_event`.
    pub length: usize,
}
/// Invalid immutable service integration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigError {
    /// Module/capacity/static directory mismatch.
    Invalid,
    /// Actual ordered DATA transport was not supplied.
    Transport,
    /// Actual backend failed while creating its pinned directory.
    Backend(HistoryFailure),
}
#[derive(Clone, Copy)]
struct Opened {
    lease: StoreLease,
    filter: HistoryFilter,
    expires: u64,
    last_message: u16,
}
#[derive(Clone, Copy)]
struct Flight {
    event: DataEvent,
    record_id: u64,
}
#[derive(Clone, Copy)]
struct Batch {
    lane: DataLease,
    ready: bool,
    granted: u32,
    used: u32,
    sent: u16,
    flight: Option<Flight>,
    done: Option<HistoryDone>,
    cancelled: bool,
}
#[derive(Clone, Copy)]
struct Cancelling {
    request: HistoryRequest,
    message: u16,
    deadline: u64,
}
/// Existing borrowed-store actor, retaining its original layout and constructor.
pub type HistoryHandler<'a, R, const D: usize, const P: usize> = HistoryActor<'a, &'a mut R, D, P>;
/// Actor that owns a small prepared adapter by value; durable work stays in its boot owner.
pub type OwnedHistoryHandler<'a, R, const D: usize, const P: usize> = HistoryActor<'a, R, D, P>;
/// Connection-owned history service; register exactly one module10 handler per connection.
/// The shared lane must be the same owner used by all other history/raw adapters.
pub struct HistoryActor<'a, R: RecordStore, const D: usize, const P: usize> {
    store: R,
    access: &'a dyn HistoryAccess,
    lane: &'a SharedDataLane,
    metadata: ModuleEntry,
    dataset_id: [u8; 16],
    domains: [HistoryDomain; D],
    domain_count: usize,
    data_max: usize,
    opened: Cell<Option<Opened>>,
    batch: Cell<Option<Batch>>,
    // Immutable while batch is present. Keep the ID plan/request out of each
    // copied progress snapshot; publishing a new batch installs both first.
    batch_plan: ReadPlan,
    batch_request: Option<(u16, HistoryRequest)>,
    cancelling: Cell<Option<Cancelling>>,
    now: Cell<u64>,
    next_snapshot: u32,
    prepared_ready: Cell<bool>,
    prepared_header: Option<Header>,
    prepared_body: [u8; 28],
    prepared_length: usize,
    closed: Cell<bool>,
    event: RefCell<[u8; LOGICAL_MAX]>,
}
impl<R: RecordStore, const D: usize, const P: usize> core::fmt::Debug
    for HistoryActor<'_, R, D, P>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HistoryHandler")
            .field("data_length_max", &self.data_max)
            .field("payload_length_max", &P)
            .finish_non_exhaustive()
    }
}
impl<'a, R: RecordStore, const D: usize, const P: usize> HistoryActor<'a, R, D, P> {
    /// Bind an actual directory/store and negotiated DATA/FIFO guarantees.
    /// # Errors
    /// Reject malformed capacities/catalogues or unavailable real transport.
    pub fn new(
        metadata: ModuleEntry,
        store: R,
        access: &'a dyn HistoryAccess,
        lane: &'a SharedDataLane,
        data_max: usize,
        ordered_queue: bool,
    ) -> Result<Self, ConfigError> {
        if !ordered_queue {
            return Err(ConfigError::Transport);
        }
        if metadata.module != 0x10
            || (metadata.major, metadata.minor) != (1, 2)
            || metadata.availability != 1
            || metadata.features != 0
            || !(128..=512).contains(&metadata.input_max)
            || !(128..=512).contains(&metadata.output_max)
            || !(128..=512).contains(&data_max)
            || D == 0
            || D > 255
            || P == 0
            || P > 474
        {
            return Err(ConfigError::Invalid);
        }
        let directory = store.directory().map_err(ConfigError::Backend)?;
        if directory.dataset == [0; 16] || directory.domains.len() > D {
            return Err(ConfigError::Invalid);
        }
        let mut domains = [HistoryDomain {
            domain: 0,
            epoch: 0,
            oldest_id: 0,
            newest_id: 0,
        }; D];
        let mut previous = None;
        for (index, domain) in directory.domains.iter().enumerate() {
            let mut bytes = [0; 21];
            if domain.encode(&mut bytes).is_err()
                || previous.is_some_and(|old| old >= domain.domain)
            {
                return Err(ConfigError::Invalid);
            }
            domains[index] = *domain;
            previous = Some(domain.domain);
        }
        let dataset_id = directory.dataset;
        let domain_count = directory.domains.len();
        Ok(Self {
            store,
            access,
            lane,
            metadata,
            dataset_id,
            domains,
            domain_count,
            data_max,
            opened: Cell::new(None),
            batch: Cell::new(None),
            batch_plan: ReadPlan {
                ids: [0; 16],
                count: 0,
                end_after_id: 0,
                eof: true,
            },
            batch_request: None,
            cancelling: Cell::new(None),
            now: Cell::new(0),
            next_snapshot: 1,
            prepared_ready: Cell::new(false),
            prepared_header: None,
            prepared_body: [0; 28],
            prepared_length: 0,
            closed: Cell::new(false),
            event: RefCell::new([0; LOGICAL_MAX]),
        })
    }
    fn invalidate(&self) {
        self.closed.set(true);
        self.batch.set(None);
        self.cancelling.set(None);
        if let Ok(mut event) = self.event.try_borrow_mut() {
            event.fill(0);
        }
        let _ = self.lane.with_mut(|owner| {
            owner.disconnect();
            Ok(())
        });
    }
    fn allowed(&self, domain: Option<u8>, operation: HistoryOperation) -> bool {
        self.access.module_allowed()
            && self.access.allowed(self.dataset_id, None, operation)
            && domain.is_none_or(|domain| {
                self.access
                    .allowed(self.dataset_id, Some(domain), operation)
            })
    }
    fn alive(&self) -> Result<(), HistoryFailure> {
        if self.closed.get() {
            return Err(HistoryFailure::Closed);
        }
        let actual = match self.store.directory() {
            Ok(actual) => actual,
            Err(error) => {
                self.invalidate();
                return Err(error);
            }
        };
        if actual.dataset != self.dataset_id || actual.domains != &self.domains[..self.domain_count]
        {
            self.invalidate();
            return Err(HistoryFailure::CursorExpired);
        }
        if self
            .opened
            .get()
            .is_some_and(|opened| !self.allowed(Some(opened.filter.domain), HistoryOperation::Read))
        {
            self.invalidate();
            return Err(HistoryFailure::Denied);
        }
        Ok(())
    }
    /// Advance trusted time before requests and while waiting for DATA drain.
    /// # Errors
    /// Regressed clock or ten-second cancellation deadline permanently ends the connection.
    pub fn advance(&mut self, now_ms: u64) -> Result<(), HistoryFailure> {
        if self.closed.get() {
            return Err(HistoryFailure::Closed);
        }
        if now_ms < self.now.get() {
            self.invalidate();
            return Err(HistoryFailure::Clock);
        }
        self.now.set(now_ms);
        if self
            .cancelling
            .get()
            .is_some_and(|pending| now_ms >= pending.deadline)
        {
            self.invalidate();
            return Err(HistoryFailure::Timeout);
        }
        self.alive()
    }
    /// Return the actual pinned dataset identity after current identity permission.
    /// # Errors
    /// Missing scope, changed identity or actual backend failure is explicit.
    pub fn dataset(&self) -> Result<[u8; 16], HistoryFailure> {
        self.alive()?;
        if !self.allowed(None, HistoryOperation::Identity) {
            return Err(HistoryFailure::Denied);
        }
        Ok(self.dataset_id)
    }
    /// Side-effect-free live history grant validation for a trusted core bridge.
    /// # Errors
    /// Reject wrong kind/generation, unfinished READ admission, scope, lease or decreasing credit.
    pub fn validate_grant(
        &self,
        kind: DataKind,
        stream: u16,
        cumulative: u32,
    ) -> Result<(), HistoryFailure> {
        if kind != DataKind::History {
            return Err(HistoryFailure::Identity);
        }
        let batch = self.grant_binding(stream)?;
        if batch.cancelled || batch.done.is_some() {
            return Err(HistoryFailure::InvalidState);
        }
        if cumulative < batch.granted {
            return Err(HistoryFailure::Conflict);
        }
        Ok(())
    }
    /// Whether a prior accepted response still belongs to the current owned stream.
    /// Normal DONE may be pending, but release/cancel completion invalidates old success.
    #[must_use]
    pub fn grant_replay_valid(&self, stream: u16, accepted_cumulative: u32) -> bool {
        self.grant_binding(stream)
            .is_ok_and(|batch| accepted_cumulative <= batch.granted)
    }
    /// Pure permission query; stream existence, credit and backend readiness are separate.
    #[must_use]
    pub fn grant_authorized(&self) -> bool {
        self.allowed(
            self.opened.get().map(|opened| opened.filter.domain),
            HistoryOperation::Read,
        )
    }
    /// Pure live Identity scope query, independent of backend readiness or streams.
    #[must_use]
    pub fn dataset_authorized(&self) -> bool {
        self.allowed(None, HistoryOperation::Identity)
    }
    fn grant_binding(&self, stream: u16) -> Result<Batch, HistoryFailure> {
        if self.closed.get() {
            return Err(HistoryFailure::Closed);
        }
        if !self.grant_authorized() {
            return Err(HistoryFailure::Denied);
        }
        let actual = self.store.directory()?;
        if actual.dataset != self.dataset_id || actual.domains != &self.domains[..self.domain_count]
        {
            return Err(HistoryFailure::CursorExpired);
        }
        let opened = self.opened.get().ok_or(HistoryFailure::InvalidState)?;
        if self.now.get() >= opened.expires {
            return Err(HistoryFailure::CursorExpired);
        }
        let batch = self.batch.get().ok_or(HistoryFailure::InvalidState)?;
        if stream != batch.lane.stream_id() {
            return Err(HistoryFailure::Identity);
        }
        if !batch.ready {
            return Err(HistoryFailure::InvalidState);
        }
        if !self
            .lane
            .with_mut(|owner| Ok(owner.owns(batch.lane)))
            .map_err(lane_failure)?
        {
            return Err(HistoryFailure::InvalidState);
        }
        Ok(batch)
    }
    /// Cumulative credit for the current history stream, never an incremental grant.
    /// # Errors
    /// Reject wrong kind/stream, decreasing totals, closed batch or revoked scope.
    pub fn grant(
        &mut self,
        kind: DataKind,
        id: u16,
        cumulative: u32,
    ) -> Result<(), HistoryFailure> {
        self.alive()?;
        self.validate_grant(kind, id, cumulative)?;
        let mut batch = self.batch.get().ok_or(HistoryFailure::InvalidState)?;
        batch.granted = cumulative;
        self.batch.set(Some(batch));
        Ok(())
    }
    fn snapshot(&self, id: u32) -> Result<Opened, HistoryFailure> {
        let opened = self
            .opened
            .get()
            .filter(|opened| opened.lease.snapshot.snapshot == id)
            .ok_or(HistoryFailure::CursorExpired)?;
        if self.now.get() >= opened.expires {
            return Err(HistoryFailure::CursorExpired);
        }
        Ok(opened)
    }
    fn renew(&mut self, snapshot: u32) -> Result<(), HistoryFailure> {
        let opened = self
            .opened
            .get_mut()
            .as_ref()
            .ok_or(HistoryFailure::CursorExpired)?;
        if opened.lease.snapshot.snapshot != snapshot || self.now.get() >= opened.expires {
            return Err(HistoryFailure::CursorExpired);
        }
        let expires = self
            .now
            .get()
            .checked_add(u64::from(opened.lease.snapshot.lease_s) * 1000)
            .ok_or(HistoryFailure::Clock)?;
        self.store
            .renew(opened.lease, self.now.get(), self.access)?;
        self.alive()?;
        self.opened
            .get_mut()
            .as_mut()
            .ok_or(HistoryFailure::CursorExpired)?
            .expires = expires;
        Ok(())
    }
    // Error/DONE bookkeeping does not share the successful prefetch scratch.
    #[inline(never)]
    fn mark_done(&self, status: Status, reason: Reason) {
        if let Some(mut batch) = self.batch.get()
            && !batch.cancelled
        {
            batch.done = Some(HistoryDone {
                stream_id: batch.lane.stream_id(),
                sent_count: batch.sent,
                last_id: if batch.sent == 0 {
                    0
                } else {
                    self.batch_plan.ids[usize::from(batch.sent) - 1]
                },
                status: status as u16,
                reason: reason as u16,
            });
            self.batch.set(Some(batch));
        }
    }
    fn copy_flight(&self, flight: Flight, out: &mut [u8]) -> Result<DataEvent, HistoryFailure> {
        if out.len() < flight.event.length {
            return Err(HistoryFailure::Capacity);
        }
        let event = self.event.try_borrow().map_err(|_| HistoryFailure::Busy)?;
        out[..flight.event.length].copy_from_slice(&event[..flight.event.length]);
        Ok(flight.event)
    }
    /// Admit/inspect the next complete body outside the FIFO reservation.
    /// This never consumes a DATA message ID, transmission credit or ACK proof.
    /// # Errors
    /// Actual scope, lease, identity or storage failures remain explicit.
    pub fn prepare_next_data(&self) -> Result<bool, HistoryFailure> {
        self.alive()?;
        let Some(batch) = self.batch.get() else {
            return Ok(false);
        };
        if !batch.ready
            || batch.cancelled
            || batch.done.is_some()
            || batch.flight.is_some()
            || batch.sent >= self.batch_plan.count
            || batch.used >= batch.granted
        {
            return Ok(false);
        }
        let opened = self.opened.get().ok_or(HistoryFailure::InvalidState)?;
        match self.store.prefetch(
            opened.lease,
            self.batch_plan.ids[usize::from(batch.sent)],
            self.access,
        ) {
            Ok(ready) => Ok(ready),
            Err(HistoryFailure::Busy) => Ok(false),
            Err(HistoryFailure::Denied | HistoryFailure::Closed) => {
                self.invalidate();
                Err(HistoryFailure::Denied)
            }
            Err(error) => {
                let (status, reason) = wire(error);
                self.mark_done(status, reason);
                Ok(false)
            }
        }
    }
    /// Inspect readiness and schedule typed prefetch outside the first-frame FIFO reservation.
    /// This never reserves credit or consumes a DATA identifier.
    /// # Errors
    /// Actual scope, lease, identity, storage or shared-owner access failure remains explicit.
    pub fn pending_event(&self) -> Result<Option<HistoryPendingEvent>, HistoryFailure> {
        self.pending_event_inner::<true>()
    }
    /// Recheck routing eligibility without scheduling another storage preparation.
    /// Actual record readiness and authority are rechecked by `stage_data`.
    /// # Errors
    /// Same live lease, scope and identity checks as `pending_event`.
    pub fn pending_event_without_prefetch(
        &self,
    ) -> Result<Option<HistoryPendingEvent>, HistoryFailure> {
        self.pending_event_inner::<false>()
    }
    fn pending_event_inner<const PREFETCH: bool>(
        &self,
    ) -> Result<Option<HistoryPendingEvent>, HistoryFailure> {
        let Some(batch) = self.pending_batch()? else {
            return Ok(None);
        };
        let stream_id = batch.lane.stream_id();
        if batch.flight.is_some() {
            return Ok(Some(HistoryPendingEvent::Record { stream_id }));
        }
        if !batch.ready || batch.cancelled {
            return Ok(None);
        }
        if batch.done.is_some() {
            return Ok(Some(HistoryPendingEvent::Done { stream_id }));
        }
        if batch.sent < self.batch_plan.count && batch.used < batch.granted {
            if PREFETCH && !self.prepare_next_data()? {
                return Ok(None);
            }
            return Ok(Some(HistoryPendingEvent::Record { stream_id }));
        }
        Ok(None)
    }
    fn pending_batch(&self) -> Result<Option<Batch>, HistoryFailure> {
        self.alive()?;
        let Some(batch) = self.batch.get() else {
            return Ok(None);
        };
        let opened = self.opened.get().ok_or(HistoryFailure::InvalidState)?;
        if self.now.get() >= opened.expires {
            return Err(HistoryFailure::CursorExpired);
        }
        if !self
            .lane
            .with_mut(|owner| Ok(owner.owns(batch.lane)))
            .map_err(lane_failure)?
        {
            return Err(HistoryFailure::InvalidState);
        }
        Ok(Some(batch))
    }
    /// Verify an exact retained event's current scope without preparing another body.
    #[must_use]
    pub fn event_valid(&self, kind: HistoryPendingEvent, message_id: Option<u16>) -> bool {
        let Ok(Some(batch)) = self.pending_batch() else {
            return false;
        };
        match kind {
            HistoryPendingEvent::Record { stream_id } => {
                batch.lane.stream_id() == stream_id
                    && batch
                        .flight
                        .is_some_and(|flight| Some(flight.event.message_id) == message_id)
            }
            HistoryPendingEvent::Done { stream_id } => {
                batch.lane.stream_id() == stream_id
                    && message_id.is_none()
                    && batch.flight.is_none()
                    && batch.ready
                    && !batch.cancelled
                    && batch.done.is_some()
            }
        }
    }
    /// Prepare/retry a complete pinned RECORD only after the READ reply was admitted.
    /// # Errors
    /// Real backend/integrity failures produce a non-OK DONE without fake records.
    /// Transport/identity/auth failures are explicit; short caller output consumes no credit/ID.
    /// The first call for a new message MUST run inside a real first-frame FIFO reservation, followed
    /// by first-fragment admission in that same serialized boundary. A readiness precheck is insufficient.
    /// This prevents an unadmitted prepared record from being mistaken for an already-started cancel barrier.
    pub fn next_data(&mut self, out: &mut [u8]) -> Result<Option<DataEvent>, HistoryFailure> {
        let event = self.stage_data_inner(out.len(), || out.fill(0))?;
        if event.is_some() {
            let flight = self
                .batch
                .get()
                .and_then(|b| b.flight)
                .ok_or(HistoryFailure::InvalidState)?;
            self.copy_flight(flight, out).map(Some)
        } else {
            Ok(None)
        }
    }
    /// Pin a validated immutable RECORD without copying its complete body.
    /// First admission MUST be inside the actual first-fragment reservation.
    /// Capacity failures consume no message identifier or whole-message credit.
    /// # Errors
    /// Same lease, scope, storage and capacity failures as `next_data`.
    // Admission checks and retained-record staging are serial phases. Do not
    // retain the staging temporaries in a caller while it checks a pending event.
    #[inline(never)]
    pub fn stage_data(&mut self, capacity: usize) -> Result<Option<DataEvent>, HistoryFailure> {
        self.stage_data_inner(capacity, || {})
    }
    /// Inspect the exact current immutable flight for one fragment encoder.
    /// No borrowed reference escapes; `finish_data` remains final-admission only.
    /// # Errors
    /// Wrong/stale message, expired scope/lease or conflicting mutable access fails explicitly.
    pub fn with_data<T>(
        &self,
        message: u16,
        inspect: impl for<'body> FnOnce(&'body [u8]) -> T,
    ) -> Result<T, HistoryFailure> {
        let batch = self.pending_batch()?.ok_or(HistoryFailure::InvalidState)?;
        let flight = batch.flight.ok_or(HistoryFailure::InvalidState)?;
        if flight.event.message_id != message {
            return Err(HistoryFailure::Identity);
        }
        let event = self.event.try_borrow().map_err(|_| HistoryFailure::Busy)?;
        Ok(inspect(&event[..flight.event.length]))
    }
    fn stage_data_inner(
        &mut self,
        capacity: usize,
        mut clear: impl FnMut(),
    ) -> Result<Option<DataEvent>, HistoryFailure> {
        if let Err(error) = self.alive() {
            clear();
            return Err(error);
        }
        let Some(batch) = self.batch.get() else {
            return Ok(None);
        };
        if let Some(flight) = batch.flight {
            return if capacity < flight.event.length {
                Err(HistoryFailure::Capacity)
            } else {
                Ok(Some(flight.event))
            };
        }
        if !batch.ready || batch.cancelled || batch.done.is_some() {
            return Ok(None);
        }
        if batch.sent == self.batch_plan.count {
            self.mark_done(Status::Ok, Reason::None);
            return Ok(None);
        }
        let Some(opened) = self.opened.get() else {
            return Err(HistoryFailure::InvalidState);
        };
        if self.now.get() >= opened.expires {
            self.mark_done(Status::CursorExpired, Reason::None);
            return Ok(None);
        }
        if batch.used >= batch.granted {
            return Ok(None);
        }
        match self.prepare_record(opened, batch, capacity) {
            Ok(event) => Ok(Some(event)),
            Err(HistoryFailure::Denied | HistoryFailure::Closed) => {
                self.invalidate();
                clear();
                Err(HistoryFailure::Denied)
            }
            Err(HistoryFailure::Capacity) if capacity < self.data_max => {
                Err(HistoryFailure::Capacity)
            }
            Err(HistoryFailure::Busy) => Err(HistoryFailure::Busy),
            Err(error) => {
                let (status, reason) = wire(error);
                self.mark_done(status, reason);
                clear();
                Ok(None)
            }
        }
    }
    fn prepare_record(
        &mut self,
        opened: Opened,
        mut batch: Batch,
        capacity: usize,
    ) -> Result<DataEvent, HistoryFailure> {
        let id = self.batch_plan.ids[usize::from(batch.sent)];
        let body_max = P.min(self.data_max - HistoryRecord::OVERHEAD);
        let length = {
            let mut event = self
                .event
                .try_borrow_mut()
                .map_err(|_| HistoryFailure::Busy)?;
            let content =
                self.store
                    .read(opened.lease, id, &mut event[34..34 + body_max], self.access)?;
            if content.length > body_max
                || content.domain != opened.filter.domain
                || content.epoch != opened.lease.snapshot.epoch
                || content.record_id != id
            {
                return Err(HistoryFailure::StoredIntegrity);
            }
            let stored = StoredRecord {
                domain: content.domain,
                epoch: content.epoch,
                record_id: content.record_id,
                record_type: content.record_type,
                schema: content.schema,
                utc_ms: content.utc_ms,
                flags: content.flags,
                payload: &event[34..34 + content.length],
            };
            stored
                .verify(content.commit, content.stored_crc)
                .map_err(|error| {
                    if error == RecordIntegrity::Uncommitted {
                        HistoryFailure::Storage
                    } else {
                        HistoryFailure::StoredIntegrity
                    }
                })?;
            self.store.validate_record(opened.lease, stored)?;
            let (header, crc) = HistoryRecord {
                snapshot: opened.lease.snapshot.snapshot,
                stream_id: batch.lane.stream_id(),
                epoch: stored.epoch,
                record_id: stored.record_id,
                record_type: stored.record_type,
                schema: stored.schema,
                utc_ms: stored.utc_ms,
                flags: stored.flags,
                payload: stored.payload,
            }
            .envelope(self.data_max)
            .map_err(|_| HistoryFailure::StoredIntegrity)?;
            let length = HistoryRecord::OVERHEAD + content.length;
            event[..34].copy_from_slice(&header);
            event[length - 4..length].copy_from_slice(&crc);
            length
        };
        self.alive()?;
        if capacity < length {
            return Err(HistoryFailure::Capacity);
        }
        let message_id = self
            .lane
            .with_mut(|owner| owner.begin_message(batch.lane))
            .map_err(lane_failure)?;
        batch.used = batch.used.checked_add(1).ok_or(HistoryFailure::Capacity)?;
        let event = DataEvent { message_id, length };
        batch.flight = Some(Flight {
            event,
            record_id: id,
        });
        self.batch.set(Some(batch));
        Ok(event)
    }
    /// Confirm only after the actual last fragment entered the ordered DATA queue.
    /// # Errors
    /// Wrong IDs preserve in-flight state; actual sent-proof failure ends the connection.
    // Sent-proof mutation runs after event validation, so its scratch need
    // not remain live during validation/prefetch.
    #[inline(never)]
    pub fn finish_data(&mut self, message: u16) -> Result<(), HistoryFailure> {
        self.alive()?;
        let batch = self
            .batch
            .get_mut()
            .as_ref()
            .ok_or(HistoryFailure::InvalidState)?;
        let flight = batch.flight.ok_or(HistoryFailure::InvalidState)?;
        if flight.event.message_id != message {
            return Err(HistoryFailure::Identity);
        }
        self.lane
            .with_mut(|owner| owner.finish_message(batch.lane, message))
            .map_err(lane_failure)?;
        let opened = self
            .opened
            .get_mut()
            .as_ref()
            .ok_or(HistoryFailure::InvalidState)?;
        if let Err(error) = self
            .store
            .record_admitted(opened.lease, flight.record_id, self.access)
        {
            self.invalidate();
            return Err(error);
        }
        self.alive()?;
        // The exclusive actor borrow keeps the cells stable across admission.
        self.opened
            .get_mut()
            .as_mut()
            .ok_or(HistoryFailure::InvalidState)?
            .last_message = message;
        let batch = self
            .batch
            .get_mut()
            .as_mut()
            .ok_or(HistoryFailure::InvalidState)?;
        batch.sent += 1;
        batch.flight = None;
        let complete = !batch.cancelled && batch.sent == self.batch_plan.count;
        self.event
            .try_borrow_mut()
            .map_err(|_| HistoryFailure::Busy)?
            .fill(0);
        if complete {
            self.mark_done(Status::Ok, Reason::None);
        }
        Ok(())
    }
    /// Copy fixed16-byte DONE; this consumes no DATA grant or identifier.
    /// # Errors
    /// Actual actor/scope or output-capacity failure.
    pub fn done(&self, out: &mut [u8]) -> Result<Option<usize>, HistoryFailure> {
        if let Err(error) = self.alive() {
            out.fill(0);
            return Err(error);
        }
        let Some(done) = self.batch.get().and_then(|batch| batch.done) else {
            return Ok(None);
        };
        done.encode(out)
            .map(Some)
            .map_err(|_| HistoryFailure::Capacity)
    }
    /// Confirm actual control-queue DONE admission, then release this normal batch.
    /// # Errors
    /// Wrong stream, cancellation or unfinished DATA cannot release ownership early.
    pub fn done_admitted(&mut self, stream: u16) -> Result<(), HistoryFailure> {
        self.alive()?;
        let batch = self.batch.get().ok_or(HistoryFailure::InvalidState)?;
        if batch.lane.stream_id() != stream {
            return Err(HistoryFailure::Identity);
        }
        if batch.flight.is_some() || batch.done.is_none() || batch.cancelled {
            return Err(HistoryFailure::InvalidState);
        }
        self.lane
            .with_mut(|owner| owner.release(batch.lane))
            .map_err(lane_failure)?;
        self.batch.set(None);
        Ok(())
    }
    fn request_scope(&self, request: HistoryRequest) -> (HistoryOperation, Option<u8>) {
        match request {
            HistoryRequest::Enumerate { .. } => (HistoryOperation::Enumerate, None),
            HistoryRequest::Open(filter)
            | HistoryRequest::OpenV2(filter)
            | HistoryRequest::Count(filter) => (HistoryOperation::Read, Some(filter.domain)),
            HistoryRequest::Ack { .. } => (
                HistoryOperation::Acknowledge,
                self.opened.get().map(|opened| opened.filter.domain),
            ),
            _ => (
                HistoryOperation::Read,
                self.opened.get().map(|opened| opened.filter.domain),
            ),
        }
    }
    #[inline(never)]
    fn start_cancel(
        &mut self,
        frame: Frame<'_>,
        request: HistoryRequest,
    ) -> Result<bool, HistoryFailure> {
        let (snapshot, stream) = match request {
            HistoryRequest::Close { snapshot } => (snapshot, None),
            HistoryRequest::CancelRead {
                snapshot,
                stream_id,
            } => (snapshot, Some(stream_id)),
            _ => return Ok(true),
        };
        let Some(opened) = self
            .opened
            .get()
            .filter(|opened| opened.lease.snapshot.snapshot == snapshot)
        else {
            return Ok(true);
        };
        if let Some(pending) = self.cancelling.get()
            && (pending.request != request || pending.message != frame.header.message_id)
        {
            return Err(HistoryFailure::InvalidState);
        }
        let Some(mut batch) = self.batch.get() else {
            return Ok(true);
        };
        if stream.is_some_and(|stream| stream != batch.lane.stream_id()) {
            return Ok(true);
        }
        let barrier = self
            .lane
            .with_mut(|owner| owner.cancel(batch.lane))
            .map_err(lane_failure)?;
        batch.cancelled = true;
        batch.done = None;
        self.batch.set(Some(batch));
        if self.cancelling.get().is_none() {
            let deadline = self
                .now
                .get()
                .checked_add(10000)
                .ok_or(HistoryFailure::Clock)?;
            self.cancelling.set(Some(Cancelling {
                request,
                message: frame.header.message_id,
                deadline,
            }));
        }
        if barrier.is_some() && batch.flight.is_none() && batch.sent < self.batch_plan.count {
            return self.store.retire_record(
                opened.lease,
                self.batch_plan.ids[usize::from(batch.sent)],
                self.access,
            );
        }
        Ok(barrier.is_some())
    }
    #[inline(never)]
    fn cancelled_reply(&mut self, request: HistoryRequest) -> Result<(u16, bool), HistoryFailure> {
        let (snapshot, stream, close) = match request {
            HistoryRequest::Close { snapshot } => (snapshot, None, true),
            HistoryRequest::CancelRead {
                snapshot,
                stream_id,
            } => (snapshot, Some(stream_id), false),
            _ => return Err(HistoryFailure::Invalid),
        };
        let opened = self
            .opened
            .get()
            .filter(|opened| opened.lease.snapshot.snapshot == snapshot)
            .ok_or(HistoryFailure::CursorExpired)?;
        let barrier = if let Some(batch) = self.batch.get() {
            if stream.is_some_and(|stream| stream != batch.lane.stream_id()) {
                return Err(HistoryFailure::InvalidState);
            }
            if !batch.cancelled {
                return Err(HistoryFailure::InvalidState);
            }
            let barrier = self
                .lane
                .with_mut(|owner| owner.cancel_barrier(batch.lane))
                .map_err(lane_failure)?;
            self.lane
                .with_mut(|owner| owner.release(batch.lane))
                .map_err(lane_failure)?;
            self.batch.set(None);
            barrier
        } else {
            if stream.is_some() {
                return Err(HistoryFailure::InvalidState);
            }
            opened.last_message
        };
        self.cancelling.set(None);
        if close {
            self.store.close(opened.lease)?;
            self.opened.set(None);
        }
        if self.now.get() >= opened.expires {
            return Err(HistoryFailure::CursorExpired);
        }
        Ok((barrier, close))
    }
    fn discard_open(
        &mut self,
        lease: StoreLease,
        filter: HistoryFilter,
    ) -> Result<(), HistoryFailure> {
        // Retain the actual opaque handle until cleanup succeeds, even if its
        // returned wire metadata was invalid. Close-hook retries remain possible.
        self.opened.set(Some(Opened {
            lease,
            filter,
            expires: self.now.get(),
            last_message: 0,
        }));
        match self.store.close(lease) {
            Ok(()) => {
                self.opened.set(None);
                Ok(())
            }
            Err(error) => {
                self.invalidate();
                Err(error)
            }
        }
    }
    fn open_snapshot(&mut self, filter: HistoryFilter) -> Result<HistorySnapshot, HistoryFailure> {
        if let Some(old) = self.opened.get() {
            if self.now.get() >= old.expires && self.batch.get().is_none() {
                self.store.close(old.lease)?;
                self.opened.set(None);
            } else {
                return Err(HistoryFailure::Busy);
            }
        }
        let snapshot = self.next_snapshot;
        self.next_snapshot = snapshot.checked_add(1).ok_or(HistoryFailure::Capacity)?;
        let mut pending_lease = None;
        self.store.open_into(
            snapshot,
            filter,
            self.now.get(),
            self.access,
            &mut pending_lease,
        )?;
        let lease = pending_lease.ok_or(HistoryFailure::StoredIntegrity)?;
        let Some(expires) = self
            .now
            .get()
            .checked_add(u64::from(lease.snapshot.lease_s) * 1000)
        else {
            self.discard_open(lease, filter)?;
            return Err(HistoryFailure::Clock);
        };
        if lease.handle == 0
            || lease.snapshot.snapshot != snapshot
            || lease.snapshot.lease_s == 0
            || self.domains[..self.domain_count]
                .iter()
                .find(|domain| domain.domain == filter.domain)
                .is_none_or(|domain| domain.epoch != lease.snapshot.epoch)
            || lease.snapshot.validate().is_err()
        {
            self.discard_open(lease, filter)?;
            return Err(HistoryFailure::Storage);
        }
        self.opened.set(Some(Opened {
            lease,
            filter,
            expires,
            last_message: 0,
        }));
        Ok(lease.snapshot)
    }
    #[inline(never)]
    fn install_read_plan(
        &mut self,
        opened: &Opened,
        after: u64,
        max: u16,
    ) -> Result<(), HistoryFailure> {
        self.batch_plan = ReadPlan {
            ids: [0; 16],
            count: 0,
            end_after_id: 0,
            eof: false,
        };
        self.store.plan_into(
            opened.lease,
            after,
            max,
            self.now.get(),
            self.access,
            &mut self.batch_plan,
        )?;
        let plan = &self.batch_plan;
        if plan.count > max
            || plan.count > 16
            || (plan.count == 0 && (!plan.eof || plan.end_after_id != after))
        {
            return Err(HistoryFailure::Storage);
        }
        let mut previous = after;
        for id in &plan.ids[..usize::from(plan.count)] {
            if *id <= previous
                || *id < opened.lease.snapshot.first_id
                || *id > opened.lease.snapshot.last_id
            {
                return Err(HistoryFailure::Storage);
            }
            previous = *id;
        }
        if plan.end_after_id != previous {
            return Err(HistoryFailure::Storage);
        }
        Ok(())
    }
    #[inline(never)]
    fn begin_read(
        &mut self,
        snapshot: u32,
        after: u64,
        max: u16,
        message: u16,
    ) -> Result<HistoryRead, HistoryFailure> {
        let opened = self.snapshot(snapshot)?;
        if self.batch.get().is_some() {
            return Err(HistoryFailure::Busy);
        }
        let lane = self
            .lane
            .with_mut(|owner| owner.acquire(DataKind::History))
            .map_err(lane_failure)?;
        let planned = (|| {
            self.install_read_plan(&opened, after, max)?;
            self.renew(snapshot)
        })();
        match planned {
            Ok(()) => (),
            Err(error) => {
                let _ = self.lane.with_mut(|owner| owner.release(lane));
                return Err(error);
            }
        }
        self.batch_request = Some((
            message,
            HistoryRequest::Read {
                snapshot,
                after_id: after,
                max_records: max,
            },
        ));
        self.batch.set(Some(Batch {
            lane,
            ready: false,
            granted: 0,
            used: 0,
            sent: 0,
            flight: None,
            done: None,
            cancelled: false,
        }));
        Ok(HistoryRead {
            stream_id: lane.stream_id(),
            end_after_id: self.batch_plan.end_after_id,
            eof: self.batch_plan.eof,
            admitted: self.batch_plan.count,
        })
    }
    #[inline(never)]
    fn enumerate_domains(
        &self,
        start: u8,
        max: u8,
        out: &mut [u8],
    ) -> Result<usize, HistoryFailure> {
        let capacity = usize::from(max).min(out.len().saturating_sub(22) / 21);
        let mut bytes = [0; 168];
        let mut count = 0;
        let mut next = 255;
        for domain in self.domains[..self.domain_count]
            .iter()
            .filter(|domain| domain.domain >= start)
        {
            if count == capacity {
                next = domain.domain;
                break;
            }
            domain
                .encode(&mut bytes[count * 21..count * 21 + 21])
                .map_err(|_| HistoryFailure::Storage)?;
            count += 1;
        }
        if count == 0 && next != 255 {
            return Err(HistoryFailure::Capacity);
        }
        HistoryResponse::Enumerate(HistoryPage {
            dataset_id: self.dataset_id,
            next,
            entries: &bytes[..count * 21],
        })
        .encode(out)
        .map_err(|_| HistoryFailure::Capacity)
    }
    fn ack_rejection(&self, request: HistoryRequest) -> Option<(Status, Reason)> {
        let HistoryRequest::Ack { snapshot, .. } = request else {
            return None;
        };
        match self.snapshot(snapshot) {
            Err(error) => Some(wire(error)),
            Ok(opened) if opened.filter.filter_flags != 0 => {
                Some((Status::InvalidState, Reason::FilteredAckForbidden))
            }
            Ok(_) => None,
        }
    }
    fn reply_min(request: HistoryRequest) -> usize {
        match request {
            HistoryRequest::Enumerate { .. } => 22,
            HistoryRequest::Open(_) | HistoryRequest::OpenV2(_) => 34,
            HistoryRequest::Read { .. } => 17,
            HistoryRequest::Ack { .. } | HistoryRequest::Count(_) => 12,
            _ => 6,
        }
    }
    fn valid_frame(&self, frame: Frame<'_>) -> bool {
        !(frame.header.kind != Kind::Request
            || frame.header.message_id == 0
            || frame.header.offset != 0
            || usize::from(frame.header.total_length) != frame.payload.len()
            || frame.payload.len() > usize::from(self.metadata.input_max))
    }
    #[inline(never)]
    fn acknowledge(
        &mut self,
        snapshot: u32,
        epoch: u32,
        upto_id: u64,
    ) -> Result<u64, HistoryFailure> {
        {
            let opened = self
                .opened
                .get_mut()
                .as_ref()
                .ok_or(HistoryFailure::CursorExpired)?;
            if opened.lease.snapshot.snapshot != snapshot || self.now.get() >= opened.expires {
                return Err(HistoryFailure::CursorExpired);
            }
            if epoch != opened.lease.snapshot.epoch {
                return Err(HistoryFailure::Conflict);
            }
        }
        self.renew(snapshot)?;
        let opened = self
            .opened
            .get_mut()
            .as_ref()
            .ok_or(HistoryFailure::CursorExpired)?;
        let committed = self.store.ack_atomic(
            opened.lease,
            opened.filter,
            epoch,
            upto_id,
            self.now.get(),
            self.access,
        )?;
        if committed < upto_id || committed < opened.lease.committed_id {
            return Err(HistoryFailure::Storage);
        }
        Ok(committed)
    }
    fn dispatch_request(
        &mut self,
        request: HistoryRequest,
        message: u16,
        out: &mut [u8],
    ) -> Result<usize, HistoryFailure> {
        match request {
            HistoryRequest::Enumerate { start, max } => self.enumerate_domains(start, max, out),
            HistoryRequest::Open(filter) => {
                let snapshot = self.open_snapshot(filter)?;
                HistoryResponse::Open(snapshot)
                    .encode(out)
                    .map_err(|_| HistoryFailure::Capacity)
            }
            HistoryRequest::OpenV2(filter) => {
                let snapshot = self.open_snapshot(filter)?;
                HistoryResponse::OpenV2(snapshot)
                    .encode(out)
                    .map_err(|_| HistoryFailure::Capacity)
            }
            HistoryRequest::Read {
                snapshot,
                after_id,
                max_records,
            } => {
                let read = self.begin_read(snapshot, after_id, max_records, message)?;
                HistoryResponse::Read(read)
                    .encode(out)
                    .map_err(|_| HistoryFailure::Capacity)
            }
            HistoryRequest::Ack {
                snapshot,
                epoch,
                upto_id,
            } => {
                let committed = self.acknowledge(snapshot, epoch, upto_id)?;
                HistoryResponse::Ack {
                    committed_id: committed,
                }
                .encode(out)
                .map_err(|_| HistoryFailure::Capacity)
            }
            HistoryRequest::Count(filter) => {
                let (epoch, count) = self.store.count(filter, self.access)?;
                if self.domains[..self.domain_count]
                    .iter()
                    .find(|domain| domain.domain == filter.domain)
                    .is_none_or(|domain| domain.epoch != epoch)
                {
                    return Err(HistoryFailure::CursorExpired);
                }
                HistoryResponse::Count { epoch, count }
                    .encode(out)
                    .map_err(|_| HistoryFailure::Capacity)
            }
            HistoryRequest::Close { .. } | HistoryRequest::CancelRead { .. } => {
                let (last_data_message_id, close) = self.cancelled_reply(request)?;
                if close {
                    HistoryResponse::Close {
                        last_data_message_id,
                    }
                } else {
                    HistoryResponse::CancelRead {
                        last_data_message_id,
                    }
                }
                .encode(out)
                .map_err(|_| HistoryFailure::Capacity)
            }
        }
    }
    fn prepare_response(
        &mut self,
        frame: Frame<'_>,
        preparation: &Preparation,
    ) -> Result<bool, SessionError> {
        if frame.payload.len() > self.prepared_body.len() {
            return Err(SessionError::InvalidFrame);
        }
        match self
            .store
            .prepare_ref(preparation, self.now.get(), self.access)
        {
            Ok(ready) => {
                if !ready || self.prepared_header.is_some() {
                    self.prepared_header = Some(frame.header);
                    self.prepared_length = frame.payload.len();
                    self.prepared_body[..self.prepared_length].copy_from_slice(frame.payload);
                }
                Ok(ready)
            }
            Err(_) => Ok(true),
        }
    }
    fn check_response_ready(&mut self, frame: Frame<'_>) -> Result<bool, SessionError> {
        if self.prepared_header.is_some_and(|header| {
            header != frame.header || self.prepared_body[..self.prepared_length] != *frame.payload
        }) {
            return Err(SessionError::InvalidHandlerResponse);
        }
        if !self.valid_frame(frame) {
            return Err(SessionError::InvalidFrame);
        }
        if self.alive().is_err() && self.closed.get() {
            return Err(SessionError::AuthorizationRevoked);
        }
        if !self.authorized(frame) {
            if self.prepared_header.is_some() {
                self.store.detach_preparation();
                self.invalidate();
                return Err(SessionError::AuthorizationRevoked);
            }
            return Ok(true);
        }
        let Ok(request) = HistoryRequest::decode(frame.header.opcode, frame.payload) else {
            return Ok(true);
        };
        if !self
            .start_cancel(frame, request)
            .map_err(|_| SessionError::InvalidHandlerResponse)?
        {
            return Ok(false);
        }
        if usize::from(self.metadata.output_max) < Self::reply_min(request) {
            return Ok(true);
        }
        let preparation = match request {
            HistoryRequest::Open(filter) | HistoryRequest::OpenV2(filter) => {
                if self.batch.get().is_some() || self.next_snapshot.checked_add(1).is_none() {
                    return Ok(true);
                }
                if let Some(opened) = self.opened.get() {
                    if self.now.get() < opened.expires {
                        return Ok(true);
                    }
                    if self.store.close(opened.lease).is_err() {
                        return Ok(true);
                    }
                    self.opened.set(None);
                }
                Some(Preparation::Open {
                    snapshot: self.next_snapshot,
                    filter,
                })
            }
            HistoryRequest::Read {
                snapshot,
                after_id,
                max_records,
            } => {
                let Ok(opened) = self.snapshot(snapshot) else {
                    return Ok(true);
                };
                if self.batch.get().is_some()
                    || !(1..=16).contains(&max_records)
                    || after_id > opened.lease.snapshot.last_id
                {
                    return Ok(true);
                }
                Some(Preparation::Plan {
                    lease: opened.lease,
                    after_id,
                    max: max_records,
                })
            }
            HistoryRequest::Count(filter) => Some(Preparation::Count { filter }),
            HistoryRequest::Ack {
                snapshot,
                epoch,
                upto_id,
            } => {
                if self.ack_rejection(request).is_some() {
                    return Ok(true);
                }
                let Ok(opened) = self.snapshot(snapshot) else {
                    return Ok(true);
                };
                if epoch != opened.lease.snapshot.epoch {
                    return Ok(true);
                }
                Some(Preparation::Ack {
                    lease: opened.lease,
                    filter: opened.filter,
                    epoch,
                    upto: upto_id,
                })
            }
            _ => None,
        };
        let Some(preparation) = preparation else {
            return Ok(true);
        };
        self.prepare_response(frame, &preparation)
    }

    fn dispatch_checked<const PREPARE: bool>(
        &mut self,
        frame: Frame<'_>,
        out: &mut [u8],
    ) -> Result<usize, SessionError> {
        if self.closed.get() {
            out.fill(0);
            return Err(SessionError::AuthorizationRevoked);
        }
        if !self.valid_frame(frame) {
            return Err(SessionError::InvalidFrame);
        }
        if out.len() < 4 {
            return Err(SessionError::Capacity);
        }
        if frame.header.module != 0x10 {
            return encode_failure(
                frame.header.opcode,
                Status::UnsupportedModule,
                Reason::None,
                out,
            );
        }
        if !matches!(frame.header.opcode, 1..=8) {
            return encode_failure(
                frame.header.opcode,
                Status::UnsupportedOpcode,
                Reason::None,
                out,
            );
        }
        if let Err(error) = self.alive() {
            if self.closed.get() {
                out.fill(0);
                return Err(SessionError::AuthorizationRevoked);
            }
            let (status, reason) = wire(error);
            return encode_failure(frame.header.opcode, status, reason, out);
        }
        let Ok(request) = HistoryRequest::decode(frame.header.opcode, frame.payload) else {
            return encode_failure(
                frame.header.opcode,
                Status::InvalidArgument,
                Reason::None,
                out,
            );
        };
        if !self.authorized(frame) {
            if self.closed.get() {
                out.fill(0);
                return Err(SessionError::AuthorizationRevoked);
            }
            return encode_failure(
                frame.header.opcode,
                Status::AccessDenied,
                Reason::RoleRequired,
                out,
            );
        }
        if let Some((status, reason)) = self.ack_rejection(request) {
            return encode_failure(frame.header.opcode, status, reason, out);
        }
        let limit = out
            .len()
            .min(usize::from(self.metadata.output_max))
            .min(CONTROL_REPLY_MAX);
        let needed = Self::reply_min(request);
        if limit < needed {
            return Err(SessionError::Capacity);
        }
        if PREPARE {
            if !self.response_ready(frame)? {
                out.fill(0);
                return Err(SessionError::Deferred);
            }
        } else if !self.prepared_ready.get()
            || self.prepared_header != Some(frame.header)
            || self.prepared_body[..self.prepared_length] != *frame.payload
        {
            out.fill(0);
            return Err(SessionError::InvalidHandlerResponse);
        }
        if PREPARE {
            let mut pending = [0; CONTROL_REPLY_MAX];
            let length = self.dispatch_prepared(request, frame, &mut pending[..limit])?;
            out[..length].copy_from_slice(&pending[..length]);
            Ok(length)
        } else {
            // Session lends its private, unpublished response staging buffer.
            // Capacity and the exact one-shot permit were checked above.
            self.dispatch_prepared(request, frame, &mut out[..limit])
        }
    }
    fn dispatch_prepared(
        &mut self,
        request: HistoryRequest,
        frame: Frame<'_>,
        out: &mut [u8],
    ) -> Result<usize, SessionError> {
        let result = self.dispatch_request(request, frame.header.message_id, out);
        self.prepared_ready.set(false);
        self.prepared_header = None;
        self.prepared_length = 0;
        if self.alive().is_err() && self.closed.get()
            || !self.authorized(frame)
            || result == Err(HistoryFailure::Denied)
        {
            self.invalidate();
            out.fill(0);
            return Err(SessionError::AuthorizationRevoked);
        }
        let length = match result {
            Ok(length) => length,
            Err(error) => {
                let (status, reason) = wire(error);
                HistoryResponse::Failure {
                    opcode: frame.header.opcode,
                    status: status as u16,
                    reason: reason as u16,
                }
                .encode(out)
                .map_err(|_| SessionError::InvalidHandlerResponse)?
            }
        };
        Ok(length)
    }
}
impl<R: RecordStore, const D: usize, const P: usize> ServiceHandler for HistoryActor<'_, R, D, P> {
    fn advance(&mut self, now_ms: u64) -> Result<(), SessionError> {
        HistoryActor::advance(self, now_ms).map_err(|_| SessionError::AuthorizationRevoked)
    }
    fn required_features(&self, module: &ModuleEntry, opcode: u8) -> Option<u32> {
        (*module == self.metadata && matches!(opcode, 1..=8)).then_some(0)
    }
    fn must_end_session(&self) -> bool {
        self.closed.get() || self.alive().is_err_and(|_| self.closed.get())
    }
    fn authorized(&self, frame: Frame<'_>) -> bool {
        if frame.header.module != 0x10 || self.alive().is_err() || !self.access.module_allowed() {
            return false;
        }
        let Ok(request) = HistoryRequest::decode(frame.header.opcode, frame.payload) else {
            return true;
        };
        let (operation, domain) = self.request_scope(request);
        if !self.allowed(domain, operation) {
            return false;
        }
        if let HistoryRequest::Enumerate { start, max } = request {
            self.domains[..self.domain_count]
                .iter()
                .filter(|domain| domain.domain >= start)
                .take(usize::from(max) + 1)
                .all(|domain| self.allowed(Some(domain.domain), operation))
        } else {
            true
        }
    }
    fn response_ready(&mut self, frame: Frame<'_>) -> Result<bool, SessionError> {
        self.prepared_ready.set(false);
        let ready = self.check_response_ready(frame)?;
        if ready
            && frame.payload.len() <= self.prepared_body.len()
            && HistoryRequest::decode(frame.header.opcode, frame.payload).is_ok()
        {
            self.prepared_header = Some(frame.header);
            self.prepared_length = frame.payload.len();
            self.prepared_body[..frame.payload.len()].copy_from_slice(frame.payload);
            self.prepared_ready.set(true);
        }
        Ok(ready)
    }
    fn response_admitted(&mut self, frame: Frame<'_>, response: &[u8]) -> Result<(), SessionError> {
        if frame.header.module != 0x10 || frame.header.opcode != 3 {
            return Ok(());
        }
        let decoded = HistoryResponse::decode(3, response)
            .map_err(|_| SessionError::InvalidHandlerResponse)?;
        let HistoryResponse::Read(read) = decoded else {
            return Ok(());
        };
        self.alive()
            .map_err(|_| SessionError::AuthorizationRevoked)?;
        let Some(mut batch) = self.batch.get() else {
            return Err(SessionError::InvalidHandlerResponse);
        };
        let expected = HistoryRead {
            stream_id: batch.lane.stream_id(),
            end_after_id: self.batch_plan.end_after_id,
            eof: self.batch_plan.eof,
            admitted: self.batch_plan.count,
        };
        let request = HistoryRequest::decode(3, frame.payload)
            .map_err(|_| SessionError::InvalidHandlerResponse)?;
        if !self.valid_frame(frame)
            || self.batch_request != Some((frame.header.message_id, request))
            || read != expected
            || batch.cancelled
        {
            return Err(SessionError::InvalidHandlerResponse);
        }
        batch.ready = true;
        self.batch.set(Some(batch));
        if self.batch_plan.count == 0 {
            self.mark_done(Status::Ok, Reason::None);
        }
        Ok(())
    }
    fn connection_closed(&mut self) -> Result<(), SessionError> {
        self.store.detach_preparation();
        self.invalidate();
        let lane = self.lane.with_mut(|owner| {
            owner.disconnect();
            Ok(())
        });
        let store = if let Some(opened) = self.opened.get() {
            let result = self.store.close(opened.lease);
            if result.is_ok() {
                self.opened.set(None);
            }
            result
        } else {
            Ok(())
        };
        if lane.is_err() || store.is_err() {
            return Err(SessionError::InvalidHandlerResponse);
        }
        Ok(())
    }
    fn dispatch(&mut self, frame: Frame<'_>, out: &mut [u8]) -> Result<usize, SessionError> {
        self.dispatch_checked::<true>(frame, out)
    }
    fn dispatch_ready(&mut self, frame: Frame<'_>, out: &mut [u8]) -> Result<usize, SessionError> {
        self.dispatch_checked::<false>(frame, out)
    }
}
fn encode_failure(
    _opcode: u8,
    status: Status,
    reason: Reason,
    out: &mut [u8],
) -> Result<usize, SessionError> {
    if out.len() < 4 {
        return Err(SessionError::Capacity);
    }
    out[..2].copy_from_slice(&(status as u16).to_le_bytes());
    out[2..4].copy_from_slice(&(reason as u16).to_le_bytes());
    Ok(4)
}
fn lane_failure(error: LaneError) -> HistoryFailure {
    match error {
        LaneError::Busy => HistoryFailure::Busy,
        LaneError::Identity => HistoryFailure::Identity,
        LaneError::Exhausted => HistoryFailure::Capacity,
        LaneError::Closed => HistoryFailure::Closed,
        LaneError::Cancelled | LaneError::InvalidState => HistoryFailure::InvalidState,
    }
}
fn wire(error: HistoryFailure) -> (Status, Reason) {
    (
        match error {
            HistoryFailure::Invalid => Status::InvalidArgument,
            HistoryFailure::InvalidState | HistoryFailure::Identity => Status::InvalidState,
            HistoryFailure::Denied => return (Status::AccessDenied, Reason::RoleRequired),
            HistoryFailure::Busy => Status::Busy,
            HistoryFailure::Capacity => Status::LimitExceeded,
            HistoryFailure::NotFound => Status::NotFound,
            HistoryFailure::Storage => Status::StorageFailure,
            HistoryFailure::Conflict => Status::Conflict,
            HistoryFailure::CursorExpired => Status::CursorExpired,
            HistoryFailure::StoredIntegrity => {
                return (Status::IntegrityFailure, Reason::StoredCrcMismatch);
            }
            HistoryFailure::Timeout => Status::Timeout,
            HistoryFailure::TransportUnavailable => Status::UnsupportedOpcode,
            HistoryFailure::Closed | HistoryFailure::Clock => Status::InternalFailure,
        },
        Reason::None,
    )
}

mod typed_joined;
pub use typed_joined::TypedJoinedHistory;
