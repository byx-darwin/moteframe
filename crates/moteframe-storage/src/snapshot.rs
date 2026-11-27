// SPDX-License-Identifier: Apache-2.0
//! V1 configuration snapshots with a dedicated final commit page.
use core::ops::RangeInclusive;

use crate::{
    deferred::ConfigMedia,
    write::{Slot, SlotRegion},
};

const HEADER: usize = 28;
const TAG: &[u8; 8] = b"MFCF\x01\x00\x1c\x00";
const DONE: &[u8; 4] = b"DONE";

/// A failed operation preserves the other slot. Reconcile ambiguous writes
/// before interpreting persistent state.
#[derive(Debug, PartialEq, Eq)]
pub enum StoreError<E> {
    /// Board operation failed.
    Media(E),
    /// Unsupported slot geometry or product policy.
    Layout,
    /// Unsupported product version.
    Version,
    /// Payload, output or read range does not fit.
    Length,
    /// Invalid marker, header, checksum or conflicting generations.
    InvalidRecord,
    /// Generation cannot be incremented.
    Exhausted,
    /// No confirmed record is selected.
    NoSelection,
}
use StoreError::{InvalidRecord, Length, Media};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Record {
    slot: Slot,
    version: u32,
    seq: u64,
    len: usize,
}

#[derive(Clone, Copy)]
enum Contents {
    Blank,
    Interrupted,
    Valid(Record),
}

/// No heap or resident configuration buffer. PAGE is the program page size;
/// the last page of each slot is reserved for its commit marker.
#[derive(Debug)]
pub struct ConfigStore<'a, R, const PAGE: usize> {
    region: &'a mut R,
    end: usize,
    limit: usize,
    versions: RangeInclusive<u32>,
    current: Option<Record>,
}

fn crc_update(mut crc: u32, bytes: &[u8]) -> u32 {
    for byte in bytes {
        crc ^= u32::from(*byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ ((0u32.wrapping_sub(crc & 1)) & 0xedb8_8320);
        }
    }
    crc
}

impl<'a, R: SlotRegion<PAGE>, const PAGE: usize> ConfigStore<'a, R, PAGE> {
    /// Read both slots without changing the medium. A and B are supplied by
    /// the board as a single exclusive region; no absolute addresses are assumed.
    ///
    /// # Errors
    /// Reject invalid geometry, unreadable slots or an ambiguous selection.
    pub fn open(
        region: &'a mut R,
        limit: usize,
        versions: RangeInclusive<u32>,
    ) -> Result<Self, StoreError<R::Error>> {
        let size = region.slot_bytes();
        let end = size.checked_sub(PAGE).ok_or(StoreError::Layout)?;
        if PAGE < 4
            || !size.is_multiple_of(PAGE)
            || end < HEADER
            || limit > end - HEADER
            || u32::try_from(limit).is_err()
            || versions.is_empty()
        {
            return Err(StoreError::Layout);
        }
        let mut store = Self {
            region,
            end,
            limit,
            versions,
            current: None,
        };
        store.recover()?;
        Ok(store)
    }

    fn inspect(&mut self, slot: Slot) -> Result<Contents, StoreError<R::Error>> {
        let mut buf = [0; 32];
        let mut marked = true;
        let mut blank = true;
        for off in (0..PAGE).step_by(buf.len()) {
            let n = buf.len().min(PAGE - off);
            self.region
                .read(slot, self.end + off, &mut buf[..n])
                .map_err(Media)?;
            for (i, &v) in buf[..n].iter().enumerate() {
                marked &= v == DONE.get(off + i).copied().unwrap_or(255);
                blank &= v == 255;
            }
        }
        if blank {
            for off in (0..self.end).step_by(buf.len()) {
                let n = buf.len().min(self.end - off);
                self.region.read(slot, off, &mut buf[..n]).map_err(Media)?;
                blank &= buf[..n].iter().all(|&v| v == 255);
            }
            return Ok(if blank {
                Contents::Blank
            } else {
                Contents::Interrupted
            });
        }
        if !marked {
            return Err(InvalidRecord);
        }
        let mut header = [0; HEADER];
        self.region.read(slot, 0, &mut header).map_err(Media)?;
        if &header[..8] != TAG {
            return Err(InvalidRecord);
        }
        let field =
            |i| u32::from_le_bytes([header[i], header[i + 1], header[i + 2], header[i + 3]]);
        let version = field(8);
        let seq = u64::from(field(12)) | (u64::from(field(16)) << 32);
        let len = usize::try_from(field(20)).map_err(|_| Length)?;
        if seq == 0 || len > self.end - HEADER {
            return Err(InvalidRecord);
        }
        let mut crc = crc_update(u32::MAX, &header[..24]);
        for off in (0..len).step_by(buf.len()) {
            let n = buf.len().min(len - off);
            self.region
                .read(slot, HEADER + off, &mut buf[..n])
                .map_err(Media)?;
            crc = crc_update(crc, &buf[..n]);
        }
        if crc != !field(24) {
            return Err(InvalidRecord);
        }
        Ok(Contents::Valid(Record {
            slot,
            version,
            seq,
            len,
        }))
    }

