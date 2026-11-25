//! Bounded exact reads; no write, erase or mutable memory interface.
use device_core::ReadAt;
/// Invalid extent, coordinates or unsupported chunk size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounds;
/// Source failure distinguished from validation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadError<E> {
    /// Rejected before I/O.
    Bounds,
    /// Underlying exact read failed.
    Source(E),
}
/// Validated byte extent in one immutable source version.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    start: u32,
    len: u32,
}
impl Region {
    /// Validates the complete extent without overflowing.
    /// # Errors
    /// Rejects an extent beyond capacity.
    pub fn new(start: u32, len: u32, capacity: u32) -> Result<Self, Bounds> {
        if start.checked_add(len).is_none_or(|end| end > capacity) {
            return Err(Bounds);
        }
        Ok(Self { start, len })
    }
    /// Byte count.
    #[must_use]
    pub const fn len(self) -> u32 {
        self.len
    }
    /// Whether this extent is empty.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.len == 0
    }
    /// Absolute source offset.
    #[must_use]
    pub const fn start(self) -> u32 {
        self.start
    }
    /// Reads within this region, rechecking the current source capacity.
    /// # Errors
    /// Invalid ranges do not touch the source; transport errors propagate.
    pub fn read<S: ReadAt>(
        self,
        source: &mut S,
        offset: u32,
        out: &mut [u8],
    ) -> Result<(), ReadError<S::Error>> {
        let count = u32::try_from(out.len()).map_err(|_| ReadError::Bounds)?;
        if offset.checked_add(count).is_none_or(|end| end > self.len)
            || self
                .start
                .checked_add(self.len)
                .is_none_or(|end| end > source.capacity())
        {
            return Err(ReadError::Bounds);
        }
        if out.is_empty() {
            return Ok(());
        }
        source
            .read_exact(self.start + offset, out)
            .map_err(ReadError::Source)
    }
}
/// Borrowed immutable bytes for host tools or memory-mapped resources.
#[derive(Clone, Copy)]
pub struct Memory<'a> {
    bytes: &'a [u8],
    len: u32,
}
impl<'a> Memory<'a> {
    /// Checks that source length fits the32-bit offset contract.
    /// # Errors
    /// Rejects sources larger than `u32::MAX`.
    pub fn new(bytes: &'a [u8]) -> Result<Self, Bounds> {
        Ok(Self {
            bytes,
            len: u32::try_from(bytes.len()).map_err(|_| Bounds)?,
        })
    }
}
impl ReadAt for Memory<'_> {
    type Error = Bounds;
    fn capacity(&self) -> u32 {
        self.len
    }
    fn read_exact(&mut self, offset: u32, out: &mut [u8]) -> Result<(), Bounds> {
        let start = usize::try_from(offset).map_err(|_| Bounds)?;
        let end = start.checked_add(out.len()).ok_or(Bounds)?;
        let bytes = self.bytes.get(start..end).ok_or(Bounds)?;
        out.copy_from_slice(bytes);
        Ok(())
    }
}

impl core::fmt::Debug for Memory<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Memory").field("len", &self.len).finish()
    }
}
