//! ARM-only MMIO, critical sections and GPIO IRQ bridge for the selected chip.
//!
//! No low-RAM vector relocation or Timer3 setup is performed: ROM owns them.
#![allow(
    unsafe_code,
    reason = "Volatile MMIO, PRIMASK and IRQ entry form the board hardware boundary"
)]

use crate::registers::{Register8, Register32, Registers};
use core::arch::asm;
use core::ptr::{addr_of_mut, read_volatile, write_volatile};
use device_core::DelayMs;

static mut CLAIMED: bool = false;

/// MMIO ownership was already claimed; it cannot be recreated after drop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlreadyTaken;

/// Executes a non-reentrant register operation with mask restoration.
pub fn critical<T>(operation: impl FnOnce() -> T) -> T {
    let previous: u32;
    // SAFETY: ARMv6-M PRIMASK access; memory-clobbering asm prevents reordering.
    unsafe {
        asm!("mrs {old}, PRIMASK", "cpsid i", old=out(reg) previous, options(nostack));
    }
    let result = operation();
    if previous & 1 == 0 {
        // SAFETY: Restore only the interrupt state that this function changed.
        unsafe {
            asm!("cpsie i", options(nostack));
        }
    }
    result
}

/// Exclusive chip register token. It is not Clone and is never released on drop.
#[derive(Debug)]
pub struct Mmio {
    _private: (),
}
impl Mmio {
    /// Claims the actual chip register bank once.
    /// # Safety
    /// Must run on SYD8810 with its peripheral mapping available, after C/Rust RAM
    /// startup. No other HAL may own these peripherals. ROM/BLE clocks and shared
    /// RAM must follow the selected SDK boot contract. Claim is foreground only.
    /// # Errors
    /// Returns `AlreadyTaken` after the first claim, including after its drop.
    pub unsafe fn claim() -> Result<Self, AlreadyTaken> {
        critical(|| {
            let pointer = addr_of_mut!(CLAIMED);
            // SAFETY: The static is exclusively accessed under PRIMASK protection.
            if unsafe { read_volatile(pointer) } {
                return Err(AlreadyTaken);
            }
            // SAFETY: Same critical section; no mutable reference escapes.
            unsafe {
                write_volatile(pointer, true);
            }
            Ok(Self { _private: () })
        })
    }
}
impl Registers for Mmio {
    fn read8(&self, register: Register8) -> u8 {
        // SAFETY: Typed register variants are valid byte MMIO locations on this chip.
        unsafe { read_volatile(register.address() as *const u8) }
    }
    fn write8(&self, register: Register8, value: u8) {
        // SAFETY: Typed register variants are valid byte MMIO locations on this chip.
        unsafe {
            write_volatile(register.address() as *mut u8, value);
        }
    }
    fn read32(&self, register: Register32) -> u32 {
        // SAFETY: All word register variants are valid and four-byte aligned.
        unsafe { read_volatile(register.address() as *const u32) }
    }
    fn write32(&self, register: Register32, value: u32) {
        // SAFETY: All word register variants are valid and four-byte aligned.
        unsafe {
            write_volatile(register.address() as *mut u32, value);
        }
    }
    fn update8(&self, register: Register8, clear: u8, set: u8) {
        critical(|| self.write8(register, (self.read8(register) & !clear) | set));
    }
    fn update32(&self, register: Register32, clear: u32, set: u32) {
        critical(|| self.write32(register, (self.read32(register) & !clear) | set));
    }
}

/// Reads and acknowledges active GPIO sources for the runtime IRQ bridge.
/// # Safety
/// The caller owns interrupt routing on an initialized SYD chip and serializes access.
#[must_use]
pub unsafe fn acknowledge_gpio_interrupt() -> u32 {
    let registers = Mmio { _private: () };
    let active =
        registers.read32(Register32::GpioPending) & !registers.read32(Register32::GpioMask);
    if active != 0 {
        registers.write32(Register32::GpioClear, active);
    }
    active
}

/// Conservative reset-delay source, assuming the CPU never exceeds 64 MHz.
/// Every iteration includes a NOP, so loop overhead makes delays longer, not shorter.
/// This is not a calibrated timer; it intentionally avoids ROM-owned Timer3/RTC.
#[derive(Debug, Default)]
pub struct BusyDelay;
impl DelayMs for BusyDelay {
    fn delay_ms(&mut self, milliseconds: u16) {
        for _ in 0..milliseconds {
            for _ in 0..64_000 {
                // SAFETY: NOP has no memory effect, and executes on ARMv6-M.
                unsafe {
                    asm!("nop", options(nomem, nostack, preserves_flags));
                }
            }
        }
    }
}

#[cfg(feature = "native-radio")]
mod radio;
#[cfg(feature = "native-radio")]
pub use radio::{RadioMmio, RawRadioMmio};
