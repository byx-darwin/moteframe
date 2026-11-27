//! Physical FR8029D board backend. No boot-time binding or write is implicit.
use core::{arch::global_asm, ptr::read_volatile};
use fr8029d_bringup::qspi_config::{Error, PAGE, Partition, SECTOR};
use moteframe_storage::write::{Slot, SlotRegion};

global_asm!(include_str!("qspi_slots.s"), options(raw));
unsafe extern "C" {
    static __fr29_image_end: u8;
    fn fr29_slot_command(
        cmd: u32,
        address: u32,
        argument: usize,
        stall: extern "C" fn() -> !,
    ) -> u32;
}
const STATUS: u32 = 0x0580_0000;
const WREN: u32 = 0x0600_0000;
const ERASE: u32 = 0x200a_0000;
const PAGE_DAC: u32 = 0; // Software selector, never issued as a flash opcode.
const PROTECTED: u32 = 0x7c;

/// Owns QSPI0 and the two checked configuration slots during an approved window.
#[derive(Debug)]
pub struct QspiSlots {
    partition: Partition,
    stall: extern "C" fn() -> !,
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
        if !(0x2000_0000..0x2001_8000).contains(&(stall as usize)) {
            return Err(Error::Profile);
        }
        let linked_end = (&raw const __fr29_image_end) as usize;
        let slot_start = partition.address(Slot::A, 0, SECTOR)? as usize;
        if !(0x1000_2000..=0x1000_0000 + slot_start).contains(&linked_end) {
            return Err(Error::Bounds);
        }
        let reg = |o: usize| {
            // SAFETY: Claim requires exclusive access to the FR8029D QSPI0 register bank.
            unsafe { read_volatile((0xe00a_4000 + o) as *const u32) }
        };
        let polling = reg(0x38);
        // SDK v0.3.6 disables automatic polling for DAC page writes. Only
        // admit the observed single-wire page-program profile in this mode;
        // the SRAM command waits for controller idle and polls NOR WIP itself.
        let sdk_manual = polling == 0x1002_4005 && reg(8) == 2;
        if reg(0) & 0x7fff_ffff != 0x0089_0087
            || reg(8) != 2
            || reg(0x14) != 0x0010_1002
            || reg(0x24) != 0
            || (polling & (1 << 14) != 0 && !sdk_manual)
            || reg(0x58) != 0
        {
            return Err(Error::Profile);
        }
        let mut device = Self { partition, stall };
        if device.cmd(0x9fa0_0000, 0, 0) & 0xff_ffff != 0x0014_6085 {
            return Err(Error::Profile);
        }
        device.ready()?;
        Ok(device)
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

    fn cmd(&mut self, cmd: u32, address: u32, arg: usize) -> u32 {
        // SAFETY: This private API accepts only the commands constructed below;
        // the assembler and timeout destination both execute in SRAM.
        unsafe { fr29_slot_command(cmd, address, arg, self.stall) }
    }

    fn protection(&mut self, bits: u32) -> Result<(), Error> {
        self.cmd(0x5000_0000, 0, 0); // volatile status-register write enable
        self.cmd(0x0100_8000, 0, bits as usize);
        if self.cmd(STATUS, 0, 0) & 255 != bits {
            return Err(Error::Protection);
        }
        Ok(())
    }

    fn mutate(&mut self, op: u32, address: u32, arg: usize) -> Result<(), Error> {
        self.ready()?;
        let operation = self.protection(0).and_then(|()| {
            if op == ERASE {
                self.cmd(WREN, 0, 0);
                if self.cmd(STATUS, 0, 0) & 255 != 2 {
                    return Err(Error::Protection);
                }
            }
            self.cmd(op, address, arg);
            Ok(())
        });
        // Always re-establish and verify the prior protection before returning.
        if self.cmd(STATUS, 0, 0) & 2 != 0 {
            self.cmd(0x0400_0000, 0, 0);
        }
        if self.protection(PROTECTED).is_err() || self.ready().is_err() {
            // SAFETY: Claim requires an exclusive board-level recovery path.
            unsafe { core::arch::asm!("cpsid i", options(nostack, preserves_flags)) };
            (self.stall)();
        }
        operation
    }
}

impl SlotRegion<PAGE> for QspiSlots {
    type Error = Error;
    fn slot_bytes(&self) -> usize {
        SECTOR
    }
    fn ready(&mut self) -> Result<(), Error> {
        if self.cmd(STATUS, 0, 0) & 255 != PROTECTED || self.cmd(0x3580_0000, 0, 0) & 255 != 2 {
            return Err(Error::Protection);
        }
        Ok(())
    }
    fn read(&mut self, slot: Slot, offset: usize, out: &mut [u8]) -> Result<(), Error> {
        let addr = self.partition.address(slot, offset, out.len())? as usize;
        for (i, byte) in out.iter_mut().enumerate() {
            // SAFETY: The checked span is within the exclusively owned mapped NOR.
            *byte = unsafe { read_volatile((0x1000_0000 + addr + i) as *const u8) };
        }
        Ok(())
    }
    fn prepare(&mut self, slot: Slot) -> Result<(), Error> {
        let offset = self.partition.address(slot, 0, SECTOR)?;
        self.mutate(ERASE, offset, 0)
    }
    fn program_page(&mut self, slot: Slot, offset: usize, page: &[u8; PAGE]) -> Result<(), Error> {
        if !offset.is_multiple_of(PAGE) {
            return Err(Error::Bounds);
        }
        let address = self.partition.address(slot, offset, PAGE)?;
        // XIP cannot source the page while the direct access port programs it.
        if !(0x2000_0000..=0x2001_8000 - PAGE).contains(&(page.as_ptr() as usize)) {
            return Err(Error::Bounds);
        }
        self.mutate(PAGE_DAC, address, page.as_ptr() as usize)
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
    use super::{ERASE, QspiSlots};
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
            for (i, byte) in dest.iter_mut().enumerate() {
                // SAFETY: Checked XIP window and the exclusive QSPI claim.
                *byte =
                    unsafe { ((0x1000_0000 + offset as usize + i) as *const u8).read_volatile() };
            }
            Ok(())
        }
        fn erase(&mut self, offset: u32) -> Result<(), Error> {
            if !offset.is_multiple_of(4096) || !(inside(offset, 4096, STAGING) || offset == INFO) {
                return Err(Error::Invalid);
            }
            self.mutate(ERASE, offset, 0).map_err(|_| Error::Media)
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
                marker.as_ptr()
            } else {
                bytes.as_ptr()
            };
            if !(0x2000_0000..=0x2001_8000 - len).contains(&(src as usize)) {
                return Err(Error::Invalid);
            }
            self.mutate(
                u32::try_from(len).map_err(|_| Error::Invalid)?,
                offset,
                src as usize,
            )
            .map_err(|_| Error::Media)
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