    fn identical(&mut self, a: Record, b: Record) -> Result<bool, StoreError<R::Error>> {
        if a.version != b.version || a.len != b.len {
            return Ok(false);
        }
        let mut left = [0; 32];
        let mut right = [0; 32];
        for off in (0..a.len).step_by(left.len()) {
            let n = left.len().min(a.len - off);
            self.region
                .read(a.slot, HEADER + off, &mut left[..n])
                .map_err(Media)?;
            self.region
                .read(b.slot, HEADER + off, &mut right[..n])
                .map_err(Media)?;
            if left[..n] != right[..n] {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Reconcile and rescan both slots on this instance, without writing.
    ///
    /// # Errors
    /// A busy, unreadable, unsupported or ambiguous record clears the selection.
    pub fn recover(&mut self) -> Result<(), StoreError<R::Error>> {
        use Contents::{Blank, Interrupted, Valid};

        self.current = None;
        self.region.ready().map_err(Media)?;
        let pair = (self.inspect(Slot::A)?, self.inspect(Slot::B)?);
        let rec = match pair {
            (Valid(a), Valid(b)) if a.seq == b.seq => {
                if !self.identical(a, b)? {
                    return Err(InvalidRecord);
                }
                Some(a)
            }
            (Valid(a), Valid(b)) => Some(if a.seq > b.seq { a } else { b }),
            (Valid(r), _) | (_, Valid(r)) => Some(r),
            (Blank | Interrupted, Blank) => None,
            _ => return Err(InvalidRecord),
        };
        if let Some(r) = rec {
            if !self.versions.contains(&r.version) {
                return Err(StoreError::Version);
            }
            if r.len > self.limit {
                return Err(Length);
            }
        }
        self.current = rec;
        Ok(())
    }

    /// Load the most recent valid snapshot. Empty media returns `None`.
    ///
    /// # Errors
    /// Rejects insufficient output before writing any bytes. Discard output
    /// after a driver read failure.
    pub fn load(&mut self, out: &mut [u8]) -> Result<Option<(u32, usize)>, StoreError<R::Error>> {
        let Some((ver, len)) = self.selected()? else {
            return Ok(None);
        };
        self.read_selected(0, out.get_mut(..len).ok_or(Length)?)?;
        Ok(Some((ver, len)))
    }

    fn verify(
        &mut self,
        slot: Slot,
        offset: usize,
        bytes: &[u8],
    ) -> Result<bool, StoreError<R::Error>> {
        let mut buf = [0; 32];
        let mut pos = offset;
        for chunk in bytes.chunks(buf.len()) {
            self.region
                .read(slot, pos, &mut buf[..chunk.len()])
                .map_err(Media)?;
            if &buf[..chunk.len()] != chunk {
                return Ok(false);
            }
            pos += chunk.len();
        }
        Ok(true)
    }

    fn save(&mut self, version: u32, data: &[u8]) -> Result<(), StoreError<R::Error>> {
        if data.len() > self.limit {
            return Err(Length);
        }
        if !self.versions.contains(&version) {
            return Err(StoreError::Version);
        }
        self.recover()?;
        let (slot, seq) = if let Some(r) = self.current {
            if r.version == version && r.len == data.len() && self.verify(r.slot, HEADER, data)? {
                return Ok(());
            }
            (
                if r.slot == Slot::A { Slot::B } else { Slot::A },
                r.seq.checked_add(1).ok_or(StoreError::Exhausted)?,
            )
        } else {
            (Slot::A, 1)
        };
        let mut header = [0; HEADER];
        header[..8].copy_from_slice(TAG);
        header[8..12].copy_from_slice(&version.to_le_bytes());
        header[12..20].copy_from_slice(&seq.to_le_bytes());
        header[20..24]
            .copy_from_slice(&u32::try_from(data.len()).map_err(|_| Length)?.to_le_bytes());
        let crc = !crc_update(crc_update(u32::MAX, &header[..24]), data);
        header[24..].copy_from_slice(&crc.to_le_bytes());
        self.current = None;
        self.region.prepare(slot).map_err(Media)?;
        let mut page = [255; PAGE];
        if !self.verify(slot, self.end, &page)? {
            return Err(InvalidRecord);
        }
        for off in (0..HEADER + data.len()).step_by(PAGE) {
            page.fill(255);
            let h = HEADER.saturating_sub(off).min(PAGE);
            if h > 0 {
                page[..h].copy_from_slice(&header[off..off + h]);
            }
            let start = off.saturating_sub(HEADER);
            let n = (PAGE - h).min(data.len() - start);
            page[h..h + n].copy_from_slice(&data[start..start + n]);
            self.region.program_page(slot, off, &page).map_err(Media)?;
            if !self.verify(slot, off, &page)? {
                return Err(InvalidRecord);
            }
        }
        page.fill(255);
        page[..4].copy_from_slice(DONE);
        self.region
            .program_page(slot, self.end, &page)
            .map_err(Media)?;
        self.recover()?;
        if self.current
            != Some(Record {
                slot,
                version,
                seq,
                len: data.len(),
            })
            || !self.verify(slot, HEADER, data)?
        {
            return Err(InvalidRecord);
        }
        Ok(())
    }
}

impl<R: SlotRegion<PAGE>, const PAGE: usize> ConfigMedia for ConfigStore<'_, R, PAGE> {
    type Error = StoreError<R::Error>;
    fn selected(&mut self) -> Result<Option<(u32, usize)>, Self::Error> {
        self.recover()?;
        Ok(self.current.map(|r| (r.version, r.len)))
    }
    fn read_selected(&mut self, offset: usize, out: &mut [u8]) -> Result<(), Self::Error> {
        let r = self.current.ok_or(StoreError::NoSelection)?;
        if offset > r.len || out.len() > r.len - offset {
            return Err(Length);
        }
        self.region
            .read(r.slot, HEADER + offset, out)
            .map_err(Media)
    }
    fn commit(&mut self, version: u32, bytes: &[u8]) -> Result<(), Self::Error> {
        self.save(version, bytes)
    }
}
