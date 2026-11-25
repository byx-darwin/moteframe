//! Strict history command, response and event codecs for wire 1.2.
//!
//! These types neither authorize access nor open a snapshot, schedule a stream,
//! confirm phone persistence or advance a durable ACK. Record payload schemas
//! remain domain-owned. Responses include exactly one common status prefix;
//! RECORD and DONE are events and do not have that prefix.

use crate::{
    ProtocolError,
    bytes::{Reader, Writer},
    event_crc,
};

fn read_u64(r: &mut Reader<'_>) -> Result<u64, ProtocolError> {
    let b = r.bytes(8)?;
    Ok(u64::from_le_bytes([
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
    ]))
}
fn read_i64(r: &mut Reader<'_>) -> Result<i64, ProtocolError> {
    let b = r.bytes(8)?;
    Ok(i64::from_le_bytes([
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
    ]))
}
fn nonzero(value: u32) -> Result<(), ProtocolError> {
    if value == 0 {
        Err(ProtocolError::InvalidId)
    } else {
        Ok(())
    }
}
fn stream_id(value: u16) -> Result<(), ProtocolError> {
    if value == 0 {
        Err(ProtocolError::InvalidId)
    } else {
        Ok(())
    }
}
fn domain(value: u8) -> Result<(), ProtocolError> {
    if value == u8::MAX {
        Err(ProtocolError::Reserved)
    } else {
        Ok(())
    }
}
fn status_reason(status: u16, reason: u16) -> Result<(), ProtocolError> {
    let required = match reason {
        0xff01 | 0xff02 => Some(0x12),
        0xff03 | 0xff04 => Some(0x05),
        0xff05 | 0xff06 => Some(0x04),
        0xff07 => Some(0x0b),
        0xff08 => Some(0x01),
        _ => None,
    };
    if status > 0x12
        || status == 0x10
        || (status == 0 && reason != 0)
        || required.is_some_and(|required| status != required)
    {
        Err(ProtocolError::InvalidState)
    } else {
        Ok(())
    }
}
fn command_opcode(opcode: u8) -> Result<(), ProtocolError> {
    if (1..=8).contains(&opcode) {
        Ok(())
    } else {
        Err(ProtocolError::InvalidKind)
    }
}

/// Shared `OPEN/OPEN_V2/COUNT` filter fields; disabled fields must be zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryFilter {
    /// Domain key; FF is reserved.
    pub domain: u8,
    /// Bit0 time, bit1 record type, bit2 parent session. Original OPEN permits bit0 only.
    pub filter_flags: u8,
    /// Left-closed UTC millisecond boundary, zero when bit0 is disabled.
    pub start_utc_ms: i64,
    /// Right-open UTC millisecond boundary, zero when bit0 is disabled.
    pub end_utc_ms: i64,
    /// Domain-owned record type, zero when bit1 is disabled.
    pub record_type: u16,
    /// Parent identity, zero when bit2 is disabled.
    pub parent_session: u64,
}
impl HistoryFilter {
    /// Validate the exact filter rules without creating an encoded request.
    /// `extended` permits the record-type and parent-session flags of `OPEN_V2/COUNT`.
    /// # Errors
    /// Reserved flags/domain or inconsistent disabled fields and time ranges fail.
    pub fn validate(self, extended: bool) -> Result<(), ProtocolError> {
        domain(self.domain)?;
        let allowed = if extended { 7 } else { 1 };
        if self.filter_flags & !allowed != 0 {
            return Err(ProtocolError::Reserved);
        }
        if self.filter_flags & 1 == 0 {
            if self.start_utc_ms != 0 || self.end_utc_ms != 0 {
                return Err(ProtocolError::InvalidState);
            }
        } else if self.start_utc_ms >= self.end_utc_ms {
            return Err(ProtocolError::InvalidState);
        }
        if (self.filter_flags & 2 == 0 && self.record_type != 0)
            || (self.filter_flags & 4 == 0 && self.parent_session != 0)
        {
            return Err(ProtocolError::InvalidState);
        }
        Ok(())
    }
    fn read(r: &mut Reader<'_>, extended: bool) -> Result<Self, ProtocolError> {
        let domain = r.u8()?;
        let filter_flags = r.u8()?;
        let start_utc_ms = read_i64(r)?;
        let end_utc_ms = read_i64(r)?;
        let (record_type, parent_session) = if extended {
            (r.u16()?, read_u64(r)?)
        } else {
            (0, 0)
        };
        Ok(Self {
            domain,
            filter_flags,
            start_utc_ms,
            end_utc_ms,
            record_type,
            parent_session,
        })
    }
    fn write(self, w: &mut Writer<'_>, extended: bool) -> Result<(), ProtocolError> {
        w.u8(self.domain)?;
        w.u8(self.filter_flags)?;
        w.bytes(&self.start_utc_ms.to_le_bytes())?;
        w.bytes(&self.end_utc_ms.to_le_bytes())?;
        if extended {
            w.u16(self.record_type)?;
            w.bytes(&self.parent_session.to_le_bytes())?;
        }
        Ok(())
    }
}

