//! Small LATEST cache over a real incremental append-only log; no resident record array.
use core::{
    cell::{Cell, RefCell},
    task::Poll,
};
use device_health::{ResultInfo, ResultOrigin, SourcedResult, Timestamp};
use device_health_service::{
    Error, HealthImport, HealthImportPort, ImportKey, LatestStore, StoredLatest,
};
use device_history_service::{
    HistoryAccess, HistoryFailure, HistoryOperation, Preparation, RecordStore,
};
use device_history_store::{
    AppendRecord, HistoryClock, HistorySchema,
    async_paged::{AsyncPagedHistoryStore, JobResult, JobToken},
};
use device_protocol::HistoryFilter;
use device_storage::{StoredRecord, incremental::AsyncMedium};
/// Product domain mapping; existing six domain IDs remain unchanged.
pub const DOMAINS: [u8; 3] = [3, 4, 8];
/// Actual strict validator for complete source-aware records, not arbitrary opaque bytes.
#[derive(Debug)]
pub struct HealthSchema;
impl HistorySchema for HealthSchema {
    fn validate(&self, r: StoredRecord<'_>, parent: u64) -> Result<(), HistoryFailure> {
        let module = module(r.domain)?;
        if parent != 0 || r.schema != 2 || r.record_type != kind(module) {
            return Err(HistoryFailure::StoredIntegrity);
        }
        SourcedResult::decode(module, r.payload)
            .map(|_| ())
            .map_err(|_| HistoryFailure::StoredIntegrity)
    }
}
/// Three independent255-record domains,4spares each, complete bodies<=16B and272B metadata.
pub type HealthHistory<M, L, C> = AsyncPagedHistoryStore<M, L, HealthSchema, C, 3, 255, 16, 272>;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum KeyKind {
    Local,
    Import,
    Ui,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Key {
    primary: u64,
    secondary: u32,
    module: u8,
    kind: KeyKind,
}
impl Key {
    const fn ui(module: u8, sequence: u64) -> Self {
        Self {
            primary: sequence,
            secondary: 0,
            module,
            kind: KeyKind::Ui,
        }
    }
    fn import(key: ImportKey) -> Self {
        Self {
            primary: key.connection,
            secondary: u32::from(key.message),
            module: key.module,
            kind: KeyKind::Import,
        }
    }
    const fn local(module: u8, job: u32, sample: u64) -> Self {
        Self {
            primary: sample,
            secondary: job,
            module,
            kind: KeyKind::Local,
        }
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
struct Item {
    key: Key,
    module: u8,
    utc: i64,
    valid: bool,
    bytes: [u8; 9],
}
#[derive(Clone, Copy)]
struct Cached {
    ui_sequence: u64,
    utc: i64,
    valid: bool,
    bytes: [u8; 9],
    id: u64,
}
#[derive(Clone, Copy)]
struct Receipt {
    item: Item,
    id: u64,
}
struct Pending<'a> {
    item: Item,
    token: JobToken<'a>,
}
/// A boot-owned cache/producer borrowing exactly one real async history owner.
/// No medium access occurs in App/LATEST callbacks; only `step` advances accepted work.
pub struct HealthLog<'a, M: AsyncMedium, L: AsyncMedium, C: HistoryClock> {
    owner: &'a HealthHistory<M, L, C>,
    cache: RefCell<[Option<Cached>; 3]>,
    receipts: RefCell<[Option<Receipt>; 9]>,
    pending: RefCell<Option<Pending<'a>>>,
    poisoned: Cell<bool>,
    ui_counter: Cell<u64>,
    ui_pending: Cell<[u64; 3]>,
    ui_cancelled: Cell<[u64; 3]>,
    view_generation: Cell<u64>,
}
impl<M: AsyncMedium, L: AsyncMedium, C: HistoryClock> core::fmt::Debug for HealthLog<'_, M, L, C> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HealthLog")
            .field("poisoned", &self.poisoned.get())
            .finish_non_exhaustive()
    }
}
struct BootRead;
impl HistoryAccess for BootRead {
    fn module_allowed(&self) -> bool {
        true
    }
    fn allowed(&self, _: [u8; 16], d: Option<u8>, op: HistoryOperation) -> bool {
        matches!(op, HistoryOperation::Read | HistoryOperation::Enumerate)
            && d.is_none_or(|d| DOMAINS.contains(&d))
    }
}
impl<'a, M: AsyncMedium, L: AsyncMedium, C: HistoryClock> HealthLog<'a, M, L, C> {
    /// Rebuild small caches from actual recovered committed records before exposing services.
    /// Each wait invokes caller progress and one actual async-owner slice; no sync Flash wrapper.
    /// # Errors
    /// Unrecovered/corrupt logs, wrong directories, clock/lease errors or actual I/O failures fail.
    pub fn restore(
        owner: &'a HealthHistory<M, L, C>,
        now: impl Fn() -> Result<u64, HistoryFailure>,
        mut progress: impl FnMut(),
    ) -> Result<Self, Error> {
        let mut reader = owner.adapter().map_err(error)?;
        if reader
            .directory()
            .map_err(error)?
            .domains
            .iter()
            .map(|d| d.domain)
            .ne(DOMAINS)
        {
            return Err(Error::Invalid);
        }
        let mut cache = [None; 3];
        for (index, domain) in DOMAINS.into_iter().enumerate() {
            let snapshot = u32::try_from(index + 1).map_err(|_| Error::Invalid)?;
            let filter = HistoryFilter {
                domain,
                filter_flags: 0,
                start_utc_ms: 0,
                end_utc_ms: 0,
                record_type: 0,
                parent_session: 0,
            };
            let open = Preparation::Open { snapshot, filter };
            while !reader
                .prepare(open, now().map_err(error)?, &BootRead)
                .map_err(error)?
            {
                progress();
                owner.step().map_err(error)?;
            }
            let lease = reader
                .open(snapshot, filter, now().map_err(error)?, &BootRead)
                .map_err(error)?;
            let id = lease.snapshot.last_id;
            if id != 0 {
                let request = Preparation::Record { lease, id };
                while !reader
                    .prepare(request, now().map_err(error)?, &BootRead)
                    .map_err(error)?
                {
                    progress();
                    owner.step().map_err(error)?;
                }
                let mut bytes = [0; 9];
                let record = reader
                    .read(lease, id, &mut bytes, &BootRead)
                    .map_err(error)?;
                let module = module(domain).map_err(error)?;
                if record.length > 9 || record.schema != 2 || record.record_type != kind(module) {
                    return Err(Error::Backend);
                }
                SourcedResult::decode(module, &bytes[..record.length])?;
                cache[index] = Some(Cached {
                    ui_sequence: 0,
                    utc: record.utc_ms,
                    valid: record.flags & 1 != 0,
                    bytes,
                    id,
                });
            }
            reader.close(lease).map_err(error)?;
        }
        Ok(Self {
            owner,
            cache: RefCell::new(cache),
            receipts: RefCell::new([None; 9]),
            pending: RefCell::new(None),
            poisoned: Cell::new(false),
            ui_counter: Cell::new(0),
            ui_pending: Cell::new([0; 3]),
            ui_cancelled: Cell::new([0; 3]),
            view_generation: Cell::new(1),
        })
    }
    /// Drive one real background slice. Accepted work survives losing its App delivery path.
    /// # Errors
    /// Storage uncertainty blocks all reads/publication until owners are explicitly recovered/rebuilt.
    pub fn step(&self) -> Result<(), Error> {
        self.step_with_scratch(&mut [0; 256])
    }
    /// Advance accepted health work with the scheduler's shared transfer page.
    /// # Errors
    /// Same storage uncertainty and delivery failures as `step`.
    #[inline(never)]
    pub fn step_with_scratch(&self, scratch: &mut [u8; 256]) -> Result<(), Error> {
        let mut pending = self.pending.try_borrow_mut().map_err(|_| Error::Busy)?;
        if let Err(failure) = self.owner.step_with_scratch(scratch) {
            if failure != HistoryFailure::Busy {
                self.poisoned.set(true);
            }
            return Err(error(failure));
        }
        self.settle_pending(&mut pending)
    }
    // Publication is a separate CPU phase after the physical slice. Its cache
    // temporaries need not stay on the stack during Flash polling/verification.
    #[inline(never)]
    fn settle_pending(&self, pending: &mut Option<Pending<'a>>) -> Result<(), Error> {
        let Some(p) = pending.as_ref() else {
            return self.ready();
        };
        let result = self
            .owner
            .inspect_result(p.token, |r| match r {
                JobResult::Appended { id } => Some(*id),
                _ => None,
            })
            .map_err(|failure| {
                if failure != HistoryFailure::Busy {
                    self.poisoned.set(true);
                }
                error(failure)
            })?;
        match result {
            Poll::Pending => Ok(()),
            Poll::Ready(Ok(Some(id))) if id != 0 => {
                let index = index(p.item.module)?;
                // Borrow both tables before publishing either transition. Only
                // this module's entries are updated; no full table lives on stack.
                let mut cache = self.cache.try_borrow_mut().map_err(|_| Error::Busy)?;
                let mut receipts = self.receipts.try_borrow_mut().map_err(|_| Error::Busy)?;

                cache[index] = Some(Cached {
                    ui_sequence: if p.item.key.kind == KeyKind::Ui {
                        p.item.key.primary
                    } else {
                        0
                    },
                    utc: p.item.utc,
                    valid: p.item.valid,
                    bytes: p.item.bytes,
                    id,
                });

                receipts[receipt_index(p.item.key)?] = Some(Receipt { item: p.item, id });

                self.bump_view()?;
                *pending = None;
                Ok(())
            }
            Poll::Ready(result) => {
                self.poisoned.set(true);
                Err(match result {
                    Err(f) => error(f),
                    _ => Error::Backend,
                })
            }
        }
    }
    fn cache_at(&self, index: usize) -> Result<Option<Cached>, Error> {
        Ok(self.cache.try_borrow().map_err(|_| Error::Busy)?[index])
    }
    fn receipt_at(&self, index: usize) -> Result<Option<Receipt>, Error> {
        Ok(self.receipts.try_borrow().map_err(|_| Error::Busy)?[index])
    }
    pub(super) fn ready(&self) -> Result<(), Error> {
        if self.poisoned.get() {
            Err(Error::Backend)
        } else {
            self.owner.stats().map(|_| ()).map_err(|failure| {
                if failure != HistoryFailure::Busy {
                    self.poisoned.set(true);
                }
                error(failure)
            })
        }
    }
    fn import_item(key: ImportKey, request: HealthImport<'_>) -> Result<Item, Error> {
        request.validate()?;
        if key.connection == 0 || key.message == 0 || key.module != request.module {
            return Err(Error::Invalid);
        }
        let mut bytes = [0; 9];
        SourcedResult::new(request.module, request.value, ResultOrigin::AppImport)?
            .encode(&mut bytes)?;
        Ok(Item {
            key: Key::import(key),
            module: request.module,
            utc: request.timestamp.utc_ms(),
            valid: request.timestamp.is_valid(),
            bytes,
        })
    }
    fn begin(&self, item: Item) -> Result<bool, Error> {
        self.ready()?;
        let i = index(item.module)?;
        if let Some(receipt) = self.receipt_at(receipt_index(item.key)?)?
            && receipt.item.key == item.key
        {
            return if receipt.item == item {
                Ok(true)
            } else {
                Err(Error::Invalid)
            };
        }
        let mut pending = self.pending.try_borrow_mut().map_err(|_| Error::Busy)?;
        if let Some(p) = pending.as_ref() {
            return if p.item == item {
                Ok(false)
            } else if p.item.key == item.key {
                Err(Error::Invalid)
            } else {
                Err(Error::Busy)
            };
        }
        let owner: &'a HealthHistory<M, L, C> = self.owner;
        let token = owner
            .begin_append(AppendRecord {
                domain: DOMAINS[i],
                record_type: kind(item.module),
                schema: 2,
                utc_ms: item.utc,
                flags: u16::from(item.valid),
                parent_session: 0,
                payload: &item.bytes[..body_length(item.module)?],
            })
            .map_err(error)?;
        *pending = Some(Pending { item, token });
        Ok(false)
    }
    /// Borrow a checked LATEST/publish view over this same log owner.
    #[must_use]
    pub const fn latest(&self) -> LatestView<'_, 'a, M, L, C> {
        LatestView(self)
    }
}
impl<M: AsyncMedium, L: AsyncMedium, C: HistoryClock> HealthImportPort for HealthLog<'_, M, L, C> {
    fn available(&self, module: u8) -> bool {
        self.ready().is_ok() && index(module).is_ok()
    }
    fn prepare(&self, key: ImportKey, r: HealthImport<'_>) -> Result<bool, Error> {
        self.begin(Self::import_item(key, r)?)
    }
    fn receipt(&self, key: ImportKey, r: HealthImport<'_>) -> Result<u64, Error> {
        self.ready()?;
        let item = Self::import_item(key, r)?;
        let receipt = self
            .receipt_at(receipt_index(Key::import(key))?)?
            .ok_or(Error::Busy)?;
        if receipt.item != item {
            return Err(Error::Busy);
        }
        Ok(receipt.id)
    }
    fn current(&self, key: ImportKey, id: u64) -> bool {
        self.ready().is_ok()
            && receipt_index(Key::import(key))
                .ok()
                .and_then(|i| self.receipt_at(i).ok().flatten())
                .is_some_and(|r| r.item.key == Key::import(key) && r.id == id)
    }
    fn detach(&self, _: ImportKey) {} // No connection-borrowed pointer/delivery is retained; accepted work is device-owned.
}
/// LATEST port sharing the same durable source of truth and bounded cache.
pub struct LatestView<'b, 'a, M: AsyncMedium, L: AsyncMedium, C: HistoryClock>(
    &'b HealthLog<'a, M, L, C>,
);
impl<M: AsyncMedium, L: AsyncMedium, C: HistoryClock> core::fmt::Debug
    for LatestView<'_, '_, M, L, C>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LatestView").finish_non_exhaustive()
    }
}
impl<M: AsyncMedium, L: AsyncMedium, C: HistoryClock> LatestStore for LatestView<'_, '_, M, L, C> {
    fn load(&self, module: u8, record_type: u16, out: &mut [u8]) -> Result<StoredLatest, Error> {
        self.0.ready()?;
        if kind(module) != record_type {
            return Err(Error::Invalid);
        }
        let cached = self
            .0
            .cache_at(index(module)?)?
            .ok_or(Error::Health(device_health::Error::NotFound))?;
        let info = result_info(module, cached.id, cached.utc, cached.valid, &cached.bytes)?;
        if out.len() < info.length {
            return Err(Error::Capacity);
        }
        out[..info.length].copy_from_slice(&cached.bytes[..info.length]);
        Ok(StoredLatest {
            info,
            generation: cached.id,
        })
    }
    fn publish(
        &mut self,
        module: u8,
        job: u32,
        info: ResultInfo,
        bytes: &[u8],
    ) -> Result<(), Error> {
        if job == 0
            || info.sample_id == 0
            || info.schema != 2
            || info.record_type != kind(module)
            || bytes.len() != info.length
            || bytes.len() > 9
        {
            return Err(Error::Invalid);
        }
        let value = SourcedResult::decode(module, bytes)?;
        if value.origin().source() != info.source {
            return Err(Error::Invalid);
        }
        let mut body = [0; 9];
        body[..bytes.len()].copy_from_slice(bytes);
        let item = Item {
            module,
            key: Key::local(module, job, info.sample_id),
            utc: info.timestamp.utc_ms(),
            valid: info.timestamp.is_valid(),
            bytes: body,
        };
        if self.0.begin(item)? {
            Ok(())
        } else {
            Err(Error::Busy)
        }
    }
    fn current(&self, module: u8, record_type: u16, generation: u64) -> bool {
        self.0.ready().is_ok()
            && kind(module) == record_type
            && index(module)
                .ok()
                .and_then(|i| self.0.cache_at(i).ok().flatten())
                .is_some_and(|r| r.id == generation)
    }
}
fn body_length(module: u8) -> Result<usize, Error> {
    match module {
        0x25 => Ok(9),
        0x26 => Ok(7),
        0x27 => Ok(4),
        _ => Err(Error::Invalid),
    }
}
fn result_info(
    module: u8,
    id: u64,
    utc: i64,
    valid: bool,
    bytes: &[u8; 9],
) -> Result<ResultInfo, Error> {
    let length = body_length(module)?;
    let value = SourcedResult::decode(module, &bytes[..length])?;
    Ok(ResultInfo {
        sample_id: id,
        record_type: kind(module),
        schema: 2,
        timestamp: Timestamp::new(utc, valid)?,
        length,
        source: value.origin().source(),
    })
}
fn index(module: u8) -> Result<usize, Error> {
    match module {
        0x25 => Ok(0),
        0x26 => Ok(1),
        0x27 => Ok(2),
        _ => Err(Error::Invalid),
    }
}
fn module(domain: u8) -> Result<u8, HistoryFailure> {
    match domain {
        3 => Ok(0x25),
        4 => Ok(0x26),
        8 => Ok(0x27),
        _ => Err(HistoryFailure::Invalid),
    }
}
const fn kind(module: u8) -> u16 {
    match module {
        0x25 => 0x2501,
        0x26 => 0x2601,
        0x27 => 0x2701,
        _ => 0,
    }
}
fn error(error: HistoryFailure) -> Error {
    match error {
        HistoryFailure::Busy => Error::Busy,
        HistoryFailure::Capacity => Error::Capacity,
        HistoryFailure::Denied => Error::Denied,
        HistoryFailure::Invalid => Error::Invalid,
        _ => Error::Backend,
    }
}

