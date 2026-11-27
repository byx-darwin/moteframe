//! FR8029D/P25Q80 configuration partition, in physical byte offsets.
use core::ops::Range;
use moteframe_storage::write::Slot;

pub use fr8029d_chip::qspi::{CAPACITY, Error, PAGE, SECTOR};

/// Two consecutive erase sectors in a caller-owned layout.
#[derive(Clone, Copy, Debug)]
pub struct Partition(u32);
impl Partition {
    /// Configuration sectors of product layout v1. The caller must verify
    /// ownership across installed firmware, rollback images and update tools
    /// before claiming the physical QSPI backend.
    #[must_use]
    pub const fn layout_v1() -> Self {
        Self(crate::flash_layout::CONFIG_A.start)
    }

    /// Validate a board-approved layout. `application_end` includes maximum
    /// application growth; `reserved` must cover every staging, copy, factory
    /// and retained region, irrespective of update transport.
    ///
    /// # Errors
    /// Reject overlaps, malformed ranges, unaligned bases and overflow.
    pub fn new(start: u32, application_end: u32, reserved: &[Range<u32>]) -> Result<Self, Error> {
        let end = start.checked_add(8192).ok_or(Error::Bounds)?;
        if application_end < 0x2000
            || start < application_end
            || !start.is_multiple_of(4096)
            || end > CAPACITY
            || reserved
                .iter()
                .any(|r| r.start >= r.end || r.end > CAPACITY || (start < r.end && r.start < end))
        {
            return Err(Error::Bounds);
        }
        Ok(Self(start))
    }

    /// Translate one slot-relative span. The end of A never aliases B.
    ///
    /// # Errors
    /// Reject even partially out-of-slot requests.
    pub fn address(self, slot: Slot, offset: usize, len: usize) -> Result<u32, Error> {
        if offset > SECTOR || len > SECTOR - offset {
            return Err(Error::Bounds);
        }
        Ok(self.0
            + if slot == Slot::A { 0 } else { 4096 }
            + u32::try_from(offset).map_err(|_| Error::Bounds)?)
    }
}