/// Typed request for historical module 10, opcodes 01..08.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HistoryRequest {
    /// Enumerate inclusive domain keys; FF is forbidden as an input sentinel.
    Enumerate {
        /// Inclusive domain start, zero for the first page.
        start: u8,
        /// Maximum domains to return, 1..8.
        max: u8,
    },
    /// Original 18-byte time-filtered or unfiltered OPEN.
    Open(HistoryFilter),
    /// Read a bounded batch after an exclusive record ID.
    Read {
        /// Nonzero live snapshot identity.
        snapshot: u32,
        /// Exclusive cursor; zero starts at the first record.
        after_id: u64,
        /// Maximum batch size, 1..16.
        max_records: u16,
    },
    /// Ask the record store to durably acknowledge a proven unfiltered prefix.
    Ack {
        /// Nonzero snapshot identity.
        snapshot: u32,
        /// Nonzero expected dataset epoch.
        epoch: u32,
        /// Claimed durable prefix endpoint, checked by the record service.
        upto_id: u64,
    },
    /// Drain a stream and close its snapshot without implying ACK.
    Close {
        /// Nonzero snapshot identity.
        snapshot: u32,
    },
    /// Extended 28-byte time/type/parent snapshot OPEN.
    OpenV2(HistoryFilter),
    /// Count using the same 28-byte filter contract, without creating an ACK proof.
    Count(HistoryFilter),
    /// Drain the current batch while retaining its snapshot and durable ACK.
    CancelRead {
        /// Nonzero snapshot identity.
        snapshot: u32,
        /// Nonzero current stream identity.
        stream_id: u16,
    },
}
impl core::fmt::Debug for HistoryRequest {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HistoryRequest")
            .field("opcode", &self.opcode())
            .finish_non_exhaustive()
    }
}
impl HistoryRequest {
    /// Module-local command opcode.
    #[must_use]
    pub const fn opcode(&self) -> u8 {
        match self {
            Self::Enumerate { .. } => 1,
            Self::Open(_) => 2,
            Self::Read { .. } => 3,
            Self::Ack { .. } => 4,
            Self::Close { .. } => 5,
            Self::OpenV2(_) => 6,
            Self::Count(_) => 7,
            Self::CancelRead { .. } => 8,
        }
    }
    /// Decode an exact request body with no common status or transport header.
    /// # Errors
    /// Rejects missing/extra bytes, reserved flags/sentinels, invalid IDs or bounds.
    pub fn decode(opcode: u8, bytes: &[u8]) -> Result<Self, ProtocolError> {
        let mut r = Reader::new(bytes);
        let value = match opcode {
            1 => Self::Enumerate {
                start: r.u8()?,
                max: r.u8()?,
            },
            2 => Self::Open(HistoryFilter::read(&mut r, false)?),
            3 => Self::Read {
                snapshot: r.u32()?,
                after_id: read_u64(&mut r)?,
                max_records: r.u16()?,
            },
            4 => Self::Ack {
                snapshot: r.u32()?,
                epoch: r.u32()?,
                upto_id: read_u64(&mut r)?,
            },
            5 => Self::Close { snapshot: r.u32()? },
            6 => Self::OpenV2(HistoryFilter::read(&mut r, true)?),
            7 => Self::Count(HistoryFilter::read(&mut r, true)?),
            8 => Self::CancelRead {
                snapshot: r.u32()?,
                stream_id: r.u16()?,
            },
            _ => return Err(ProtocolError::InvalidKind),
        };
        r.finish()?;
        value.validate()?;
        Ok(value)
    }
    /// Encode an exact validated request without touching trailing capacity.
    /// # Errors
    /// Rejects invalid fields or insufficient caller capacity before writing.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, ProtocolError> {
        self.validate()?;
        let len = match self {
            Self::Enumerate { .. } => 2,
            Self::Open(_) => 18,
            Self::Read { .. } => 14,
            Self::Ack { .. } => 16,
            Self::Close { .. } => 4,
            Self::OpenV2(_) | Self::Count(_) => 28,
            Self::CancelRead { .. } => 6,
        };
        if out.len() < len {
            return Err(ProtocolError::Capacity);
        }
        let mut w = Writer::new(&mut out[..len]);
        match *self {
            Self::Enumerate { start, max } => {
                w.u8(start)?;
                w.u8(max)?;
            }
            Self::Open(filter) => filter.write(&mut w, false)?,
            Self::OpenV2(filter) | Self::Count(filter) => filter.write(&mut w, true)?,
            Self::Read {
                snapshot,
                after_id,
                max_records,
            } => {
                w.u32(snapshot)?;
                w.bytes(&after_id.to_le_bytes())?;
                w.u16(max_records)?;
            }
            Self::Ack {
                snapshot,
                epoch,
                upto_id,
            } => {
                w.u32(snapshot)?;
                w.u32(epoch)?;
                w.bytes(&upto_id.to_le_bytes())?;
            }
            Self::Close { snapshot } => w.u32(snapshot)?,
            Self::CancelRead {
                snapshot,
                stream_id,
            } => {
                w.u32(snapshot)?;
                w.u16(stream_id)?;
            }
        }
        Ok(w.len())
    }
    fn validate(self) -> Result<(), ProtocolError> {
        match self {
            Self::Enumerate { start, max } => {
                domain(start)?;
                if !(1..=8).contains(&max) {
                    return Err(ProtocolError::InvalidLength);
                }
            }
            Self::Open(filter) => filter.validate(false)?,
            Self::OpenV2(filter) | Self::Count(filter) => filter.validate(true)?,
            Self::Read {
                snapshot,
                max_records,
                ..
            } => {
                nonzero(snapshot)?;
                if !(1..=16).contains(&max_records) {
                    return Err(ProtocolError::InvalidLength);
                }
            }
            Self::Ack {
                snapshot, epoch, ..
            } => {
                nonzero(snapshot)?;
                nonzero(epoch)?;
            }
            Self::Close { snapshot } => nonzero(snapshot)?,
            Self::CancelRead {
                snapshot,
                stream_id: id,
            } => {
                nonzero(snapshot)?;
                stream_id(id)?;
            }
        }
        Ok(())
    }
}

