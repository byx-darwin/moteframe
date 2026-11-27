//! FR802x TIMER0 one-shot operation using the 24 MHz clock source.

/// External interrupt number in the FR802x vector table.
pub const IRQ_NUMBER: u8 = 16;

/// Encode a one-shot deadline in microseconds as the 16-bit auto-reload value.
#[must_use]
pub const fn reload(delay_us: u32) -> u16 {
    match delay_us {
        0..=1 => 1,
        2..=65_536 => (delay_us - 1) as u16,
        _ => u16::MAX,
    }
}

#[cfg(target_arch = "arm")]
mod hardware {
    use core::ptr::{read_volatile, write_volatile};

    const TIMER: usize = 0x5000_0000;
    const CONTROL: usize = TIMER;
    const SLAVE_MODE: usize = TIMER + 0x08;
    const INTERRUPT_ENABLE: usize = TIMER + 0x0c;
    const STATUS: usize = TIMER + 0x10;
    const GENERATE: usize = TIMER + 0x14;
    const PRESCALER: usize = TIMER + 0x28;
    const RELOAD: usize = TIMER + 0x2c;
    const REPETITION: usize = TIMER + 0x30;
    const ONE_SHOT: u32 = (1 << 3) | (1 << 2);
    const UPDATE: u32 = 1;

    fn read(address: usize) -> u32 {
        // SAFETY: These are aligned FR802x clock control registers.
        unsafe { read_volatile(address as *const u32) }
    }

    fn write(address: usize, value: u32) {
        // SAFETY: Only fixed aligned TIMER0 and APB clock registers are written.
        unsafe { write_volatile(address as *mut u32, value) };
    }

    /// Select the 24 MHz clock and configure a stopped 1 MHz one-shot timer.
    ///
    /// # Safety
    ///
    /// Use only on a matching FR802x register map with TIMER0 exclusively
    /// reserved for this caller. Mask the IRQ during configuration.
    #[inline]
    pub unsafe fn initialize() {
        const SELECT: usize = 0xe005_3018;
        const DIVIDER: usize = 0xe005_301c;
        const GATES: usize = 0xe005_3024;
        write(SELECT, read(SELECT) & !3);
        write(DIVIDER, read(DIVIDER) & !15);
        write(GATES, read(GATES) | 1);
        write(CONTROL, ONE_SHOT);
        write(SLAVE_MODE, 0);
        write(INTERRUPT_ENABLE, 0);
        write(PRESCALER, 23);
        write(REPETITION, 0);
        write(STATUS, !UPDATE);
    }

    /// Arm the stopped timer for a deadline in microseconds.
    ///
    /// # Safety
    ///
    /// Only the owner of an initialized TIMER0 may arm it, with the
    /// update handler excluded until the deadline is installed.
    #[inline]
    pub unsafe fn arm(delay_us: u32) {
        write(RELOAD, u32::from(super::reload(delay_us)));
        write(GENERATE, UPDATE);
        write(STATUS, !UPDATE);
        write(INTERRUPT_ENABLE, UPDATE);
        write(CONTROL, ONE_SHOT | 1);
    }

    /// Stop the timer, mask its update interrupt and clear the update flag.
    ///
    /// # Safety
    ///
    /// Only the timer owner may stop it, with the update handler excluded.
    #[inline]
    pub unsafe fn stop() {
        write(CONTROL, ONE_SHOT);
        write(INTERRUPT_ENABLE, 0);
        write(STATUS, !UPDATE);
    }

    /// Clear the timer's update flag in the IRQ handler.
    ///
    /// # Safety
    ///
    /// Only use from the installed handler for this timer.
    #[inline]
    pub unsafe fn acknowledge_update() {
        write(STATUS, !UPDATE);
    }
}

#[cfg(target_arch = "arm")]
pub use hardware::{acknowledge_update, arm, initialize, stop};

#[cfg(test)]
mod tests {
    use super::reload;

    #[test]
    fn clamps_the_one_shot_deadline_and_encodes_inclusive_reload() {
        for (delay, expected) in [
            (0, 1),
            (1, 1),
            (2, 1),
            (1000, 999),
            (65_536, 65_535),
            (u32::MAX, 65_535),
        ] {
            assert_eq!(reload(delay), expected);
        }
    }
}
