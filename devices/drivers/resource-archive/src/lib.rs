//! Immutable 1 KiB-block zlib archive over an injected, bounded physical reader.
//!
//! Physical archive offsets are never original resource offsets. The expected
//! original SHA must come from a separately verified manifest. CRC/Adler detect
//! corruption, not malicious replacement. This crate grants no physical writes.
#![no_std]
#![forbid(unsafe_code)]
mod inflate;
use core::fmt;
use device_core::ReadAt;

/// Decoded bytes per independently compressed block.
pub const BLOCK_BYTES: usize = 1024;
/// Original virtual image length.
pub const VIRTUAL_BYTES: u32 = 0x40_0000;
const PHYSICAL_BYTES: u32 = 0x20_0000;
const COUNT: u32 = 4096;
const DATA_START: u32 = 64 + COUNT * 12;
/// Maximum accepted compressed block extent, including zlib framing.
pub const MAX_PACKED_BYTES: u16 = 1040;

/// Archive failure; source errors retain their original identity.
#[derive(Debug, Eq, PartialEq)]
pub enum ArchiveError<E> {
    /// Physical reader failed (including Busy/recovery).
    Source(E),
    /// Requested virtual range is invalid.
    Bounds,
    /// Header, identity, canonical index or compressed syntax is invalid.
    Format,
    /// Decoded CRC32 or zlib Adler32 does not match.
    Integrity,
}

/// Read-only virtual resource view. No heap, persistent cache or write grant.
pub struct Archive<R> {
    source: R,
    used_end: u32,
}
impl<R> fmt::Debug for Archive<R> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Archive")
            .field("used_end", &self.used_end)
            .finish_non_exhaustive()
    }
}
#[derive(Clone, Copy, Debug)]
struct Entry {
    start: u32,
    packed: u16,
    crc: u32,
}
fn u16at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}
fn u32at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}
impl<R: ReadAt> Archive<R> {
    /// Validate the complete canonical index and expected original identity.
    /// Does not decode all payloads; each read independently verifies its blocks.
    ///
    /// # Errors
    /// Rejects source failures, foreign identity, unsupported format and all
    /// noncontiguous, overlapping, overflowing or out-of-range payload extents.
    pub fn open(
        mut source: R,
        expected_source_sha256: [u8; 32],
    ) -> Result<Self, ArchiveError<R::Error>> {
        if source.capacity() < PHYSICAL_BYTES {
            return Err(ArchiveError::Format);
        }
        let mut h = [0; 64];
        source.read_exact(0, &mut h).map_err(ArchiveError::Source)?;
        let used_end = u32at(&h, 28);
        if &h[..8] != b"W5RARC01"
            || u16at(&h, 8) != 1
            || u16at(&h, 10) != 1
            || u32at(&h, 12) != VIRTUAL_BYTES
            || u32at(&h, 16) != 1024
            || u32at(&h, 20) != COUNT
            || u32at(&h, 24) != DATA_START
            || !(DATA_START..=PHYSICAL_BYTES).contains(&used_end)
            || h[32..64] != expected_source_sha256
        {
            return Err(ArchiveError::Format);
        }
        let mut archive = Self { source, used_end };
        let mut next = DATA_START;
        for n in 0..COUNT {
            let e = archive.entry(n)?;
            if e.start != next {
                return Err(ArchiveError::Format);
            }
            next = e.start + u32::from(e.packed);
        }
        if next != used_end {
            return Err(ArchiveError::Format);
        }
        Ok(archive)
    }
    /// Release the physical reader without changing any state on the medium.
    pub fn into_inner(self) -> R {
        self.source
    }
    fn entry(&mut self, n: u32) -> Result<Entry, ArchiveError<R::Error>> {
        let mut b = [0; 12];
        self.source
            .read_exact(64 + n * 12, &mut b)
            .map_err(ArchiveError::Source)?;
        let start = u32at(&b, 0);
        let packed = u16at(&b, 4);
        if !(6..=MAX_PACKED_BYTES).contains(&packed)
            || u16at(&b, 6) != 1024
            || start < DATA_START
            || start
                .checked_add(u32::from(packed))
                .is_none_or(|end| end > self.used_end)
        {
            return Err(ArchiveError::Format);
        }
        Ok(Entry {
            start,
            packed,
            crc: u32at(&b, 8),
        })
    }
}
impl<R: ReadAt> ReadAt for Archive<R> {
    type Error = ArchiveError<R::Error>;
    fn capacity(&self) -> u32 {
        VIRTUAL_BYTES
    }
    fn read_exact(&mut self, offset: u32, out: &mut [u8]) -> Result<(), Self::Error> {
        let len = u32::try_from(out.len()).map_err(|_| ArchiveError::Bounds)?;
        if offset
            .checked_add(len)
            .is_none_or(|end| end > VIRTUAL_BYTES)
        {
            return Err(ArchiveError::Bounds);
        }
        let mut done = 0;
        let mut decoded = [0; BLOCK_BYTES];
        while done < out.len() {
            let at = offset + u32::try_from(done).map_err(|_| ArchiveError::Bounds)?;
            let e = self.entry(at / 1024)?;
            inflate::decode(&mut self.source, e.start, e.packed, &mut decoded)?;
            if crc32(&decoded) != e.crc {
                return Err(ArchiveError::Integrity);
            }
            let within = (at % 1024) as usize;
            let take = (BLOCK_BYTES - within).min(out.len() - done);
            out[done..done + take].copy_from_slice(&decoded[within..within + take]);
            done += take;
        }
        Ok(())
    }
}
fn crc32(bytes: &[u8]) -> u32 {
    let mut c = !0_u32;
    for b in bytes {
        c ^= u32::from(*b);
        for _ in 0..8 {
            c = (c >> 1) ^ ((0_u32.wrapping_sub(c & 1)) & 0xedb8_8320);
        }
    }
    !c
}