fn receipt_index(key: Key) -> Result<usize, Error> {
    index(key.module).map(|n| n * 3 + key.kind as usize)
}
impl<M: AsyncMedium, L: AsyncMedium, C: HistoryClock> HealthLog<'_, M, L, C> {
    fn bump_view(&self) -> Result<(), Error> {
        let Some(next) = self.view_generation.get().checked_add(1) else {
            self.poisoned.set(true);
            return Err(Error::Capacity);
        };
        self.view_generation.set(next);
        Ok(())
    }
    pub(super) fn view_generation(&self) -> u64 {
        self.view_generation.get()
    }
    pub(super) fn start_ui(
        &self,
        module: u8,
        inputs: super::LegacyInputs,
        timestamp: Timestamp,
    ) -> Result<(), Error> {
        self.ready()?;
        let i = index(module)?;
        let sequence = self.ui_pending.get()[i];
        if sequence != 0
            && !self
                .receipt_at(i * 3 + 2)?
                .is_some_and(|r| r.item.key == Key::ui(module, sequence))
        {
            return Err(Error::Busy);
        }
        let sequence = self
            .ui_counter
            .get()
            .checked_add(1)
            .ok_or(Error::Capacity)?;
        if self.view_generation.get() > u64::MAX - 3 {
            return Err(Error::Capacity);
        }
        let mut bytes = [0; 9];
        super::calculate(module, inputs, &mut bytes)?;
        self.begin(Item {
            key: Key::ui(module, sequence),
            module,
            utc: timestamp.utc_ms(),
            valid: timestamp.is_valid(),
            bytes,
        })?;
        self.ui_counter.set(sequence);
        let mut state = self.ui_pending.get();
        state[i] = sequence;
        self.ui_pending.set(state);
        let mut cancelled = self.ui_cancelled.get();
        cancelled[i] = 0;
        self.ui_cancelled.set(cancelled);
        self.bump_view()
    }
    pub(super) fn stop_ui(&self, module: u8) -> Result<(), Error> {
        self.ready()?;
        let i = index(module)?;
        let mut cancelled = self.ui_cancelled.get();
        cancelled[i] = self.ui_pending.get()[i];
        self.ui_cancelled.set(cancelled);
        self.bump_view()
    }
    /// Current page state over actual accepted work and committed values; no source label.
    /// # Errors
    /// Unsupported page kinds fail explicitly.
    pub fn page(
        &self,
        kind: wattch5_parity_ui_demo::MeasureKind,
    ) -> Result<wattch5_parity_ui_demo::MeasurementSnapshot, Error> {
        use wattch5_parity_ui_demo::{
            MeasureState as State, MeasurementSnapshot as Snapshot, Reading,
        };
        let module = super::ui::module(kind)?;
        let i = index(module)?;
        if self.ready().is_err() {
            return Ok(Snapshot {
                state: State::Error,
                reading: Reading::Missing,
            });
        }
        let sequence = self.ui_pending.get()[i];
        if sequence != 0 {
            if self.ui_cancelled.get()[i] == sequence {
                if self.cache_at(i)?.is_none_or(|r| r.ui_sequence == sequence) {
                    return Ok(Snapshot::default());
                }
            } else if !self
                .receipt_at(i * 3 + 2)?
                .is_some_and(|r| r.item.key == Key::ui(module, sequence))
            {
                return Ok(Snapshot {
                    state: State::Pending,
                    reading: Reading::Missing,
                });
            }
        }
        self.cache_at(i)?.map_or(Ok(Snapshot::default()), |r| {
            super::measurement(module, &r.bytes[..body_length(module)?]).map_err(Error::from)
        })
    }
}