/// One exact 21-byte history domain directory entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryDomain {
    /// Domain key, with FF reserved.
    pub domain: u8,
    /// Stable nonzero epoch; normal reboot must not change it.
    pub epoch: u32,
    /// Oldest retained record ID, or zero for an empty domain.
    pub oldest_id: u64,
    /// Newest retained record ID, zero together with oldest for an empty domain.
    pub newest_id: u64,
}
impl HistoryDomain {
    /// Encoded directory-entry size.
    pub const LENGTH: usize = 21;
    /// Decode a domain entry, independent of directory authorization.
    /// # Errors
    /// Rejects incomplete/extra fields, reserved domain, zero epoch or reversed ranges.
    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        let mut r = Reader::new(bytes);
        let value = Self {
            domain: r.u8()?,
            epoch: r.u32()?,
            oldest_id: read_u64(&mut r)?,
            newest_id: read_u64(&mut r)?,
        };
        r.finish()?;
        value.validate()?;
        Ok(value)
    }
    /// Encode a validated domain entry.
    /// # Errors
    /// Rejects invalid fields or insufficient capacity before writing.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, ProtocolError> {
        self.validate()?;
        if out.len() < Self::LENGTH {
            return Err(ProtocolError::Capacity);
        }
        let mut w = Writer::new(&mut out[..Self::LENGTH]);
        w.u8(self.domain)?;
        w.u32(self.epoch)?;
        w.bytes(&self.oldest_id.to_le_bytes())?;
        w.bytes(&self.newest_id.to_le_bytes())?;
        Ok(w.len())
    }
    fn validate(self) -> Result<(), ProtocolError> {
        domain(self.domain)?;
        nonzero(self.epoch)?;
        if self.oldest_id > self.newest_id || (self.oldest_id == 0) != (self.newest_id == 0) {
            return Err(ProtocolError::InvalidState);
        }
        Ok(())
    }
}

