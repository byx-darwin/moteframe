//! Shared deadline and wake state for the two FR8029D executors.

use crate::{advertising::CLOCK_MASK, connection::PacketTime};
use core::sync::atomic::{AtomicBool, Ordering};

/// Work timed against the 28-bit baseband clock.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RadioDeadline {
    /// Submit the next advertising event at this clock value.
    Advertising {
        /// Next advertising tick.
        at: u32,
    },
    /// Revisit a connection before the event starts.
    Connection {
        /// Event start tick.
        start: u32,
        /// Lead time in coarse ticks needed to submit the slot.
        lead: u32,
    },
    /// Check for a missing completion interrupt after the maximum slot time.
    Completion {
        /// Submitted slot's start tick.
        start: u32,
        /// Maximum slot duration in coarse ticks.
        timeout: u32,
    },
}

impl RadioDeadline {
    fn remaining_ticks(self, now: u32) -> u32 {
        let at = match self {
            Self::Advertising { at } => at,
            Self::Connection { start, lead } => start.wrapping_sub(lead),
            Self::Completion { start, timeout } => start.wrapping_add(timeout),
        };
        let ahead = at.wrapping_sub(now) & CLOCK_MASK;
        if ahead > CLOCK_MASK / 2 { 0 } else { ahead }
    }
}

/// Relative microseconds to the earliest currently scheduled work.
///
/// `Some(0)` means work is already due. The native time wraps; absolute Host
/// time is monotonic. Conversion floors fractional microseconds.
#[must_use]
pub fn next_delay_us(
    now: PacketTime,
    now_us: u64,
    radio: Option<RadioDeadline>,
    host_at_us: Option<u64>,
) -> Option<u32> {
    let radio = radio.map(|deadline| {
        (u64::from(deadline.remaining_ticks(now.coarse)) * 625)
            .saturating_sub(u64::from(now.elapsed_half_us()))
            / 2
    });
    let host = host_at_us.map(|at| at.saturating_sub(now_us));
    radio
        .into_iter()
        .chain(host)
        .min()
        .map(|us| u32::try_from(us).unwrap_or(u32::MAX))
}

/// Coalesces wakeups between foreground passes. The source retains the work.
#[derive(Debug, Default)]
pub struct WakeSignal(AtomicBool);

impl WakeSignal {
    /// Creates an idle signal.
    #[must_use]
    pub const fn new() -> Self {
        Self(AtomicBool::new(false))
    }

    /// Records a pending wake.
    pub fn signal(&self) {
        self.0.store(true, Ordering::Release);
    }

    /// Takes all wakes recorded before this foreground pass.
    pub fn take(&self) -> bool {
        self.0.swap(false, Ordering::AcqRel)
    }

    /// Checks for a wake during the current foreground pass.
    #[must_use]
    pub fn is_pending(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }

    /// Checks for work before and after arming the completion interrupt.
    /// The callback enables the interrupt and reports any already-latched status.
    /// Keep interrupts masked into WFI, or wait on a latched task notification.
    #[must_use]
    pub fn should_wait(
        &self,
        delay: Option<u32>,
        arm_and_check_status: impl FnOnce() -> bool,
    ) -> bool {
        delay != Some(0) && !self.is_pending() && !arm_and_check_status() && !self.is_pending()
    }
}
