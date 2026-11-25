//! Single-core critical sections and exclusive polling/IRQ monotonic time.
#![allow(
    unsafe_code,
    reason = "Single-core PRIMASK and SysTick runtime boundary"
)]
use core::{arch::asm, cell::RefCell, num::NonZeroU32, task::Waker};
use critical_section::Mutex;
use embassy_time_driver::Driver;
use syd881x_hal::clock::{CycleClock, PollAlarms};
use syd881x_hal::hardware::Mmio;
struct SingleCore;
critical_section::set_impl!(SingleCore);
// SAFETY: This board has one Cortex-M0 core. PRIMASK excludes every configurable
// interrupt and its prior state is restored, including nested critical sections.
unsafe impl critical_section::Impl for SingleCore {
    unsafe fn acquire() -> critical_section::RawRestoreState {
        let previous: u32;
        // SAFETY: ARMv6-M PRIMASK access with memory-clobbering inline assembly.
        unsafe {
            asm!("mrs {old}, PRIMASK","cpsid i",old=out(reg) previous,options(nostack));
        }
        previous & 1 != 0
    }
    unsafe fn release(previous: critical_section::RawRestoreState) {
        if !previous {
            // SAFETY: Only re-enable interrupts if the matching acquire found them enabled.
            unsafe {
                asm!("cpsie i", options(nostack));
            }
        }
    }
}
/// The core timer cannot be exclusively initialized.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeError {
    /// This driver already owns the timer; its frequency cannot be changed.
    AlreadyStarted,
    /// Another subsystem enabled the core timer or its interrupt.
    TimerBusy,
}
enum Clock {
    Polling(CycleClock),
    Irq(crate::irq_clock::IrqClock),
}
struct State {
    clock: Option<Clock>,
    alarms: PollAlarms<8>,
}
impl State {
    fn now(&mut self) -> u64 {
        match self.clock.as_mut() {
            None => 0,
            Some(Clock::Polling(clock)) => {
                // Polling owns the counter and never acknowledges COUNTFLAG.
                clock.sample_register(crate::counter_mmio::current())
            }
            Some(Clock::Irq(clock)) => clock.now(&mut crate::counter_mmio::Access),
        }
    }
}
struct ForegroundTime {
    state: Mutex<RefCell<State>>,
}
embassy_time_driver::time_driver_impl!(static TIME:ForegroundTime=ForegroundTime{state:Mutex::new(RefCell::new(State{clock:None,alarms:PollAlarms::new()}))});
impl Driver for ForegroundTime {
    fn now(&self) -> u64 {
        critical_section::with(|cs| self.state.borrow(cs).borrow_mut().now())
    }
    fn schedule_wake(&self, at: u64, waker: &Waker) {
        let wake = critical_section::with(|cs| {
            let mut state = self.state.borrow(cs).borrow_mut();
            at <= state.now() || state.alarms.schedule(at, waker).is_err()
        });
        // Full storage requests an immediate repoll instead of losing the deadline.
        if wake {
            waker.wake_by_ref();
        }
    }
}
/// Starts `SysTick` at the caller's constant nominal CPU frequency, without an IRQ.
///
/// Call after radio Reset has selected the MCU clock. Poll time at least once
/// every 2^24 CPU cycles (262 ms at 64 MHz). No sleep/clock switching is supported.
/// Nominal frequency is configuration, not a measurement of oscillator accuracy.
/// # Errors
/// Rejects a previously started or already-enabled timer before changing it.
#[allow(
    clippy::needless_pass_by_ref_mut,
    reason = "Mutable board token proves exclusive setup"
)]
pub fn start_time(_owner: &mut Mmio, hz: NonZeroU32) -> Result<(), TimeError> {
    critical_section::with(|cs| {
        let mut state = TIME.state.borrow(cs).borrow_mut();
        if state.clock.is_some() {
            return Err(TimeError::AlreadyStarted);
        }
        // Exclusive board token and critical section serialize shared counter setup.
        crate::counter_mmio::try_start_owned().map_err(|_| TimeError::TimerBusy)?;
        let initial = crate::counter_mmio::current() & 0x00ff_ffff;
        state.clock = Some(Clock::Polling(CycleClock::from_register(hz, initial)));
        Ok(())
    })
}
/// Starts an exclusive interrupt-backed `SysTick` clock at nominal CPU frequency.
///
/// Call after radio Reset selects the clock. This is mutually exclusive with
/// `start_time` and lab counter ownership. Initialization preserves PRIMASK and
/// never enables global interrupts. State and stale pending clear are complete
/// before the enclosing critical section can restore an enabled PRIMASK.
///
/// Every wrap must be serviced before the next: all interrupt latency, handler
/// execution and masked intervals must fit within 2^24 CPU cycles (262.144 ms at
/// nominal 64 MHz). Multiple masked wraps cannot be detected or reconstructed.
/// Do not sleep, stop the timer, change frequency or software-pend `SysTick`.
/// Continue calling `poll_time` every executor pass; the ISR never wakes tasks.
/// The frequency is nominal configuration, not oscillator calibration, UTC or a
/// durable boot/continuity epoch.
///
/// # Safety
/// The real ROM-forwarded Flash `SysTick` vector must point to `SysTick_Handler`.
/// All exception vectors must be valid before interrupts are unmasked; disable
/// and clear unsupported NVIC routes (especially ROM LL/Timer3/LLC2). The exact
/// verified ROM forwards vector 15 via application address 0x1000003c, without
/// VTOR/remapping. The caller must uphold the timing/ownership contract above
/// for the clock's remaining lifetime. No other code may read CSR/COUNTFLAG or
/// modify SysTick/its pending bit after successful acquisition.
/// # Errors
/// Rejects either already-started time mode or an enabled hardware timer before
/// changing timer configuration or pending state.
#[allow(
    clippy::needless_pass_by_ref_mut,
    reason = "Mutable board token proves exclusive setup"
)]
pub unsafe fn start_irq_time(_owner: &mut Mmio, hz: NonZeroU32) -> Result<(), TimeError> {
    critical_section::with(|cs| {
        let mut state = TIME.state.borrow(cs).borrow_mut();
        if state.clock.is_some() {
            return Err(TimeError::AlreadyStarted);
        }
        // Publish the zero-origin state before setting TICKINT. PRIMASK keeps
        // both setup and a possible busy rollback invisible to the ISR.
        state.clock = Some(Clock::Irq(crate::irq_clock::IrqClock::new(hz)));
        if crate::counter_mmio::try_start_irq_owned().is_err() {
            state.clock = None;
            return Err(TimeError::TimerBusy);
        }
        Ok(())
    })
}