/// Borrowed encoded directory entries, validated on response encode/decode.
/// Construct outgoing entry bytes with [`HistoryDomain::encode`].
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct HistoryPage<'a> {
    /// Stable dataset identity, not derived from a radio address.
    pub dataset_id: [u8; 16],
    /// First unreturned domain key, or FF at the end.
    pub next: u8,
    /// Exact concatenated 21-byte entries; at most eight, sorted and unique.
    pub entries: &'a [u8],
}
impl core::fmt::Debug for HistoryPage<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HistoryPage")
            .field("entries_length", &self.entries.len())
            .finish_non_exhaustive()
    }
}
impl HistoryPage<'_> {
    /// Decode the indexed domain without allocating; returns none for invalid/out-of-range data.
    #[must_use]
    pub fn domain(&self, index: usize) -> Option<HistoryDomain> {
        let start = index.checked_mul(HistoryDomain::LENGTH)?;
        let end = start.checked_add(HistoryDomain::LENGTH)?;
        HistoryDomain::decode(self.entries.get(start..end)?).ok()
    }
    fn validate(self) -> Result<(), ProtocolError> {
        if !self.entries.len().is_multiple_of(HistoryDomain::LENGTH)
            || self.entries.len() / HistoryDomain::LENGTH > 8
        {
            return Err(ProtocolError::InvalidLength);
        }
        let mut previous = None;
        for bytes in self.entries.chunks_exact(HistoryDomain::LENGTH) {
            let entry = HistoryDomain::decode(bytes)?;
            if previous.is_some_and(|domain| entry.domain <= domain) {
                return Err(ProtocolError::Sequence);
            }
            previous = Some(entry.domain);
        }
        if self.next != u8::MAX && previous.is_none_or(|last| self.next <= last) {
            return Err(ProtocolError::Sequence);
        }
        Ok(())
    }
}

/// Exact 30-byte `OPEN/OPEN_V2` success data, excluding common status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistorySnapshot {
    /// Nonzero connection-local snapshot identity.
    pub snapshot: u32,
    /// Nonzero persistent domain epoch.
    pub epoch: u32,
    /// First eligible record ID; zero for an empty snapshot.
    pub first_id: u64,
    /// Fixed upper record ID; zero for an empty snapshot.
    pub last_id: u64,
    /// Record count; zero exactly when first and last IDs are zero.
    pub count: u32,
    /// Lease in seconds; service, not this codec, enforces expiry.
    pub lease_s: u16,
}
impl HistorySnapshot {
    fn read(r: &mut Reader<'_>) -> Result<Self, ProtocolError> {
        Ok(Self {
            snapshot: r.u32()?,
            epoch: r.u32()?,
            first_id: read_u64(r)?,
            last_id: read_u64(r)?,
            count: r.u32()?,
            lease_s: r.u16()?,
        })
    }
    /// Validate the exact OPEN result rules without response encoding scratch.
    /// # Errors
    /// Invalid identities, lease or inconsistent empty/range/count metadata fail.
    pub fn validate(self) -> Result<(), ProtocolError> {
        nonzero(self.snapshot)?;
        nonzero(self.epoch)?;
        if self.first_id > self.last_id
            || (self.first_id == 0) != (self.last_id == 0)
            || (self.count == 0) != (self.first_id == 0)
        {
            return Err(ProtocolError::InvalidState);
        }
        if self.count > 0 && u64::from(self.count) > self.last_id - self.first_id + 1 {
            return Err(ProtocolError::InvalidLength);
        }
        Ok(())
    }
    fn write(self, w: &mut Writer<'_>) -> Result<(), ProtocolError> {
        w.u32(self.snapshot)?;
        w.u32(self.epoch)?;
        w.bytes(&self.first_id.to_le_bytes())?;
        w.bytes(&self.last_id.to_le_bytes())?;
        w.u32(self.count)?;
        w.u16(self.lease_s)
    }
}

