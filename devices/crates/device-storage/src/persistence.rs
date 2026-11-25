//! Bounded, two-bank durable snapshots over an injected medium.
//!
//! See the crate's persistence README for the on-media format and fault model.
//! CRC detects accidental corruption; it does not authenticate data.
#![forbid(unsafe_code)]
use core::fmt;
use device_protocol::Crc32c;

pub(crate) const HEADER_BYTES: usize = 32;
pub(crate) const MAX_WRITE: usize = 256;
pub(crate) const MAGIC: &[u8; 8] = b"DSNAP001";

/// Persistence operation failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Error {
    /// Geometry, alignment or selected bank range is invalid.
    InvalidLayout,
    /// A payload or output buffer exceeds available capacity.
    Capacity,
    /// Expected revision differs from the recovered current revision.
    Conflict,
    /// The current revision cannot increment without wrapping.
    RevisionExhausted,
    /// The medium could not complete an operation.
    Io,
    /// Committed metadata or data is corrupt or ambiguous.
    Corrupt,
    /// A mutation or recovery failed; recover before reading or writing again.
    NeedsRecovery,
}
/// Logical medium geometry, in bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Geometry {
    /// Total readable capacity.
    pub capacity: u32,
    /// Erase offset and length alignment.
    pub erase_size: u32,
    /// Program offset and length alignment; supported range is 1 through 256.
    pub write_size: u32,
}
/// Two contiguous, equally sized disjoint banks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Layout {
    /// First bank start, aligned for erase and program operations.
    pub start: u32,
    /// Each bank's size, aligned for erase and program operations.
    pub bank_bytes: u32,
}
/// Metadata for one complete snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Snapshot {
    /// Monotonically increasing revision, starting at one.
    pub revision: u64,
    /// Number of payload bytes.
    pub length: usize,
}
/// Ordered logical storage with erased bytes equal to `0xff`.
///
/// The adapter must preserve operations preceding a successful sync across
/// restart. Failed mutations may have partially applied; see the README's
/// explicit byte-level fault assumptions. Reads need no alignment.
pub trait Medium {
    /// Return fixed geometry for this medium instance.
    fn geometry(&self) -> Geometry;
    /// Read the entire requested range.
    ///
    /// # Errors
    /// Return `Io` if the requested read cannot complete.
    fn read(&mut self, offset: u32, out: &mut [u8]) -> Result<(), Error>;
    /// Erase an aligned range to `0xff`.
    ///
    /// # Errors
    /// Return `Io` on failure, including a partially completed erase.
    fn erase(&mut self, offset: u32, length: u32) -> Result<(), Error>;
    /// Program aligned units, changing only one bits to zero bits.
    ///
    /// # Errors
    /// Return `Io` on failure, including partially programmed bytes.
    fn program(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Error>;
    /// Make preceding ordered operations durable.
    ///
    /// # Errors
    /// Return `Io` if durability cannot be established; the outcome is uncertain.
    fn sync(&mut self) -> Result<(), Error>;
}
/// Whole-snapshot compare-and-swap storage.
pub trait SnapshotStore {
    /// Copy the recovered payload into `out`; blank storage returns `None`.
    ///
    /// # Errors
    /// Returns `Capacity` for short output and `NeedsRecovery` after uncertainty.
    fn load(&self, out: &mut [u8]) -> Result<Option<Snapshot>, Error>;
    /// Durably replace the payload only if `expected` equals its revision.
    ///
    /// # Errors
    /// Returns `Conflict`, `Capacity`, or `RevisionExhausted` before mutation;
    /// medium failures return `Io` and poison subsequent operations.
    fn commit(&mut self, expected: u64, bytes: &[u8]) -> Result<Snapshot, Error>;
    /// Revalidate both banks and rebuild the owned cache.
    ///
    /// # Errors
    /// Returns `Io` or `Corrupt`; subsequent access requires successful recovery.
    fn recover(&mut self) -> Result<(), Error>;
}
/// Heap-free snapshot engine; `N` bounds the owned payload cache.
///
/// Debug output intentionally omits payload and medium contents.
pub struct AtomicSnapshot<M, const N: usize> {
    medium: M,
    layout: Layout,
    geometry: Geometry,
    payload_offset: u32,
    cache: [u8; N],
    current: Option<Snapshot>,
    bank: u32,
    poisoned: bool,
}
impl<M, const N: usize> fmt::Debug for AtomicSnapshot<M, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AtomicSnapshot")
            .field("layout", &self.layout)
            .field("current", &self.current)
            .field("poisoned", &self.poisoned)
            .finish_non_exhaustive()
    }
}
impl<M: Medium, const N: usize> AtomicSnapshot<M, N> {
    /// Validate layout and recover committed storage without formatting it.
    ///
    /// # Errors
    /// Returns `InvalidLayout` for unsupported geometry/ranges, or a recovery
    /// error (`Io` or `Corrupt`). Recovery syncs the medium but never erases or
    /// programs it.
    pub fn open(medium: M, layout: Layout) -> Result<Self, Error> {
        let geometry = medium.geometry();
        let payload_offset = validate::<N>(geometry, layout)?;
        let mut store = Self {
            medium,
            layout,
            geometry,
            payload_offset,
            cache: [0; N],
            current: None,
            bank: 0,
            poisoned: true,
        };
        store.recover()?;
        Ok(store)
    }
    /// Release the medium, including after an uncertain commit outcome.
    pub fn into_medium(self) -> M {
        self.medium
    }

