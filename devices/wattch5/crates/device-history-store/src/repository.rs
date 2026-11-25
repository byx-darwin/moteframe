use crate::{
    AppendRecord, Data, DomainConfig, HistoryClock, HistorySchema, HistoryStore, Lease, Record,
    image,
};
use device_history_service::{HistoryAccess, HistoryFailure, HistoryOperation, StoreLease};
use device_protocol::{HistoryFilter, HistoryRequest};
use device_storage::persistence::{Error, SnapshotStore};

impl<
    S: SnapshotStore,
    V: HistorySchema,
    C: HistoryClock,
    const D: usize,
    const R: usize,
    const B: usize,
    const I: usize,
> HistoryStore<S, V, C, D, R, B, I>
{
    /// Open and strictly validate an initialized snapshot without formatting.
    /// # Errors
    /// Rejects blank, corrupt, unknown-schema, unavailable or invalid-capacity storage.
    pub fn open(snapshot: S, schema: V, clock: C, lease_s: u16) -> Result<Self, HistoryFailure> {
        let mut store = Self::empty(snapshot, schema, clock, lease_s)?;
        store.reload()?;
        Ok(store)
    }
    /// Initialize only confirmed-empty storage with explicitly supplied identities.
    /// # Errors
    /// Rejects existing data, zero/unsorted identities, capacity or persistence failures.
    pub fn initialize(
        snapshot: S,
        schema: V,
        clock: C,
        dataset: [u8; 16],
        domains: &[DomainConfig],
        lease_s: u16,
    ) -> Result<Self, HistoryFailure> {
        let mut store = Self::empty(snapshot, schema, clock, lease_s)?;
        let mut bytes = [0; I];
        if store.snapshot.load(&mut bytes).map_err(storage)?.is_some() {
            return Err(HistoryFailure::Conflict);
        }
        if dataset == [0; 16] || domains.is_empty() || domains.len() > D {
            return Err(HistoryFailure::Invalid);
        }
        let mut data = Data::EMPTY;
        data.dataset = dataset;
        data.domains_len = domains.len();
        for (i, d) in domains.iter().enumerate() {
            if d.domain == 255 || d.epoch == 0 || i > 0 && domains[i - 1].domain >= d.domain {
                return Err(HistoryFailure::Invalid);
            }
            data.domains[i].domain = d.domain;
            data.domains[i].epoch = d.epoch;
        }
        store.persist(data)?;
        Ok(store)
    }
    fn empty(snapshot: S, schema: V, clock: C, lease_s: u16) -> Result<Self, HistoryFailure> {
        if D == 0
            || D > 255
            || R == 0
            || R > u32::MAX as usize
            || B == 0
            || B > usize::from(u16::MAX)
            || I == 0
            || lease_s == 0
        {
            return Err(HistoryFailure::Invalid);
        }
        Ok(Self {
            snapshot,
            schema,
            clock,
            data: Data::EMPTY,
            revision: 0,
            poisoned: core::cell::Cell::new(false),
            lease: core::cell::Cell::new(None),
            released: core::cell::Cell::new(None),
            next_handle: 1,
            lease_s,
            actual_time: core::cell::Cell::new(None),
            supplied_time: core::cell::Cell::new(None),
        })
    }
    /// Release the exclusively owned snapshot, including after uncertain writes.
    pub fn into_snapshot(self) -> S {
        self.snapshot
    }
    /// Recover storage and clear all leases and volatile admission evidence.
    /// Handle numbering and trusted clock history remain nonreused in this owner.
    /// # Errors
    /// Any medium, image, record CRC or real schema error keeps the owner poisoned.
    pub fn recover(&mut self) -> Result<(), HistoryFailure> {
        self.poisoned.set(true);
        self.release();
        self.snapshot.recover().map_err(storage)?;
        self.reload()
    }
    fn reload(&mut self) -> Result<(), HistoryFailure> {
        self.poisoned.set(true);
        self.release();
        let mut bytes = [0; I];
        let meta =
            self.snapshot
                .load(&mut bytes)
                .map_err(storage)?
                .ok_or(if self.revision == 0 {
                    HistoryFailure::NotFound
                } else {
                    HistoryFailure::StoredIntegrity
                })?;
        if meta.revision == 0 || meta.length > I || meta.revision < self.revision {
            return Err(HistoryFailure::StoredIntegrity);
        }
        let data = image::decode(&bytes[..meta.length], &self.schema)?;
        self.data = data;
        self.revision = meta.revision;
        self.poisoned.set(false);
        Ok(())
    }
    /// Append actual validated producer content and publish its ID atomically.
    /// Calls are serialized by ownership. Appending changes directory rows and
    /// requires reconstructing existing `HistoryHandler` connection actors.
    /// # Errors
    /// Invalid schema, full retention/image capacity, exhausted IDs or failed writes.
    pub fn append(&mut self, input: AppendRecord<'_>) -> Result<u64, HistoryFailure> {
        self.ready()?;
        let d = self.data.domain(input.domain)?;
        if input.payload.len() > B {
            return Err(HistoryFailure::Capacity);
        }
        if self.data.records_len == R {
            return Err(HistoryFailure::Capacity);
        }
        let id = self.data.high[d]
            .checked_add(1)
            .ok_or(HistoryFailure::Capacity)?;
        let mut r = Record::EMPTY;
        r.domain = input.domain;
        r.epoch = self.data.domains[d].epoch;
        r.id = id;
        r.kind = input.record_type;
        r.schema = input.schema;
        r.utc = input.utc_ms;
        r.flags = input.flags;
        r.parent = input.parent_session;
        r.len = input.payload.len();
        r.body[..r.len].copy_from_slice(input.payload);
        r.crc = r.stored().crc().map_err(|_| HistoryFailure::Invalid)?;
        self.schema.validate(r.stored(), r.parent)?;
        let mut next = self.data;
        let at = next.records[..next.records_len]
            .partition_point(|old| (old.domain, old.id) < (r.domain, r.id));
        next.records.copy_within(at..next.records_len, at + 1);
        next.records[at] = r;
        next.records_len += 1;
        next.high[d] = id;
        next.domains[d].newest_id = id;
        if next.domains[d].oldest_id == 0 {
            next.domains[d].oldest_id = id;
        }
        self.persist(next)?;
        Ok(id)
    }
    pub(crate) fn persist(&mut self, next: Data<D, R, B>) -> Result<(), HistoryFailure> {
        let mut bytes = [0; I];
        let len = image::encode(&next, &mut bytes)?;
        let expected = self
            .revision
            .checked_add(1)
            .ok_or(HistoryFailure::Capacity)?;
        match self.snapshot.commit(self.revision, &bytes[..len]) {
            Ok(meta) if meta.revision == expected && meta.length == len => {
                self.data = next;
                self.revision = meta.revision;
                Ok(())
            }
            Ok(_) => {
                self.poisoned.set(true);
                self.release();
                Err(HistoryFailure::StoredIntegrity)
            }
            Err(error) => {
                if !matches!(
                    error,
                    Error::Capacity | Error::Conflict | Error::RevisionExhausted
                ) {
                    self.poisoned.set(true);
                    self.release();
                }
                Err(storage(error))
            }
        }
    }
    pub(crate) fn release(&self) {
        if let Some(lease) = self.lease.replace(None) {
            self.released.set(Some(lease.public));
        }
    }
    pub(crate) fn corrupt(&self) -> HistoryFailure {
        self.poisoned.set(true);
        self.release();
        HistoryFailure::StoredIntegrity
    }
    pub(crate) fn ready(&self) -> Result<(), HistoryFailure> {
        if self.poisoned.get() {
            Err(HistoryFailure::Storage)
        } else {
            Ok(())
        }
    }
    pub(crate) fn authorize(
        &self,
        domain: u8,
        op: HistoryOperation,
        access: &dyn HistoryAccess,
    ) -> Result<(), HistoryFailure> {
        if access.module_allowed()
            && access.allowed(self.data.dataset, None, op)
            && access.allowed(self.data.dataset, Some(domain), op)
        {
            Ok(())
        } else {
            Err(HistoryFailure::Denied)
        }
    }
    pub(crate) fn observe(&self, supplied: Option<u64>) -> Result<u64, HistoryFailure> {
        let actual = match self.clock.now_ms() {
            Ok(time) => time,
            Err(error) => {
                self.release();
                return Err(error);
            }
        };
        if self.actual_time.get().is_some_and(|old| actual < old) {
            self.release();
            return Err(HistoryFailure::Clock);
        }
        self.actual_time.set(Some(actual));
        if supplied.is_some_and(|now| {
            now > actual || self.supplied_time.get().is_some_and(|old| now < old)
        }) {
            self.release();
            return Err(HistoryFailure::Clock);
        }
        if let Some(time) = supplied {
            self.supplied_time.set(Some(time));
        }
        Ok(actual)
    }
    pub(crate) fn bound(
        &self,
        public: StoreLease,
        supplied: Option<u64>,
        access: Option<&dyn HistoryAccess>,
    ) -> Result<Lease<R>, HistoryFailure> {
        self.ready()?;
        let now = self.observe(supplied)?;
        let lease = self.lease.get().ok_or(HistoryFailure::CursorExpired)?;
        if public != lease.public {
            return Err(HistoryFailure::Identity);
        }
        if now >= lease.deadline {
            self.release();
            return Err(HistoryFailure::CursorExpired);
        }
        if let Some(access) = access {
            self.authorize(lease.filter.domain, HistoryOperation::Read, access)?;
        }
        Ok(lease)
    }
    pub(crate) fn check_filter(
        &self,
        filter: HistoryFilter,
        access: &dyn HistoryAccess,
    ) -> Result<usize, HistoryFailure> {
        self.ready()?;
        let mut bytes = [0; 28];
        HistoryRequest::OpenV2(filter)
            .encode(&mut bytes)
            .map_err(|_| HistoryFailure::Invalid)?;
        self.authorize(filter.domain, HistoryOperation::Read, access)?;
        self.data.domain(filter.domain)
    }
}
pub(crate) fn matches<const B: usize>(r: &Record<B>, f: HistoryFilter) -> bool {
    r.domain == f.domain
        && (f.filter_flags & 1 == 0
            || r.flags & 1 != 0 && r.utc >= f.start_utc_ms && r.utc < f.end_utc_ms)
        && (f.filter_flags & 2 == 0 || r.kind == f.record_type)
        && (f.filter_flags & 4 == 0 || r.parent == f.parent_session)
}
fn storage(error: Error) -> HistoryFailure {
    match error {
        Error::Capacity | Error::RevisionExhausted => HistoryFailure::Capacity,
        Error::Conflict => HistoryFailure::Conflict,
        Error::Corrupt => HistoryFailure::StoredIntegrity,
        _ => HistoryFailure::Storage,
    }
}
