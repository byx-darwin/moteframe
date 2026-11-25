//! Durable bounded history and ACKs with resident snapshot or append-only paged storage.
//! The optional `paged` backend scans bodies from an exclusively owned logical medium.
//! Bodies and their parent relationships are checked by an injected actual schema.
#![no_std]
#![forbid(unsafe_code)]
#[cfg(test)]
extern crate self as device_history_store;
#[cfg(test)]
extern crate std;
/// Incremental asynchronous paged history and prepared service adapter.
pub mod async_paged;
mod image;
/// Optional synchronous append-only paged history backend.
pub mod paged;
mod repository;
mod service;
use core::cell::Cell;
use device_history_service::{HistoryFailure, StoreLease};
use device_protocol::{HistoryDomain, HistoryFilter};
use device_storage::StoredRecord;

/// Trusted actual domain body and parent relationship validator.
pub trait HistorySchema {
    /// Validate complete body, metadata and its actual parent relationship.
    /// # Errors
    /// Unknown schema or invalid body/relationship must be rejected explicitly.
    fn validate(&self, record: StoredRecord<'_>, parent_session: u64)
    -> Result<(), HistoryFailure>;
}
/// Trusted monotonic time source, independent of request contents.
pub trait HistoryClock {
    /// Read actual monotonic milliseconds.
    /// # Errors
    /// Unavailable time must fail explicitly; no synthetic fallback is permitted.
    fn now_ms(&self) -> Result<u64, HistoryFailure>;
}
/// Explicit initial persistent domain identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DomainConfig {
    /// Domain key, excluding reserved FF.
    pub domain: u8,
    /// Nonzero persistent domain epoch.
    pub epoch: u32,
}
/// Actual producer-owned content, before assigning its durable ID.
#[derive(Clone, Copy)]
pub struct AppendRecord<'a> {
    /// Existing configured domain.
    pub domain: u8,
    /// Nonzero actual record type.
    pub record_type: u16,
    /// Nonzero actual body schema.
    pub schema: u16,
    /// UTC milliseconds, zero when invalid.
    pub utc_ms: i64,
    /// Timestamp-valid bit0, all other bits reserved.
    pub flags: u16,
    /// Actual parent identity, validated by the supplied domain schema.
    pub parent_session: u64,
    /// Complete actual body, never truncated.
    pub payload: &'a [u8],
}
impl core::fmt::Debug for AppendRecord<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("AppendRecord")
            .field("length", &self.payload.len())
            .finish_non_exhaustive()
    }
}
#[derive(Clone, Copy)]
struct Record<const B: usize> {
    domain: u8,
    epoch: u32,
    id: u64,
    kind: u16,
    schema: u16,
    utc: i64,
    flags: u16,
    parent: u64,
    len: usize,
    body: [u8; B],
    crc: u32,
}
impl<const B: usize> Record<B> {
    const EMPTY: Self = Self {
        domain: 0,
        epoch: 0,
        id: 0,
        kind: 0,
        schema: 0,
        utc: 0,
        flags: 0,
        parent: 0,
        len: 0,
        body: [0; B],
        crc: 0,
    };
    fn stored(&self) -> StoredRecord<'_> {
        StoredRecord {
            domain: self.domain,
            epoch: self.epoch,
            record_id: self.id,
            record_type: self.kind,
            schema: self.schema,
            utc_ms: self.utc,
            flags: self.flags,
            payload: &self.body[..self.len],
        }
    }
}
#[derive(Clone, Copy)]
struct Data<const D: usize, const R: usize, const B: usize> {
    dataset: [u8; 16],
    domains: [HistoryDomain; D],
    domains_len: usize,
    high: [u64; D],
    ack: [u64; D],
    records: [Record<B>; R],
    records_len: usize,
}
impl<const D: usize, const R: usize, const B: usize> Data<D, R, B> {
    const EMPTY: Self = Self {
        dataset: [0; 16],
        domains: [HistoryDomain {
            domain: 0,
            epoch: 0,
            oldest_id: 0,
            newest_id: 0,
        }; D],
        domains_len: 0,
        high: [0; D],
        ack: [0; D],
        records: [Record::EMPTY; R],
        records_len: 0,
    };
    fn domain(&self, key: u8) -> Result<usize, HistoryFailure> {
        self.domains[..self.domains_len]
            .iter()
            .position(|d| d.domain == key)
            .ok_or(HistoryFailure::NotFound)
    }
}
#[derive(Clone, Copy)]
struct Lease<const R: usize> {
    public: StoreLease,
    filter: HistoryFilter,
    ids: [u64; R],
    count: usize,
    // Successful final-fragment admissions, independently of query cursor order.
    admitted: [bool; R],
    // Durable ACK coverage plus the contiguous prefix of actual admissions.
    covered: usize,
    deadline: u64,
}
/// Heap-free durable history owner. D bounds domains, R retained records, B body
/// bytes, and I the complete encoded image. Exactly one live retention lease is
/// supported. Acknowledged records remain retained until capacity is exhausted.
/// Reopening requires rebuilding all connection actors that used the old owner.
pub struct HistoryStore<S, V, C, const D: usize, const R: usize, const B: usize, const I: usize> {
    snapshot: S,
    schema: V,
    clock: C,
    data: Data<D, R, B>,
    revision: u64,
    poisoned: Cell<bool>,
    lease: Cell<Option<Lease<R>>>,
    released: Cell<Option<StoreLease>>,
    next_handle: u64,
    lease_s: u16,
    actual_time: Cell<Option<u64>>,
    supplied_time: Cell<Option<u64>>,
}
impl<S, V, C, const D: usize, const R: usize, const B: usize, const I: usize> core::fmt::Debug
    for HistoryStore<S, V, C, D, R, B, I>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HistoryStore")
            .field("domains", &self.data.domains_len)
            .field("records", &self.data.records_len)
            .field("revision", &self.revision)
            .field("poisoned", &self.poisoned.get())
            .finish_non_exhaustive()
    }
}