/// Exact 13-byte READ admission success data, excluding common status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryRead {
    /// Newly assigned nonzero stream identity; service enforces connection-wide nonreuse.
    pub stream_id: u16,
    /// Exclusive continuation cursor after this batch.
    pub end_after_id: u64,
    /// True when this snapshot has no further eligible records.
    pub eof: bool,
    /// Accepted RECORD count, at most sixteen; must be checked against DONE and DATA.
    pub admitted: u16,
}
impl HistoryRead {
    fn validate(self) -> Result<(), ProtocolError> {
        stream_id(self.stream_id)?;
        if self.admitted > 16 {
            return Err(ProtocolError::InvalidLength);
        }
        Ok(())
    }
}

/// Complete history logical response, including exactly one four-byte status prefix.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HistoryResponse<'a> {
    /// 01: domain directory page.
    Enumerate(HistoryPage<'a>),
    /// 02: immutable snapshot metadata.
    Open(HistorySnapshot),
    /// 03: accepted batch metadata; not proof of sent or saved data.
    Read(HistoryRead),
    /// 04: actual durable watermark after the record-store transaction.
    Ack {
        /// Persisted domain acknowledgement point.
        committed_id: u64,
    },
    /// 05: final admitted DATA barrier after closing.
    Close {
        /// Zero if no DATA started; otherwise the last whole admitted message ID.
        last_data_message_id: u16,
    },
    /// 06: extended-filter snapshot metadata.
    OpenV2(HistorySnapshot),
    /// 07: count result, which never creates ACK coverage.
    Count {
        /// Nonzero domain epoch.
        epoch: u32,
        /// Matching record count.
        count: u32,
    },
    /// 08: final admitted DATA barrier after cancelling a batch.
    CancelRead {
        /// Zero if no DATA started; otherwise the last whole admitted message ID.
        last_data_message_id: u16,
    },
    /// Any failure has only its exact common status/reason and no success tail.
    Failure {
        /// Original command opcode, 01..08.
        opcode: u8,
        /// Nonzero public status, excluding reserved PENDING=0010.
        status: u16,
        /// Common or module-local reason; authorization is enforced elsewhere.
        reason: u16,
    },
}
impl core::fmt::Debug for HistoryResponse<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HistoryResponse")
            .field("opcode", &self.opcode())
            .finish_non_exhaustive()
    }
}
impl<'a> HistoryResponse<'a> {
    /// Original request's opcode.
    #[must_use]
    pub const fn opcode(&self) -> u8 {
        match self {
            Self::Enumerate(_) => 1,
            Self::Open(_) => 2,
            Self::Read(_) => 3,
            Self::Ack { .. } => 4,
            Self::Close { .. } => 5,
            Self::OpenV2(_) => 6,
            Self::Count { .. } => 7,
            Self::CancelRead { .. } => 8,
            Self::Failure { opcode, .. } => *opcode,
        }
    }
    /// Decode exact logical response bytes including common status.
    /// # Errors
    /// Rejects malformed success data, extra failure data, unknown/reserved status,
    /// invalid opcode, inconsistent snapshot/range/page fields or nonzero OK reason.
    pub fn decode(opcode: u8, bytes: &'a [u8]) -> Result<Self, ProtocolError> {
        command_opcode(opcode)?;
        let mut r = Reader::new(bytes);
        let status = r.u16()?;
        let reason = r.u16()?;
        status_reason(status, reason)?;
        if status != 0 {
            r.finish()?;
            return Ok(Self::Failure {
                opcode,
                status,
                reason,
            });
        }
        let value = match opcode {
            1 => {
                let mut dataset_id = [0; 16];
                dataset_id.copy_from_slice(r.bytes(16)?);
                let next = r.u8()?;
                let count = usize::from(r.u8()?);
                if count > 8 {
                    return Err(ProtocolError::InvalidLength);
                }
                let entries = r.bytes(count * HistoryDomain::LENGTH)?;
                Self::Enumerate(HistoryPage {
                    dataset_id,
                    next,
                    entries,
                })
            }
            2 => Self::Open(HistorySnapshot::read(&mut r)?),
            3 => {
                let stream_id = r.u16()?;
                let end_after_id = read_u64(&mut r)?;
                let eof = r.u8()?;
                if eof > 1 {
                    return Err(ProtocolError::InvalidState);
                }
                Self::Read(HistoryRead {
                    stream_id,
                    end_after_id,
                    eof: eof == 1,
                    admitted: r.u16()?,
                })
            }
            4 => Self::Ack {
                committed_id: read_u64(&mut r)?,
            },
            5 => Self::Close {
                last_data_message_id: r.u16()?,
            },
            6 => Self::OpenV2(HistorySnapshot::read(&mut r)?),
            7 => Self::Count {
                epoch: r.u32()?,
                count: r.u32()?,
            },
            8 => Self::CancelRead {
                last_data_message_id: r.u16()?,
            },
            _ => return Err(ProtocolError::InvalidKind),
        };
        r.finish()?;
        value.validate()?;
        Ok(value)
    }
    /// Encode a validated logical response including its common status prefix.
    /// # Errors
    /// Rejects invalid fields or insufficient output capacity before writing.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, ProtocolError> {
        self.validate()?;
        let len = match self {
            Self::Enumerate(page) => 22 + page.entries.len(),
            Self::Open(_) | Self::OpenV2(_) => 34,
            Self::Read(_) => 17,
            Self::Ack { .. } | Self::Count { .. } => 12,
            Self::Close { .. } | Self::CancelRead { .. } => 6,
            Self::Failure { .. } => 4,
        };
        if out.len() < len {
            return Err(ProtocolError::Capacity);
        }
        let mut w = Writer::new(&mut out[..len]);
        let (status, reason) = if let Self::Failure { status, reason, .. } = *self {
            (status, reason)
        } else {
            (0, 0)
        };
        w.u16(status)?;
        w.u16(reason)?;
        match *self {
            Self::Enumerate(page) => {
                w.bytes(&page.dataset_id)?;
                w.u8(page.next)?;
                w.u8(u8::try_from(page.entries.len() / HistoryDomain::LENGTH)
                    .map_err(|_| ProtocolError::InvalidLength)?)?;
                w.bytes(page.entries)?;
            }
            Self::Open(snapshot) | Self::OpenV2(snapshot) => snapshot.write(&mut w)?,
            Self::Read(read) => {
                w.u16(read.stream_id)?;
                w.bytes(&read.end_after_id.to_le_bytes())?;
                w.u8(u8::from(read.eof))?;
                w.u16(read.admitted)?;
            }
            Self::Ack { committed_id } => w.bytes(&committed_id.to_le_bytes())?,
            Self::Close {
                last_data_message_id,
            }
            | Self::CancelRead {
                last_data_message_id,
            } => w.u16(last_data_message_id)?,
            Self::Count { epoch, count } => {
                w.u32(epoch)?;
                w.u32(count)?;
            }
            Self::Failure { .. } => {}
        }
        Ok(w.len())
    }
    fn validate(self) -> Result<(), ProtocolError> {
        match self {
            Self::Enumerate(page) => page.validate()?,
            Self::Open(snapshot) | Self::OpenV2(snapshot) => snapshot.validate()?,
            Self::Read(read) => read.validate()?,
            Self::Count { epoch, .. } => nonzero(epoch)?,
            Self::Failure {
                opcode,
                status,
                reason,
            } => {
                command_opcode(opcode)?;
                if status == 0 {
                    return Err(ProtocolError::InvalidState);
                }
                status_reason(status, reason)?;
            }
            Self::Ack { .. } | Self::Close { .. } | Self::CancelRead { .. } => {}
        }
        Ok(())
    }
}