/// Accounts for one hardware wrap; foreground owns alarm expiry and wake calls.
///
/// Exception entry clears PENDSTSET. Neither this handler nor `now` reads CSR,
/// so COUNTFLAG is never used as a second, competing source of wrap events.
/// # Safety
/// Must be entered only as the actual `SysTick` exception after `start_irq_time`
/// with its documented vector, exclusive ownership and single-wrap contract.
// The default handler remains the vector symbol. A laboratory firmware may
// supply a zero-stack observer that tail-forwards to this same clock body.
#[cfg_attr(not(feature = "lab-external-systick"), unsafe(no_mangle))]
#[cfg_attr(
    feature = "lab-external-systick",
    allow(
        non_snake_case,
        reason = "Preserve the existing public handler name beneath the lab vector wrapper."
    )
)]
pub unsafe extern "C" fn SysTick_Handler() {
    critical_section::with(|cs| {
        let mut state = TIME.state.borrow(cs).borrow_mut();
        if let Some(Clock::Irq(clock)) = state.clock.as_mut() {
            clock.on_interrupt();
        }
    });
}

/// Samples either selected time mode and wakes tasks after releasing all locks.
/// Call on every executor pass, even when the Host has no other work.
pub fn poll_time() {
    let due = critical_section::with(|cs| {
        let mut state = TIME.state.borrow(cs).borrow_mut();
        let now = state.now();
        state.alarms.expire(now)
    });
    for wake in due.into_iter().flatten() {
        wake.wake();
    }
}
