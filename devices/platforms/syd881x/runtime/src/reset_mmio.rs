//! ARM lab reset MMIO; no ROM reset or vector relocation is performed.
#![allow(
    unsafe_code,
    reason = "Existing exclusive lab reset operations form the chip boundary"
)]
use crate::reset::{CandidateResetRegisters, PowerResetRegisters, ResetRegisters};
use core::ptr::{read_volatile, write_volatile};
#[derive(Debug)]
struct Access;
impl CandidateResetRegisters for Access {
    fn clear_system_pending(&mut self, value: u32) {
        // SAFETY: Exclusive masked candidate entry owns the core ICSR W1C bits.
        unsafe { write_volatile(0xe000_ed04 as *mut u32, value) }
    }
    fn clear_sleep_configuration(&mut self) {
        // SAFETY: Exclusive privileged candidate entry owns SCR before any WFI.
        unsafe { write_volatile(0xe000_ed10 as *mut u32, 0) }
    }
    fn clear_system_priorities(&mut self) {
        // SAFETY: Exclusive privileged entry resets ARMv6-M SHPR2/SHPR3; reserved bits remain zero.
        unsafe {
            write_volatile(0xe000_ed1c as *mut u32, 0);
            write_volatile(0xe000_ed20 as *mut u32, 0);
        }
    }
}
/// Normalize the candidate's core exception state before application startup.
/// # Safety
/// Called exclusively from the candidate Reset entry, in privileged Thread mode,
/// with PRIMASK set and MSP selected. No application or interrupt task may exist.
pub unsafe fn prepare_candidate_entry() {
    crate::reset::prepare_candidate_entry(&mut Access);
}
impl PowerResetRegisters for Access {
    fn mask_interrupts(&mut self) {
        // SAFETY: Exclusive lab reset request masks CPU interrupts before reset RMW.
        unsafe {
            core::arch::asm!("cpsid i", options(nostack));
        }
    }
    fn power_control(&mut self) -> u8 {
        // SAFETY: The pinned chip exposes this byte-wide native PWR/reset register.
        unsafe { read_volatile(0x5000_100d as *const u8) }
    }
    fn set_power_control(&mut self, value: u8) {
        // SAFETY: Same exclusive reset request; the sequencer preserves other bits.
        unsafe {
            write_volatile(0x5000_100d as *mut u8, value);
        }
    }
}
impl ResetRegisters for Access {
    fn disable_interrupts(&mut self) {
        // SAFETY: Constructed only by exclusive initialized lab reset entries below.
        unsafe {
            write_volatile(0xe000_e180 as *mut u32, u32::MAX);
        }
    }
    fn clear_pending(&mut self) {
        // SAFETY: Same exclusive reset owns NVIC state.
        unsafe {
            write_volatile(0xe000_e280 as *mut u32, u32::MAX);
        }
    }
    fn stop_systick(&mut self) {
        // SAFETY: Native lab reset owns the stopped core timer.
        unsafe {
            write_volatile(0xe000_e010 as *mut u32, 0);
        }
    }
    fn watchdog(&mut self) -> u8 {
        // SAFETY: The selected chip has this byte-wide watchdog control register.
        unsafe { read_volatile(0x5000_10a0 as *const u8) }
    }
    fn set_watchdog(&mut self, value: u8) {
        // SAFETY: Exclusive lab reset preserves reserved bits through the sequencer.
        unsafe {
            write_volatile(0x5000_10a0 as *mut u8, value);
        }
    }
}
/// Performs native-lab interrupt/timer/watchdog preparation after RAM startup.
/// # Safety
/// Caller is the existing initialized native probe's exclusive reset entry, with
/// competing IRQs masked and ownership of NVIC, `SysTick` and the SYD watchdog.
/// This is not a general product wake, ROM startup or clock-switch operation.
pub unsafe fn prepare_native_lab() {
    crate::reset::prepare_native_lab(&mut Access);
}
/// Disables only watchdog control bits0/1 for the existing vendor experiment.
/// # Safety
/// Caller exclusively owns the selected chip's watchdog at vendor-lab reset.
/// Keep this mode distinct from native IRQ/timer preparation.
pub unsafe fn disable_watchdog_for_vendor_lab() {
    crate::reset::disable_watchdog(&mut Access);
}

/// Requests the exact native lab PWR reset without ROM reset or Flash cleanup.
/// This may return if the platform has not reset; the caller keeps its terminal loop.
/// # Safety
/// Caller exclusively owns the initialized chip and has stopped competing radio
/// and peripheral work according to its existing lab mode. This resets core state;
/// it is not a new product power-management or wake API.
pub unsafe fn request_native_reset_for_lab() {
    crate::reset::request_native_reset(&mut Access);
}
