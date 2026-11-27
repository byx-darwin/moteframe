//! FR802x APB TouchKey controller with a continuous two-electrode scan.

/// Supported TouchKey input mappings.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
pub enum Electrode {
    /// PC2, hardware mapping index 4.
    Pc2 = 4,
    /// PC3, hardware mapping index 5.
    Pc3 = 5,
}

/// Scan start interval with the 1 MHz TouchKey clock.
pub const SCAN_INTERVAL_US: u32 = 5_000;

/// Result of polling the TouchKey controller.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Scan {
    /// No new sample or fault.
    Pending,
    /// A complete scan, in channel order.
    Complete([u16; 2]),
    /// A scan was aborted by the controller.
    Fault,
}

#[cfg(target_arch = "arm")]
mod hardware {
    use super::{Electrode, SCAN_INTERVAL_US, Scan};
    use crate::{gpio, pmu};
    use core::ptr::{read_volatile, write_volatile};

    const TOUCH: usize = 0x5009_0000;
    const EPB_GATES: usize = 0xe005_3014;
    const APB_SELECT: usize = 0xe005_3018;
    const APB_DIVIDER: usize = 0xe005_3020;
    const APB_GATES: usize = 0xe005_3024;
    const SCAN_DONE: u32 = 4;
    const SCAN_ERROR: u32 = 3;
    const LOOP_MODE: u32 = 2;

    fn read(address: usize) -> u32 {
        // SAFETY: The caller supplies an aligned FR802x system or TouchKey register.
        unsafe { read_volatile(address as *const u32) }
    }

    fn write(address: usize, value: u32) {
        // SAFETY: The caller supplies an aligned writable FR802x register.
        unsafe { write_volatile(address as *mut u32, value) };
    }

    fn update(address: usize, clear: u32, set: u32) {
        write(address, (read(address) & !clear) | set);
    }

    fn power_on() -> Option<()> {
        // SAFETY: Initialization enabled the PMU clock and owns analog settings.
        // The shared transaction serializes the PMU port.
        unsafe {
            let analog0 = pmu::exchange(0xee, None)?;
            let analog1 = pmu::exchange(0xef, None)?;
            let controller = pmu::exchange(0xa0, None)?;
            pmu::exchange(0xee, Some((analog0 & !(3 << 4)) | (1 << 2)))?;
            pmu::exchange(0xef, Some(analog1 | 0x19))?;
            pmu::exchange(0xea, Some(0x6f))?;
            pmu::exchange(0xeb, Some(0x68))?;
            pmu::exchange(0xa0, Some(controller | 2))?;
        }
        Some(())
    }

    /// Power up and start scanning the two electrodes in order.
    ///
    /// # Safety
    ///
    /// Use on the matching FR802x register map with a 24 MHz reference.
    /// The caller owns the TouchKey controller, both PC pads, their clock
    /// gates and the PMU analog setup during initialization.
    pub unsafe fn initialize(electrodes: [Electrode; 2]) -> bool {
        let [first, second] = electrodes;
        let pins = (1 << (first as u8 - 2)) | (1 << (second as u8 - 2));
        gpio::isolate_capacitive_inputs(pins);
        update(EPB_GATES, 0, 1 << 3);
        update(APB_SELECT, 3 << 14, 0);
        update(APB_DIVIDER, 0xfff << 12, 23 << 12);
        update(APB_GATES, 0, 1 << 9);
        if power_on().is_none() {
            return false;
        }

        write(TOUCH, 0);
        write(TOUCH + 0x04, LOOP_MODE);
        write(TOUCH + 0x08, SCAN_INTERVAL_US);
        write(TOUCH + 0x10, 1); // two keys, encoded as count minus one
        write(TOUCH + 0x14, 0); // one measurement per channel
        write(TOUCH + 0x1c, (first as u32) | ((second as u32) << 4));
        write(TOUCH + 0x24, 2);
        // Deliver out-of-range values to the per-key filter without aborting
        // the whole scan before the other key can be measured.
        write(TOUCH + 0x30, 0xffff);
        write(TOUCH + 0x34, 0); // foreground polls the raw status
        write(TOUCH + 0x38, 7);
        write(TOUCH, 1);
        write(TOUCH + 0x04, LOOP_MODE | 1);
        true
    }

    /// Read a completed scan or acknowledge a fault and restart scanning.
    ///
    /// # Safety
    ///
    /// Call only from the same foreground that initialized the controller.
    #[inline]
    pub unsafe fn sample() -> Scan {
        let status = read(TOUCH + 0x3c) & (SCAN_DONE | SCAN_ERROR);
        if status & SCAN_ERROR != 0 {
            write(TOUCH + 0x04, LOOP_MODE);
            write(TOUCH + 0x38, status);
            write(TOUCH + 0x04, LOOP_MODE | 1);
            Scan::Fault
        } else if status != 0 {
            let samples = [read(TOUCH + 0x48) as u16, read(TOUCH + 0x4c) as u16];
            write(TOUCH + 0x38, status);
            Scan::Complete(samples)
        } else {
            Scan::Pending
        }
    }
}

#[cfg(target_arch = "arm")]
pub use hardware::{initialize, sample};
