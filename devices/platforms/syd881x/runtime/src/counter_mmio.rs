//! ARM core-counter boundary shared by the runtime clock and exclusive lab sampler.
#![allow(unsafe_code, reason = "Exclusive core-timer MMIO boundary")]

use crate::counter::{self, CounterRegisters, IrqCounterRegisters, TimerBusy};
use core::{
    marker::PhantomData,
    ptr::{read_volatile, write_volatile},
};

pub(crate) struct Access;
impl CounterRegisters for Access {
    fn control(&mut self) -> u32 {
        control()
    }
    fn set_control(&mut self, value: u32) {
        // SAFETY: Only exclusive runtime/lab owners construct and use this port.
        unsafe {
            write_volatile(0xe000_e010 as *mut u32, value);
        }
    }
    fn set_reload(&mut self, value: u32) {
        // SAFETY: Same exclusive owner; the shared sequencer supplies 24-bit reload.
        unsafe {
            write_volatile(0xe000_e014 as *mut u32, value);
        }
    }
    fn clear_current(&mut self) {
        // SAFETY: Same exclusive owner; zero clears current value and COUNTFLAG.
        unsafe {
            write_volatile(0xe000_e018 as *mut u32, 0);
        }
    }
}

impl IrqCounterRegisters for Access {
    fn clear_pending(&mut self) {
        // SAFETY: Exclusive stopped timer setup under PRIMASK. PENDSTCLR is a
        // write-one bit; a direct write avoids modifying unrelated ICSR fields.
        unsafe {
            write_volatile(0xe000_ed04 as *mut u32, 1 << 25);
        }
    }
}
impl crate::irq_clock::SnapshotRegisters for Access {
    fn pending(&mut self) -> bool {
        // SAFETY: Read-only ICSR access under the runtime's PRIMASK section.
        unsafe { read_volatile(0xe000_ed04 as *const u32) & (1 << 26) != 0 }
    }
    fn current(&mut self) -> u32 {
        current()
    }
}

pub(crate) fn try_start_irq_owned() -> Result<(), TimerBusy> {
    counter::try_start_irq(&mut Access)
}

pub(crate) fn current() -> u32 {
    // SAFETY: Runtime callers serialize an initialized core timer; lab callers
    // retain the exclusive token created under their unsafe acquisition contract.
    unsafe { read_volatile(0xe000_e018 as *const u32) }
}
fn control() -> u32 {
    // SAFETY: Same core mapping and exclusive owner; reading acknowledges COUNTFLAG.
    unsafe { read_volatile(0xe000_e010 as *const u32) }
}
pub(crate) fn try_start_owned() -> Result<(), TimerBusy> {
    counter::try_start(&mut Access)
}

/// Exclusive lab sampler; dropping it does not stop or release the running timer.
/// It must not overlap the runtime clock or another timer owner.
#[derive(Debug)]
pub struct LabCounter {
    _exclusive: PhantomData<*mut ()>,
}
impl LabCounter {
    /// Acquires a disabled core counter without enabling its interrupt.
    /// # Safety
    /// Caller owns initialized ARM core state and excludes other foreground/IRQ
    /// timer mutators for the entire token lifetime. No other clock may be active.
    /// # Errors
    /// Rejects an enabled counter or timer interrupt before any write.
    pub unsafe fn try_start() -> Result<Self, TimerBusy> {
        try_start_owned()?;
        Ok(Self {
            _exclusive: PhantomData,
        })
    }
    /// Stops and reloads the counter for the existing factory comparison experiment.
    /// # Safety
    /// Caller already owns and has saved all core timer state, excludes all other
    /// timer users, and accepts replacing that state for this token's lifetime.
    pub unsafe fn restart_exclusive() -> Self {
        counter::restart(&mut Access);
        Self {
            _exclusive: PhantomData,
        }
    }
    /// Samples the current downcounter without acknowledging its wrap flag.
    #[must_use]
    pub fn sample(&self) -> u32 {
        current()
    }
    /// Reads control and acknowledges the wrap flag, as required by the lab sampler.
    #[must_use]
    pub fn control(&self) -> u32 {
        control()
    }
    /// Clears current value and wrap flag before a bounded measurement.
    pub fn clear_current(&mut self) {
        Access.clear_current();
    }
    /// Stops this counter without changing its reload value.
    pub fn stop(&mut self) {
        Access.set_control(0);
    }
}
