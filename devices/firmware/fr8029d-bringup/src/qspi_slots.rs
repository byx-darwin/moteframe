//! Product configuration and OTA ranges over the FR8029D QSPI driver.
use fr8029d_bringup::{
    flash_layout::APP,
    qspi_config::{Error, PAGE, Partition, SECTOR},
};
use fr8029d_chip::qspi::{QspiFlash, XIP_BASE};
use moteframe_storage::write::{Slot, SlotRegion};

unsafe extern "C" {
    static __fr29_image_end: u8;
}

/// Owns QSPI0 and the two checked configuration slots during an approved window.
#[derive(Debug)]
pub struct QspiSlots {
    partition: Partition,
    flash: QspiFlash,
}

impl QspiSlots {
    /// Acquire the device only for the approved board layout and time window.
    ///
    /// # Safety
    /// The caller owns QSPI0, the entire medium's mutation gate, and the cache.
    /// It must exclude other writers, DMA and debugger accesses, including after
    /// an interrupted operation. The immutable boot/application vectors, NMI and
    /// fault paths must be safe during XIP unavailability. The SRAM-only stall
    /// handler must quarantine the medium without fetching from Flash or returning.
    /// XIP timing and the board's independent recovery policy require board sign-off.
    ///
    /// # Errors
    /// Refuses a different chip, remap/controller profile or protection baseline.
    pub unsafe fn claim(partition: Partition, stall: extern "C" fn() -> !) -> Result<Self, Error> {
        let linked_end = (&raw const __fr29_image_end) as usize;
        let slot_start = partition.address(Slot::A, 0, SECTOR)? as usize;
        if !(XIP_BASE + APP.start as usize..=XIP_BASE + slot_start).contains(&linked_end) {
            return Err(Error::Bounds);
        }
        // SAFETY: The firmware caller reserves QSPI0, the cache and the SRAM exception paths.
        let flash = unsafe { QspiFlash::claim(stall) }?;
        Ok(Self { partition, flash })
    }

    /// Open both slots and recover the latest committed configuration.
    ///
    /// # Errors
    /// Propagates incompatible layouts, versions and storage read failures.
    pub fn open_config(
        &mut self,
        limit: usize,
        versions: core::ops::RangeInclusive<u32>,
    ) -> Result<
        moteframe_storage::snapshot::ConfigStore<'_, Self, PAGE>,
        moteframe_storage::snapshot::StoreError<Error>,
    > {
        moteframe_storage::snapshot::ConfigStore::open(self, limit, versions)
    }
}

impl SlotRegion<PAGE> for QspiSlots {
    type Error = Error;
    fn slot_bytes(&self) -> usize {
        SECTOR
    }
    fn ready(&mut self) -> Result<(), Error> {
        self.flash.ready()
    }
    fn read(&mut self, slot: Slot, offset: usize, out: &mut [u8]) -> Result<(), Error> {
        self.flash
            .read(self.partition.address(slot, offset, out.len())?, out)
    }
    fn prepare(&mut self, slot: Slot) -> Result<(), Error> {
        self.flash
            .erase_sector(self.partition.address(slot, 0, SECTOR)?)
    }
    fn program_page(&mut self, slot: Slot, offset: usize, page: &[u8; PAGE]) -> Result<(), Error> {
        if !offset.is_multiple_of(PAGE) {
            return Err(Error::Bounds);
        }
        self.flash
            .program(self.partition.address(slot, offset, PAGE)?, page)
    }
}

#[cfg(feature = "ble-gatt-ota")]
#[unsafe(link_section = ".ramfunc.ota_stall")]
pub(crate) extern "C" fn ota_stall() -> ! {
    loop {
        core::hint::spin_loop();
    }
}

#[cfg(feature = "ble-gatt-ota")]
mod ota_medium {
    use super::{Error as QspiError, QspiSlots};
    use fr8029d_bringup::{
        flash_layout::STAGING,
        ota::{Error, INFO, MARKER, Medium},
    };

    fn inside(offset: u32, len: usize, range: core::ops::Range<u32>) -> bool {
        offset >= range.start
            && u32::try_from(len)
                .ok()
                .and_then(|n| offset.checked_add(n))
                .is_some_and(|end| end <= range.end)
    }

    impl Medium for QspiSlots {
        fn read(&mut self, offset: u32, dest: &mut [u8]) -> Result<(), Error> {
            if !(inside(offset, dest.len(), STAGING)
                || inside(offset, dest.len(), INFO..MARKER + 4))
            {
                return Err(Error::Invalid);
            }
            self.flash.read(offset, dest).map_err(|_| Error::Media)
        }
        fn erase(&mut self, offset: u32) -> Result<(), Error> {
            if !offset.is_multiple_of(4096) || !(inside(offset, 4096, STAGING) || offset == INFO) {
                return Err(Error::Invalid);
            }
            self.flash.erase_sector(offset).map_err(|_| Error::Media)
        }
        fn program(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Error> {
            let len = bytes.len();
            if !((len == 256 && offset.is_multiple_of(256) && inside(offset, len, STAGING))
                || (offset == INFO && len == 48)
                || (offset == MARKER && len == 4))
            {
                return Err(Error::Invalid);
            }
            // A promoted marker constant may reside in XIP. Copy it before the bus becomes busy.
            let mut marker = [0_u8; 4];
            let src = if len == 4 {
                core::hint::black_box(&mut marker).copy_from_slice(bytes);
                &marker
            } else {
                bytes
            };
            self.flash
                .program(offset, src)
                .map_err(|error| match error {
                    QspiError::Bounds => Error::Invalid,
                    _ => Error::Media,
                })
        }
    }
}

#[cfg(feature = "ble-gatt-ota")]
pub(crate) unsafe fn ota_receiver() -> Result<fr8029d_bringup::ota::Receiver<QspiSlots>, Error> {
    // SAFETY: The board foreground transfers its exclusive QSPI and cache ownership.
    let media = unsafe { QspiSlots::claim(Partition::layout_v1(), ota_stall) }?;
    Ok(fr8029d_bringup::ota::Receiver::new(
        media,
        *include_bytes!(concat!(env!("OUT_DIR"), "/ota-manifest.bin")),
    ))
}
