//! Interrupt-backed 24-bit `SysTick` arithmetic, independent of ARM registers.
//!
//! One pending bit represents at most one unserviced wrap. The integration must
//! service each wrap before the next one, including interrupt latency, handler
//! time and every masked interval. This cannot detect or recover multiple missed
//! wraps. The CPU clock must run continuously at its constant nominal frequency;
//! sleep, frequency changes, calibrated UTC and restart identity are out of scope.
#![forbid(unsafe_code)]
use core::num::NonZeroU32;

const PERIOD: u64 = 1 << 24;
const MASK: u32 = 0x00ff_ffff;

/// Non-acknowledging reads from the exclusively owned core timer.
///
/// Callers serialize the entire snapshot with the wrap ISR, for example using
/// PRIMASK. Reads must be ordered and reflect the hardware `SysTick` pending bit.
/// Reading CSR/COUNTFLAG is deliberately absent: snapshots never consume wraps.
pub trait SnapshotRegisters {
    /// Reads ICSR.PENDSTSET without clearing the pending exception.
    fn pending(&mut self) -> bool;
    /// Reads the 24-bit current downcounter without side effects.
    fn current(&mut self) -> u32;
}

/// Extends actual serviced and pending wraps into nominal microseconds.
///
/// Start with a cleared current register, then enable a full 24-bit CPU-clocked
/// timer. Exception entry acknowledges the pending bit; call `on_interrupt`
/// exactly once per actual exception before foreground snapshots can resume.
#[derive(Debug)]
pub struct IrqClock {
    hz: NonZeroU32,
    completed_cycles: u64,
}
impl IrqClock {
    /// Creates a zero-origin clock for a freshly cleared, exclusively owned timer.
    #[must_use]
    pub const fn new(hz: NonZeroU32) -> Self {
        Self {
            hz,
            completed_cycles: 0,
        }
    }
    /// Accounts for one wrap whose pending exception was acknowledged on entry.
    /// Never invoke this as a manual timer poll or software-pend this exception.
    pub fn on_interrupt(&mut self) {
        self.completed_cycles = self.completed_cycles.saturating_add(PERIOD);
    }
    /// Reads monotonic nominal microseconds while the wrap ISR is excluded.
    ///
    /// A wrap between the first pending read and VAL must use a post-wrap VAL;
    /// rereading when pending changes avoids combining a new epoch with old VAL.
    /// A wrap after the final pending read may leave this snapshot in the earlier
    /// period, which is valid. Already-pending wraps remain pending for the ISR.
    /// VAL=0 is phase zero (initial state or exact wrap), never a lost full period.
    /// Arithmetic saturates on representational exhaustion rather than wrapping.
    pub fn now(&self, registers: &mut impl SnapshotRegisters) -> u64 {
        let pending_before = registers.pending();
        let mut value = registers.current() & MASK;
        let pending_after = registers.pending();
        if !pending_before && pending_after {
            value = registers.current() & MASK;
        }
        let phase = if value == 0 {
            0
        } else {
            PERIOD - u64::from(value)
        };
        let cycles = self
            .completed_cycles
            .saturating_add(if pending_after { PERIOD } else { 0 })
            .saturating_add(phase);
        let hz = u64::from(self.hz.get());
        (cycles / hz)
            .saturating_mul(1_000_000)
            .saturating_add((cycles % hz) * 1_000_000 / hz)
    }
}
