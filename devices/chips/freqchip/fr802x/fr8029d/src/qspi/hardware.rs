//! SRAM-only bus command entry and FR8029D controller profile.

use super::{CAPACITY, Error, PAGE, SECTOR, XIP_BASE, boot_protection};
use core::{arch::global_asm, ptr::read_volatile};

global_asm!(include_str!("commands.s"), options(raw));
unsafe extern "C" {
    fn fr29_slot_command(
        cmd: u32,
        address: u32,
        argument: usize,
        stall: extern "C" fn() -> !,
    ) -> u32;
}

const SRAM_START: usize = 0x2000_0000;
const SRAM_END: usize = 0x2001_8000;
const STATUS: u32 = 0x0580_0000;
const WREN: u32 = 0x0600_0000;
const ERASE: u32 = 0x200a_0000;
const PROTECTED: u32 = 0x7c;

fn span_in_flash(address: u32, len: usize) -> bool {
    (address as usize).saturating_add(len) <= CAPACITY as usize
}

/// Restore the volatile P25Q80 protection baseline after Boot ROM installation.
///
/// # Safety
///
/// Use only on the FR8029D before any other QSPI or cache owner runs. The
/// SRAM command must have been copied at reset. The caller's SRAM fault
/// handler and immutable exception paths must tolerate XIP unavailability.
pub unsafe fn restore_boot_protection(stall: extern "C" fn() -> !) -> bool {
    (SRAM_START..SRAM_END).contains(&(stall as usize))
        && boot_protection::ensure(|command, value| {
            // SAFETY: The startup caller owns the device and supplied the SRAM fault handler.
            unsafe { fr29_slot_command(command, 0, value as usize, stall) }
        })
}

/// Exclusive access to the validated QSPI0 and P25Q80 pair.
#[derive(Debug)]
pub struct QspiFlash {
    stall: extern "C" fn() -> !,
}

impl QspiFlash {
    /// Check the controller, NOR ID and write-protection baseline.
    ///
    /// # Safety
    ///
    /// The caller must own QSPI0, the cache and the medium's mutation gate.
    /// Exclude other writers, DMA and debugger accesses. Copy the command
    /// routine to SRAM at reset and supply an SRAM-only fault handler. All
    /// non-maskable exception paths must be safe while XIP is unavailable.
    /// Choose the read and mutation ranges that the product owns before use.
    ///
    /// # Errors
    ///
    /// Rejects a mismatched controller, device or protection profile.
    pub unsafe fn claim(stall: extern "C" fn() -> !) -> Result<Self, Error> {
        if !(SRAM_START..SRAM_END).contains(&(stall as usize)) {
            return Err(Error::Profile);
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
        let mut device = Self { stall };
        if device.cmd(0x9fa0_0000, 0, 0) & 0xff_ffff != 0x0014_6085 {
            return Err(Error::Profile);
        }
        device.ready()?;
        Ok(device)
    }

    /// Check the locked and idle protection baseline.
    ///
    /// # Errors
    ///
    /// Returns a protection error for any unexpected status bits.
    pub fn ready(&mut self) -> Result<(), Error> {
        if self.cmd(STATUS, 0, 0) & 255 != PROTECTED || self.cmd(0x3580_0000, 0, 0) & 255 != 2 {
            return Err(Error::Protection);
        }
        Ok(())
    }

    /// Read a span at a physical NOR byte offset.
    ///
    /// # Errors
    ///
    /// Rejects spans extending past the end of the device.
    pub fn read(&mut self, address: u32, out: &mut [u8]) -> Result<(), Error> {
        if !span_in_flash(address, out.len()) {
            return Err(Error::Bounds);
        }
        for (i, byte) in out.iter_mut().enumerate() {
            // SAFETY: The checked span is within the exclusively owned mapped NOR.
            *byte = unsafe { read_volatile((XIP_BASE + address as usize + i) as *const u8) };
        }
        Ok(())
    }

    /// Erase one complete sector and restore write protection.
    ///
    /// # Errors
    ///
    /// Rejects misaligned or out-of-device offsets and unexpected protection.
    pub fn erase_sector(&mut self, address: u32) -> Result<(), Error> {
        if !(address as usize).is_multiple_of(SECTOR) || !span_in_flash(address, SECTOR) {
            return Err(Error::Bounds);
        }
        self.mutate(ERASE, address, 0)
    }

    /// Program part or all of one page from SRAM and restore write protection.
    ///
    /// # Errors
    ///
    /// Rejects empty writes, page crossings, invalid source addresses and unexpected protection.
    pub fn program(&mut self, address: u32, bytes: &[u8]) -> Result<(), Error> {
        let len = bytes.len();
        let src = bytes.as_ptr() as usize;
        if len == 0
            || len > PAGE - (address as usize % PAGE)
            || !span_in_flash(address, len)
            || !(SRAM_START..=SRAM_END - len).contains(&src)
        {
            return Err(Error::Bounds);
        }
        self.mutate(u32::try_from(len).map_err(|_| Error::Bounds)?, address, src)
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