/// Complete RECORD event, with an integrity-checked borrowed opaque domain payload.
/// It never includes a common command status prefix.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct HistoryRecord<'a> {
    /// Nonzero live snapshot identity.
    pub snapshot: u32,
    /// Nonzero live stream identity.
    pub stream_id: u16,
    /// Nonzero stable domain epoch.
    pub epoch: u32,
    /// Nonzero persistent record identity, independent of UTC order.
    pub record_id: u64,
    /// Domain-owned record type; payload schema validation is a separate concern.
    pub record_type: u16,
    /// Version of the domain-owned record encoding.
    pub schema: u16,
    /// Signed UTC milliseconds; zero when time-valid bit0 is clear.
    pub utc_ms: i64,
    /// Bit0 means time valid; all other bits are reserved.
    pub flags: u16,
    /// Opaque versioned payload, borrowed without allocation or copying on decode.
    pub payload: &'a [u8],
}
impl core::fmt::Debug for HistoryRecord<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HistoryRecord")
            .field("payload_length", &self.payload.len())
            .finish_non_exhaustive()
    }
}
impl<'a> HistoryRecord<'a> {
    /// Fixed header plus CRC, excluding the domain payload.
    pub const OVERHEAD: usize = 38;
    /// Decode a full RECORD event and verify its WAP1/10/80 CRC before exposing fields.
    /// # Errors
    /// Rejects invalid negotiated data capacity, lengths, reserved fields, zero
    /// identities or CRC. The CRC error is `Integrity`; no record is returned.
    pub fn decode(bytes: &'a [u8], data_max: usize) -> Result<Self, ProtocolError> {
        if !(128..=512).contains(&data_max)
            || bytes.len() < Self::OVERHEAD
            || bytes.len() > data_max
        {
            return Err(ProtocolError::InvalidLength);
        }
        let body_len = bytes.len() - 4;
        let crc_bytes = &bytes[body_len..];
        let expected = u32::from_le_bytes([crc_bytes[0], crc_bytes[1], crc_bytes[2], crc_bytes[3]]);
        if event_crc(0x10, 0x80, &bytes[..body_len])? != expected {
            return Err(ProtocolError::Integrity);
        }
        let mut r = Reader::new(&bytes[..body_len]);
        let snapshot = r.u32()?;
        let stream_id = r.u16()?;
        let epoch = r.u32()?;
        let record_id = read_u64(&mut r)?;
        let record_type = r.u16()?;
        let schema = r.u16()?;
        let utc_ms = read_i64(&mut r)?;
        let flags = r.u16()?;
        let n = usize::from(r.u16()?);
        let payload = r.bytes(n)?;
        r.finish()?;
        let value = Self {
            snapshot,
            stream_id,
            epoch,
            record_id,
            record_type,
            schema,
            utc_ms,
            flags,
            payload,
        };
        value.validate(data_max)?;
        Ok(value)
    }
    /// Encode header/payload and calculate a fresh event-domain CRC in caller storage.
    /// The negotiated `data_max` includes the entire event and its trailing CRC.
    /// # Errors
    /// Rejects invalid fields/capacity before writing. Storage integrity must have
    /// been checked separately; this method cannot replace the WREC storage CRC.
    pub fn encode(&self, out: &mut [u8], data_max: usize) -> Result<usize, ProtocolError> {
        self.validate(data_max)?;
        let len = Self::OVERHEAD + self.payload.len();
        if out.len() < len {
            return Err(ProtocolError::Capacity);
        }
        let (header, crc) = self.envelope(data_max)?;
        out[..34].copy_from_slice(&header);
        out[34..len - 4].copy_from_slice(self.payload);
        out[len - 4..len].copy_from_slice(&crc);
        Ok(len)
    }
    /// Encode the fixed 34-byte header and trailing CRC separately.
    /// This allows a caller to read payload bytes directly into the DATA frame
    /// and then install its envelope, without a second payload-sized buffer.
    /// # Errors
    /// Rejects the same invalid fields/negotiated capacity as `encode`.
    pub fn envelope(&self, data_max: usize) -> Result<([u8; 34], [u8; 4]), ProtocolError> {
        self.validate(data_max)?;
        let mut header = [0; 34];
        let mut w = Writer::new(&mut header);
        w.u32(self.snapshot)?;
        w.u16(self.stream_id)?;
        w.u32(self.epoch)?;
        w.bytes(&self.record_id.to_le_bytes())?;
        w.u16(self.record_type)?;
        w.u16(self.schema)?;
        w.bytes(&self.utc_ms.to_le_bytes())?;
        w.u16(self.flags)?;
        w.u16(u16::try_from(self.payload.len()).map_err(|_| ProtocolError::InvalidLength)?)?;
        let mut crc = crate::Crc32c::new();
        crc.update(b"WAP1");
        crc.update(&[0x10, 0x80]);
        crc.update(
            &u16::try_from(34 + self.payload.len())
                .map_err(|_| ProtocolError::InvalidLength)?
                .to_le_bytes(),
        );
        crc.update(&header);
        crc.update(self.payload);
        Ok((header, crc.finish().to_le_bytes()))
    }
    fn validate(self, data_max: usize) -> Result<(), ProtocolError> {
        if !(128..=512).contains(&data_max) || self.payload.len() > data_max - Self::OVERHEAD {
            return Err(ProtocolError::InvalidLength);
        }
        nonzero(self.snapshot)?;
        stream_id(self.stream_id)?;
        nonzero(self.epoch)?;
        if self.record_id == 0 {
            return Err(ProtocolError::InvalidId);
        }
        if self.flags & !1 != 0 {
            return Err(ProtocolError::Reserved);
        }
        if self.flags & 1 == 0 && self.utc_ms != 0 {
            return Err(ProtocolError::InvalidState);
        }
        Ok(())
    }
}

