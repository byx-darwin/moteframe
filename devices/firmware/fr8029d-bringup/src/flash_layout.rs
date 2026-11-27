//! FR8029D 1 MiB product layout v1. All ranges are physical, end-exclusive.
use core::ops::Range;

/// Installed raw XIP payload including the vector table and RAM initializers.
pub const APP: Range<u32> = 0x02000..0x3c000;
/// SDK `FlashDB` storage.
pub const FLASH_ENV: Range<u32> = APP.end..0x40000;
/// First configuration erase sector.
pub const CONFIG_A: Range<u32> = FLASH_ENV.end..0x41000;
/// Second configuration erase sector.
pub const CONFIG_B: Range<u32> = CONFIG_A.end..0x42000;
/// Dedicated OTA source window sized for the largest installable image.
pub const STAGING: Range<u32> = CONFIG_B.end..CONFIG_B.end + (APP.end - APP.start);
/// Copy granularity recorded in the SDK v0.3.6 OTA installation header.
pub const COPY_UNIT: u32 = 8192;

/// Approved physical footprints for a single raw, uncompressed application.
#[derive(Clone, Copy, Debug)]
pub struct UpgradePlan {
    footprint: u32,
}

impl UpgradePlan {
    /// Check the complete plan before issuing any mutation. The package length
    /// comes from the actual payload (or a verified length-bound manifest for
    /// streaming). Accept only fixed v1 offsets and copy semantics.
    #[must_use]
    pub fn check(
        declared_bytes: u32,
        package_bytes: usize,
        source: u32,
        destination: u32,
        copy_unit: u32,
    ) -> Option<Self> {
        if declared_bytes == 0
            || u32::try_from(package_bytes).ok()? != declared_bytes
            || source != STAGING.start
            || destination != APP.start
            || copy_unit != COPY_UNIT
        {
            return None;
        }
        let footprint = declared_bytes.checked_next_multiple_of(COPY_UNIT)?;
        if source.checked_add(footprint)? > STAGING.end
            || destination.checked_add(footprint)? > APP.end
        {
            return None;
        }
        Some(Self { footprint })
    }

    /// Erase the last source block through its padding before receiving data.
    #[must_use]
    pub fn staging(self) -> Range<u32> {
        STAGING.start..STAGING.start + self.footprint
    }

    /// Maximum destination footprint, including padding of the last ROM block.
    #[must_use]
    pub fn destination(self) -> Range<u32> {
        APP.start..APP.start + self.footprint
    }
}
