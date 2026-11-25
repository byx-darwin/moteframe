//! Heap-free synchronous append-only history; ACK never reclaims records.
pub(crate) mod format;
mod media;
mod service;
use crate::{AppendRecord, DomainConfig, HistoryClock, HistorySchema, Record};
use core::cell::{Cell, RefCell};
use device_history_service::{HistoryFailure, StoreLease};
use device_protocol::{HistoryDomain, HistoryFilter};
use device_storage::persistence::{Geometry, Medium, SnapshotStore};
pub use media::{MediaLayout, MediaView, PagedMedia};
/// Explicit interpretation and physical namespace of an append-only log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogConfig {
    /// Nonzero caller-owned namespace.
    pub namespace: [u8; 16],
    /// Absolute physical arena start; must match the view grant.
    pub log_start: u32,
    /// Independent physical slots for each configured domain.
    pub slots_per_domain: u32,
}
/// Exact fixed slot size, including isolated marker unit.
/// # Errors
/// Rejects unsupported body, geometry and arithmetic overflow.
pub const fn slot_bytes<const B: usize>(geometry: Geometry) -> Result<u32, HistoryFailure> {
    let w = geometry.write_size;
    if B == 0 || B > 65535 || w == 0 || w > 256 {
        return Err(HistoryFailure::Invalid);
    }
    let bytes = B.to_le_bytes();
    let body = u32::from_le_bytes([bytes[0], bytes[1], 0, 0]);
    let rounded = (80 + body).div_ceil(w) * w;
    match rounded.checked_add(w) {
        Some(n) => Ok(n),
        None => Err(HistoryFailure::Invalid),
    }
}
/// Maximum metadata payload bytes for D configured domains.
#[must_use]
pub const fn metadata_bytes<const D: usize>() -> usize {
    80_usize.saturating_add(D.saturating_mul(64))
}
#[derive(Clone, Copy)]
pub(crate) struct State<const D: usize> {
    pub(crate) dataset: [u8; 16],
    pub(crate) domains: [HistoryDomain; D],
    pub(crate) len: usize,
    pub(crate) ack: [u64; D],
    pub(crate) count: [u32; D],
    pub(crate) tail: [u32; D],
}
impl<const D: usize> State<D> {
    pub(crate) const EMPTY: Self = Self {
        dataset: [0; 16],
        domains: [HistoryDomain {
            domain: 0,
            epoch: 0,
            oldest_id: 0,
            newest_id: 0,
        }; D],
        len: 0,
        ack: [0; D],
        count: [0; D],
        tail: [0; D],
    };
    pub(crate) fn domain(&self, key: u8) -> Result<usize, HistoryFailure> {
        self.domains[..self.len]
            .iter()
            .position(|d| d.domain == key)
            .ok_or(HistoryFailure::NotFound)
    }
}
#[derive(Clone, Copy)]
pub(crate) struct Lease {
    pub(crate) public: StoreLease,
    pub(crate) filter: HistoryFilter,
    pub(crate) cut: u32,
    pub(crate) count: usize,
    pub(crate) admitted: [u8; 32],
    pub(crate) covered: usize,
    pub(crate) deadline: u64,
}
/// O(D) directory/scan state and a single 32-byte lease admission bitmap.
/// R is a per-domain quota (1..=255), B the complete body bound, N metadata bound.
pub struct PagedHistoryStore<
    S,
    L,
    V,
    C,
    const D: usize,
    const R: usize,
    const B: usize,
    const N: usize,
