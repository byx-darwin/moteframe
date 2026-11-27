//! FR8029D QSPI0 operations for the validated P25Q80 profile.

/// CPU address of the first byte in the QSPI XIP window.
pub const XIP_BASE: usize = 0x1000_0000;

/// Physical size of the validated P25Q80 chip.
pub const CAPACITY: u32 = 1 << 20;
/// Independent erase sector size.
pub const SECTOR: usize = 4096;
/// Direct access program granularity used by the snapshot format.
pub const PAGE: usize = 256;

/// Physical Flash or controller precondition failed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// Geometry, alignment, offset or a protected range conflicts.
    Bounds,
    /// Chip ID or controller profile differs from the supported board.
    Profile,
    /// Unexpected protection or write-enable status.
    Protection,
}

#[cfg(any(target_arch = "arm", test))]
mod boot_protection;

#[cfg(target_arch = "arm")]
mod hardware;

#[cfg(target_arch = "arm")]
pub use hardware::{QspiFlash, restore_boot_protection};