/// Exact sixteen-byte DONE control event, with its own completion status/reason.
/// No extra common status, application CRC, persistence proof or barrier is implied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HistoryDone {
    /// Nonzero batch stream identity.
    pub stream_id: u16,
    /// Complete RECORD messages admitted, at most the READ limit of sixteen.
    pub sent_count: u16,
    /// Last record in the batch; service correlates it with READ and DATA.
    pub last_id: u64,
    /// Public completion status, excluding reserved PENDING.
    pub status: u16,
    /// Completion reason; zero with success.
    pub reason: u16,
}
impl HistoryDone {
    /// Fixed control event length without any header or additional prefix.
    pub const LENGTH: usize = 16;
    /// Decode an exact DONE body.
    /// # Errors
    /// Rejects missing/extra bytes, invalid stream/count/status or nonzero OK reason.
    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        let mut r = Reader::new(bytes);
        let value = Self {
            stream_id: r.u16()?,
            sent_count: r.u16()?,
            last_id: read_u64(&mut r)?,
            status: r.u16()?,
            reason: r.u16()?,
        };
        r.finish()?;
        value.validate()?;
        Ok(value)
    }
    /// Encode a validated fixed-length control event.
    /// # Errors
    /// Rejects invalid fields or insufficient caller capacity before writing.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, ProtocolError> {
        self.validate()?;
        if out.len() < Self::LENGTH {
            return Err(ProtocolError::Capacity);
        }
        let mut w = Writer::new(&mut out[..Self::LENGTH]);
        w.u16(self.stream_id)?;
        w.u16(self.sent_count)?;
        w.bytes(&self.last_id.to_le_bytes())?;
        w.u16(self.status)?;
        w.u16(self.reason)?;
        Ok(w.len())
    }
    fn validate(self) -> Result<(), ProtocolError> {
        stream_id(self.stream_id)?;
        if self.sent_count > 16 {
            return Err(ProtocolError::InvalidLength);
        }
        status_reason(self.status, self.reason)
    }
}
