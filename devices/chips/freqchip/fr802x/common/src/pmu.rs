//! FR802x PMU byte transactions over the on-chip serial port.

use core::ptr::{read_volatile, write_volatile};

const FRSPIM: usize = 0xe008_0000;

/// Execute a PMU byte read or write with a bounded completion wait.
///
/// # Safety
///
/// The FR802x serial port clock must be enabled and the port available to this
/// execution context. All clients must serialize access with the same critical
/// section implementation. No DMA or external master may own the port.
// Several drivers issue sequences of PMU commands. Share the wait loop in
// constrained SRAM images.
#[inline(never)]
pub unsafe fn exchange(address: u8, value: Option<u32>) -> Option<u32> {
    critical_section::with(|_| {
        let mut command = (u32::from(address) << 16) | 0x11;
        // SAFETY: These are fixed aligned FRSPIM TX, control and RX registers.
        // The caller owns the port under the critical section.
        unsafe {
            if let Some(data) = value {
                write_volatile((FRSPIM + 4) as *mut u32, data);
                command |= 1 << 8;
            }
            write_volatile(FRSPIM as *mut u32, command);
            (0..512).find(|_| read_volatile(FRSPIM as *const u32) & 3 == 2)?;
            Some(read_volatile((FRSPIM + 8) as *const u32) & 0xff)
        }
    })
}