> {
    snapshot: S,
    log: RefCell<L>,
    schema: V,
    clock: C,
    config: LogConfig,
    geometry: Geometry,
    stride: u32,
    data: State<D>,
    revision: u64,
    metadata_crc: u32,
    metadata_length: usize,
    poisoned: Cell<bool>,
    lease: Cell<Option<Lease>>,
    released: Cell<Option<StoreLease>>,
    next_handle: u64,
    lease_s: u16,
    actual_time: Cell<Option<u64>>,
    supplied_time: Cell<Option<u64>>,
}
impl<S, L, V, C, const D: usize, const R: usize, const B: usize, const N: usize> core::fmt::Debug
    for PagedHistoryStore<S, L, V, C, D, R, B, N>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PagedHistoryStore")
            .field("domains", &self.data.len)
            .field("revision", &self.revision)
            .field("poisoned", &self.poisoned.get())
            .finish_non_exhaustive()
    }
}
/// Retention and finite physical reserve accounting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogStats<const D: usize> {
    /// Records per configured domain ordinal.
    pub retained: [u32; D],
    /// Consumed physical prefix per configured domain ordinal.
    pub consumed: [u32; D],
    /// Physical slots independently available per domain.
    pub slots_per_domain: u32,
    /// Physical bytes per slot.
    pub slot_bytes: u32,
}
impl<
    S: SnapshotStore,
    L: Medium,
    V: HistorySchema,
    C: HistoryClock,
    const D: usize,
    const R: usize,
    const B: usize,
    const N: usize,