    fn bank_start(&self, bank: u32) -> u32 {
        // Construction checked the entire two-bank range.
        self.layout.start + bank * self.layout.bank_bytes
    }
    fn marker_offset(&self, bank: u32) -> u32 {
        self.bank_start(bank) + self.layout.bank_bytes - self.geometry.write_size
    }
    fn read_bank(&mut self, bank: u32, payload: &mut [u8; N]) -> Result<Option<Snapshot>, Error> {
        let mut marker = [0; MAX_WRITE];
        let unit = usize::try_from(self.geometry.write_size).map_err(|_| Error::InvalidLayout)?;
        self.medium
            .read(self.marker_offset(bank), &mut marker[..unit])
            .map_err(|_| Error::Io)?;
        if !marker[..unit].iter().all(|&byte| byte == 0xa5) {
            return Ok(None);
        }
        let mut header = [0; HEADER_BYTES];
        self.medium
            .read(self.bank_start(bank), &mut header)
            .map_err(|_| Error::Io)?;
        let (snapshot, payload_crc) =
            decode_header::<N>(&header, self.geometry, self.layout, self.payload_offset)?;
        let Snapshot { revision, length } = snapshot;
        self.medium
            .read(
                self.bank_start(bank) + self.payload_offset,
                &mut payload[..length],
            )
            .map_err(|_| Error::Io)?;
        if crc(&payload[..length]) != payload_crc {
            return Err(Error::Corrupt);
        }
        Ok(Some(Snapshot { revision, length }))
    }
    fn program_padded(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Error> {
        let unit = usize::try_from(self.geometry.write_size).map_err(|_| Error::InvalidLayout)?;
        let mut scratch = [0xff; MAX_WRITE];
        for (index, chunk) in bytes.chunks(unit).enumerate() {
            scratch[..unit].fill(0xff);
            scratch[..chunk.len()].copy_from_slice(chunk);
            let relative = u32::try_from(index * unit).map_err(|_| Error::Capacity)?;
            self.medium
                .program(offset + relative, &scratch[..unit])
                .map_err(|_| Error::Io)?;
        }
        Ok(())
    }
    fn write_snapshot(&mut self, bank: u32, snapshot: Snapshot, bytes: &[u8]) -> Result<(), Error> {
        let unit = usize::try_from(self.geometry.write_size).map_err(|_| Error::InvalidLayout)?;
        // Retire the stale commit before any erase can damage its header/data.
        self.medium
            .program(self.marker_offset(bank), &[0; MAX_WRITE][..unit])
            .map_err(|_| Error::Io)?;
        self.medium.sync().map_err(|_| Error::Io)?;
        self.medium
            .erase(self.bank_start(bank), self.layout.bank_bytes)
            .map_err(|_| Error::Io)?;
        let header = encode_header(snapshot, crc(bytes))?;
        self.program_padded(self.bank_start(bank), &header)?;
        self.program_padded(self.bank_start(bank) + self.payload_offset, bytes)?;
        self.medium.sync().map_err(|_| Error::Io)?;
        self.medium
            .program(self.marker_offset(bank), &[0xa5; MAX_WRITE][..unit])
            .map_err(|_| Error::Io)?;
        self.medium.sync().map_err(|_| Error::Io)
    }
}
impl<M: Medium, const N: usize> SnapshotStore for AtomicSnapshot<M, N> {
    fn load(&self, out: &mut [u8]) -> Result<Option<Snapshot>, Error> {
        if self.poisoned {
            return Err(Error::NeedsRecovery);
        }
        if let Some(snapshot) = self.current {
            if out.len() < snapshot.length {
                return Err(Error::Capacity);
            }
            out[..snapshot.length].copy_from_slice(&self.cache[..snapshot.length]);
        }
        Ok(self.current)
    }
    fn commit(&mut self, expected: u64, bytes: &[u8]) -> Result<Snapshot, Error> {
        if self.poisoned {
            return Err(Error::NeedsRecovery);
        }
        let revision = self.current.map_or(0, |snapshot| snapshot.revision);
        if expected != revision {
            return Err(Error::Conflict);
        }
        if bytes.len() > N {
            return Err(Error::Capacity);
        }
        let revision = revision.checked_add(1).ok_or(Error::RevisionExhausted)?;
        let snapshot = Snapshot {
            revision,
            length: bytes.len(),
        };
        let bank = if self.current.is_some() {
            1 - self.bank
        } else {
            0
        };
        self.poisoned = true;
        self.write_snapshot(bank, snapshot, bytes)?;
        self.cache[..bytes.len()].copy_from_slice(bytes);
        self.current = Some(snapshot);
        self.bank = bank;
        self.poisoned = false;
        Ok(snapshot)
    }
    fn recover(&mut self) -> Result<(), Error> {
        self.poisoned = true;
        self.medium.sync().map_err(|_| Error::Io)?;
        let mut first = [0; N];
        let mut second = [0; N];
        let a = self.read_bank(0, &mut first)?;
        let b = self.read_bank(1, &mut second)?;
        let bank = match (a, b) {
            (Some(left), Some(right)) if left.revision == right.revision => {
                return Err(Error::Corrupt);
            }
            (Some(left), Some(right)) => u32::from(right.revision > left.revision),
            (None, Some(_)) => 1,
            _ => 0,
        };
        self.current = if bank == 0 { a } else { b };
        self.cache = if bank == 0 { first } else { second };
        self.bank = bank;
        self.poisoned = false;
        Ok(())
    }
}
pub(crate) fn validate<const N: usize>(geometry: Geometry, layout: Layout) -> Result<u32, Error> {
    let Geometry {
        capacity,
        erase_size,
        write_size,
    } = geometry;
    if erase_size == 0
        || write_size == 0
        || write_size > 256
        || !layout.start.is_multiple_of(erase_size)
        || !layout.start.is_multiple_of(write_size)
        || layout.bank_bytes == 0
        || !layout.bank_bytes.is_multiple_of(erase_size)
        || !layout.bank_bytes.is_multiple_of(write_size)
    {
        return Err(Error::InvalidLayout);
    }
    let end = layout
        .bank_bytes
        .checked_mul(2)
        .and_then(|size| layout.start.checked_add(size))
        .ok_or(Error::InvalidLayout)?;
    if end > capacity {
        return Err(Error::InvalidLayout);
    }
    let header = 32_u32.div_ceil(write_size) * write_size;
    let payload = u32::try_from(N).map_err(|_| Error::InvalidLayout)?;
    let padded = payload
        .checked_add(write_size - 1)
        .ok_or(Error::InvalidLayout)?
        / write_size
        * write_size;
    let required = header
        .checked_add(padded)
        .and_then(|size| size.checked_add(write_size))
        .ok_or(Error::InvalidLayout)?;
    if required > layout.bank_bytes {
        return Err(Error::InvalidLayout);
    }
    Ok(header)
}
fn decode_u32(bytes: &[u8]) -> u32 {
    bytes.iter().enumerate().fold(0, |value, (index, byte)| {
        value | (u32::from(*byte) << (index * 8))
    })
}
fn decode_u64(bytes: &[u8]) -> u64 {
    bytes.iter().enumerate().fold(0, |value, (index, byte)| {
        value | (u64::from(*byte) << (index * 8))
    })
}
pub(crate) fn crc(bytes: &[u8]) -> u32 {
    let mut crc = Crc32c::new();
    crc.update(bytes);
    crc.finish()
}

/// Shared legacy/incremental encoding; no on-media format variant.
pub(crate) fn encode_header(
    snapshot: Snapshot,
    payload_crc: u32,
) -> Result<[u8; HEADER_BYTES], Error> {
    let mut header = [0; HEADER_BYTES];
    header[..8].copy_from_slice(MAGIC);
    header[8..16].copy_from_slice(&snapshot.revision.to_le_bytes());
    let length = u32::try_from(snapshot.length).map_err(|_| Error::Capacity)?;
    header[16..20].copy_from_slice(&length.to_le_bytes());
    header[20..24].copy_from_slice(&payload_crc.to_le_bytes());
    let check = crc(&header[..28]);
    header[28..32].copy_from_slice(&check.to_le_bytes());
    Ok(header)
}
/// Shared strict committed-header validation.
pub(crate) fn decode_header<const N: usize>(
    header: &[u8; HEADER_BYTES],
    geometry: Geometry,
    layout: Layout,
    payload_offset: u32,
) -> Result<(Snapshot, u32), Error> {
    if &header[..8] != MAGIC || crc(&header[..28]) != decode_u32(&header[28..32]) {
        return Err(Error::Corrupt);
    }
    let revision = decode_u64(&header[8..16]);
    let length = usize::try_from(decode_u32(&header[16..20])).map_err(|_| Error::Corrupt)?;
    if revision == 0 || header[24..28] != [0; 4] || length > N {
        return Err(Error::Corrupt);
    }
    let length_u32 = u32::try_from(length).map_err(|_| Error::Corrupt)?;
    if length_u32 > layout.bank_bytes - geometry.write_size - payload_offset {
        return Err(Error::Corrupt);
    }
    Ok((Snapshot { revision, length }, decode_u32(&header[20..24])))
}