> PagedHistoryStore<S, L, V, C, D, R, B, N>
{
    fn empty(
        snapshot: S,
        log: L,
        schema: V,
        clock: C,
        config: LogConfig,
        lease_s: u16,
    ) -> Result<Self, HistoryFailure> {
        let geometry = log.geometry();
        let stride = slot_bytes::<B>(geometry)?;
        if D == 0
            || D > 255
            || R == 0
            || R > 255
            || N < metadata_bytes::<D>()
            || lease_s == 0
            || config.namespace == [0; 16]
            || config.slots_per_domain < to_u32(R)?
            || geometry.erase_size == 0
            || !geometry.erase_size.is_multiple_of(geometry.write_size)
            || !config.log_start.is_multiple_of(geometry.write_size)
        {
            return Err(HistoryFailure::Invalid);
        }
        Ok(Self {
            snapshot,
            log: RefCell::new(log),
            schema,
            clock,
            config,
            geometry,
            stride,
            data: State::EMPTY,
            revision: 0,
            metadata_crc: 0,
            metadata_length: 0,
            poisoned: Cell::new(true),
            lease: Cell::new(None),
            released: Cell::new(None),
            next_handle: 1,
            lease_s,
            actual_time: Cell::new(None),
            supplied_time: Cell::new(None),
        })
    }
    /// Open initialized metadata and completely validate the settled log.
    /// # Errors
    /// Blank, corrupt, mismatched configuration or unavailable storage fails closed.
    pub fn open(
        snapshot: S,
        log: L,
        schema: V,
        clock: C,
        config: LogConfig,
        lease_s: u16,
    ) -> Result<Self, HistoryFailure> {
        let mut s = Self::empty(snapshot, log, schema, clock, config, lease_s)?;
        s.recover()?;
        Ok(s)
    }
    /// Initialize only explicit identities and an entirely blank log and snapshot.
    /// # Errors
    /// Invalid identities, existing snapshot, dirty log or uncertain I/O is rejected.
    #[allow(clippy::too_many_arguments)] // Explicit ownership, validators, layout and identities are separate inputs.
    pub fn initialize(
        snapshot: S,
        log: L,
        schema: V,
        clock: C,
        config: LogConfig,
        dataset: [u8; 16],
        domains: &[DomainConfig],
        lease_s: u16,
    ) -> Result<Self, HistoryFailure> {
        let mut s = Self::empty(snapshot, log, schema, clock, config, lease_s)?;
        if dataset == [0; 16] || domains.is_empty() || domains.len() > D {
            return Err(HistoryFailure::Invalid);
        }
        s.data.dataset = dataset;
        s.data.len = domains.len();
        for (i, d) in domains.iter().enumerate() {
            if d.domain == 255 || d.epoch == 0 || i > 0 && domains[i - 1].domain >= d.domain {
                return Err(HistoryFailure::Invalid);
            }
            s.data.domains[i].domain = d.domain;
            s.data.domains[i].epoch = d.epoch;
        }
        s.check_space()?;
        s.log.get_mut().sync().map_err(storage)?;
        let mut b = [0; N];
        if s.snapshot.load(&mut b).map_err(storage)?.is_some() {
            return Err(HistoryFailure::Conflict);
        }
        if !s.ff(0, s.geometry.capacity)? {
            return Err(HistoryFailure::StoredIntegrity);
        }
        s.persist(s.data)?;
        s.poisoned.set(false);
        Ok(s)
    }
    /// Release both views; uncertain state must still be recovered by the next owner.
    pub fn into_parts(self) -> (S, L) {
        (self.snapshot, self.log.into_inner())
    }
    /// Return separate retained and consumed physical counts.
    /// # Errors
    /// Uncertainty requires recovery.
    pub fn stats(&self) -> Result<LogStats<D>, HistoryFailure> {
        self.ready()?;
        Ok(LogStats {
            retained: self.data.count,
            consumed: self.data.tail,
            slots_per_domain: self.config.slots_per_domain,
            slot_bytes: self.stride,
        })
    }
    fn check_space(&self) -> Result<(), HistoryFailure> {
        let n = self
            .config
            .slots_per_domain
            .checked_mul(self.stride)
            .and_then(|n| n.checked_mul(u32::try_from(self.data.len).ok()?))
            .ok_or(HistoryFailure::Invalid)?;
        if n > self.geometry.capacity {
            return Err(HistoryFailure::Capacity);
        }
        Ok(())
    }
    fn release(&self) {
        if let Some(l) = self.lease.replace(None) {
            self.released.set(Some(l.public));
        }
    }
    fn corrupt(&self) -> HistoryFailure {
        self.poisoned.set(true);
        self.release();
        HistoryFailure::StoredIntegrity
    }
    fn io(&self) -> HistoryFailure {
        self.poisoned.set(true);
        self.release();
        HistoryFailure::Storage
    }
    fn ready(&self) -> Result<(), HistoryFailure> {
        if self.poisoned.get() {
            Err(HistoryFailure::Storage)
        } else {
            Ok(())
        }
    }
    fn read_bytes(&self, at: u32, out: &mut [u8]) -> Result<(), HistoryFailure> {
        let mut m = self
            .log
            .try_borrow_mut()
            .map_err(|_| HistoryFailure::Busy)?;
        for (i, c) in out.chunks_mut(256).enumerate() {
            m.read(at + to_u32(i * 256)?, c).map_err(|_| self.io())?;
        }
        Ok(())
    }
    fn ff(&self, at: u32, n: u32) -> Result<bool, HistoryFailure> {
        let mut b = [0; 256];
        let mut pos = 0;
        while pos < n {
            let len = (n - pos).min(256) as usize;
            self.read_bytes(at + pos, &mut b[..len])?;
            if b[..len].iter().any(|b| *b != 255) {
                return Ok(false);
            }
            pos += to_u32(len)?;
        }
        Ok(true)
    }
    fn offset(&self, d: usize, slot: u32) -> Result<u32, HistoryFailure> {
        to_u32(d)?
            .checked_mul(self.config.slots_per_domain)
            .and_then(|n| n.checked_add(slot))
            .and_then(|n| n.checked_mul(self.stride))
            .ok_or(HistoryFailure::Invalid)
    }
    /// Settle all storage, validate every committed record, revoke leases and retain floors.
    /// # Errors
    /// Missing/corrupt metadata, rollback, malformed committed slots or I/O remains poisoned.
    pub fn recover(&mut self) -> Result<(), HistoryFailure> {
        let confirmed = self.data;
        let result = self.recover_inner();
        if result.is_err() {
            self.data = confirmed;
        }
        result
    }
    fn recover_inner(&mut self) -> Result<(), HistoryFailure> {
        self.poisoned.set(true);
        self.release();
        self.log.get_mut().sync().map_err(storage)?;
        self.snapshot.recover().map_err(storage)?;
        let mut b = [0; N];
        let meta = self
            .snapshot
            .load(&mut b)
            .map_err(storage)?
            .ok_or(if self.revision == 0 {
                HistoryFailure::NotFound
            } else {
                HistoryFailure::StoredIntegrity
            })?;
        if meta.revision == 0 || meta.revision < self.revision || meta.length > N {
            return Err(HistoryFailure::StoredIntegrity);
        }
        let root = metadata_crc(&b[..meta.length]);
        if meta.revision == self.revision
            && (root != self.metadata_crc || meta.length != self.metadata_length)
        {
            return Err(HistoryFailure::StoredIntegrity);
        }
        let checkpoint = self.decode(&b[..meta.length])?;
        if self.revision != 0
            && (checkpoint.dataset != self.data.dataset
                || checkpoint.len != self.data.len
                || checkpoint.domains[..checkpoint.len]
                    .iter()
                    .zip(&self.data.domains)
                    .any(|(a, b)| a.domain != b.domain || a.epoch != b.epoch))
        {
            return Err(HistoryFailure::StoredIntegrity);
        }
        let old = self.data;
        self.data = checkpoint;
        self.check_space()?;
        let mut next = checkpoint;
        next.count = [0; D];
        next.tail = [0; D];
        for d in 0..next.len {
            next.domains[d].oldest_id = 0;
            next.domains[d].newest_id = 0;
            let mut ack_found = next.ack[d] == 0;
            let mut prefix = State::<1>::EMPTY;
            for slot in 0..self.config.slots_per_domain {
                let at = self.offset(d, slot)?;
                if !self.ff(at, self.stride)? {
                    next.tail[d] = slot + 1;
                }
                if let Some(r) = self.load_record(d, slot)? {
                    if r.id <= next.domains[d].newest_id || next.count[d] >= to_u32(R)? {
                        return Err(HistoryFailure::StoredIntegrity);
                    }
                    next.domains[d].newest_id = r.id;
                    next.count[d] += 1;
                    if next.domains[d].oldest_id == 0 {
                        next.domains[d].oldest_id = r.id;
                    }
                    next.domains[d].newest_id = r.id;
                    ack_found |= r.id == next.ack[d];
                    if slot < checkpoint.tail[d] {
                        prefix.count[0] += 1;
                        if prefix.domains[0].oldest_id == 0 {
                            prefix.domains[0].oldest_id = r.id;
                        }
                        prefix.domains[0].newest_id = r.id;
                    }
                }
            }
            if !ack_found
                || next.ack[d] > next.domains[d].newest_id
                || checkpoint.tail[d] > next.tail[d]
                || prefix.count[0] != checkpoint.count[d]
                || prefix.domains[0].oldest_id != checkpoint.domains[d].oldest_id
                || prefix.domains[0].newest_id != checkpoint.domains[d].newest_id
                || self.revision != 0
                    && (next.domains[d].newest_id < old.domains[d].newest_id
                        || next.ack[d] < old.ack[d]
                        || next.count[d] < old.count[d]
                        || next.tail[d] < old.tail[d])
            {
                return Err(HistoryFailure::StoredIntegrity);
            }
        }
        let used = self.config.slots_per_domain * self.stride * to_u32(next.len)?;
        if !self.ff(used, self.geometry.capacity - used)? {
            return Err(HistoryFailure::StoredIntegrity);
        }
        self.data = next;
        self.revision = meta.revision;
        self.metadata_crc = root;
        self.metadata_length = meta.length;
        self.poisoned.set(false);
        Ok(())
    }
    /// Commit one complete validated body, sync preparation then isolated marker, and read back.
    /// # Errors
    /// Full quota/reserve, invalid schema, exhausted IDs or uncertainty is explicit.
    pub fn append(&mut self, input: AppendRecord<'_>) -> Result<u64, HistoryFailure> {
        self.ready()?;
        let d = self.data.domain(input.domain)?;
        if input.payload.len() > B
            || self.data.count[d] >= to_u32(R)?
            || self.data.tail[d] >= self.config.slots_per_domain
        {
            return Err(HistoryFailure::Capacity);
        }
        let mut r = Record::<B>::EMPTY;
        r.domain = input.domain;
        r.epoch = self.data.domains[d].epoch;
        r.id = self.data.domains[d]
            .newest_id
            .checked_add(1)
            .ok_or(HistoryFailure::Capacity)?;
        r.kind = input.record_type;
        r.schema = input.schema;
        r.utc = input.utc_ms;
        r.flags = input.flags;
        r.parent = input.parent_session;
        r.len = input.payload.len();
        r.body[..r.len].copy_from_slice(input.payload);
        r.crc = r.stored().crc().map_err(|_| HistoryFailure::Invalid)?;
        self.schema.validate(r.stored(), r.parent)?;
        let slot = self.data.tail[d];
        let at = self.offset(d, slot)?;
        if !self.ff(at, self.stride)? {
            return Err(self.corrupt());
        }
        let header = self.header(&r, d, slot)?;
        let unit = self.geometry.write_size as usize;
        let mut scratch = [255; 256];
        let prep = self.stride - self.geometry.write_size;
        self.poisoned.set(true);
        let result = (|| {
            let m = self.log.get_mut();
            let mut pos = 0;
            while pos < prep {
                scratch[..unit].fill(255);
                for (i, b) in scratch[..unit].iter_mut().enumerate() {
                    let k = pos as usize + i;
                    if k < 80 {
                        *b = header[k];
                    } else if k - 80 < r.len {
                        *b = r.body[k - 80];
                    }
                }
                m.program(at + pos, &scratch[..unit]).map_err(storage)?;
                pos += self.geometry.write_size;
            }
            m.sync().map_err(storage)?;
            scratch[..unit].fill(0xa5);
            m.program(at + prep, &scratch[..unit]).map_err(storage)?;
            m.sync().map_err(storage)
        })();
        if let Err(e) = result {
            self.release();
            return Err(e);
        }
        match self.load_record(d, slot)? {
            Some(actual) if actual.stored() == r.stored() && actual.parent == r.parent => {}
            _ => return Err(self.corrupt()),
        }
        self.data.domains[d].newest_id = r.id;
        self.data.count[d] += 1;
        self.data.tail[d] += 1;
        if self.data.domains[d].oldest_id == 0 {
            self.data.domains[d].oldest_id = r.id;
        }
        self.data.domains[d].newest_id = r.id;
        self.poisoned.set(false);
        Ok(r.id)
    }
    fn persist(&mut self, next: State<D>) -> Result<(), HistoryFailure> {
        let mut b = [0; N];
        let len = self.encode(&next, &mut b)?;
        let rev = self
            .revision
            .checked_add(1)
            .ok_or(HistoryFailure::Capacity)?;
        match self.snapshot.commit(self.revision, &b[..len]) {
            Ok(meta) if meta.revision == rev && meta.length == len => {
                self.data = next;
                self.revision = rev;
                self.metadata_crc = metadata_crc(&b[..len]);
                self.metadata_length = len;
                Ok(())
            }
            Ok(_) => Err(self.corrupt()),
            Err(e) => {
                if !matches!(
                    e,
                    device_storage::persistence::Error::Capacity
                        | device_storage::persistence::Error::Conflict
                        | device_storage::persistence::Error::RevisionExhausted
                ) {
                    self.io();
                }
                Err(storage(e))
            }
        }
    }
}
fn storage(e: device_storage::persistence::Error) -> HistoryFailure {
    use device_storage::persistence::Error;
    match e {
        Error::Capacity | Error::RevisionExhausted => HistoryFailure::Capacity,
        Error::Conflict => HistoryFailure::Conflict,
        Error::Corrupt => HistoryFailure::StoredIntegrity,
        _ => HistoryFailure::Storage,
    }
}

fn to_u32(n: usize) -> Result<u32, HistoryFailure> {
    u32::try_from(n).map_err(|_| HistoryFailure::Invalid)
}
fn to_u16(n: usize) -> Result<u16, HistoryFailure> {
    u16::try_from(n).map_err(|_| HistoryFailure::Invalid)
}

fn metadata_crc(bytes: &[u8]) -> u32 {
    let mut c = device_protocol::Crc32c::new();
    c.update(bytes);
    c.finish()
}
