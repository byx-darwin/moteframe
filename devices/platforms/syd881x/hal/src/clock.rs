//! Polling clock arithmetic, independent of register access.
use core::num::NonZeroU32;
/// A sample exceeds the 24-bit downcounter range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidSample;
/// Extends a 24-bit downcounter to monotonic microseconds without losing fractions.
///
/// The caller must sample at least once per hardware wrap and keep the supplied
/// nominal CPU frequency constant. This is a foreground clock, unsuitable for
/// sleep or unbounded blocking operations. It does not measure oscillator error.
#[derive(Debug)]
pub struct CycleClock {
    hz: NonZeroU32,
    previous: u32,
    micros: u64,
    remainder: u64,
}
impl CycleClock {
    /// Captures a raw hardware register, ignoring bits outside the 24-bit counter.
    #[must_use]
    pub const fn from_register(hz: NonZeroU32, value: u32) -> Self {
        Self::from_counter(hz, value & 0x00ff_ffff)
    }
    /// Samples the low 24 bits of a hardware counter register.
    pub fn sample_register(&mut self, value: u32) -> u64 {
        self.sample_counter(value & 0x00ff_ffff)
    }
    /// Captures the initial sample without counting time before initialization.
    /// # Errors
    /// Rejects an initial sample outside the 24-bit hardware range.
    pub fn new(hz: NonZeroU32, initial: u32) -> Result<Self, InvalidSample> {
        if initial > 0x00ff_ffff {
            return Err(InvalidSample);
        }
        Ok(Self::from_counter(hz, initial))
    }
    /// Accumulates elapsed nominal microseconds, handling one counter wrap.
    /// # Errors
    /// Rejects out-of-range samples without changing accumulated state.
    pub fn sample(&mut self, current: u32) -> Result<u64, InvalidSample> {
        if current > 0x00ff_ffff {
            return Err(InvalidSample);
        }
        Ok(self.sample_counter(current))
    }
    pub(crate) const fn from_counter(hz: NonZeroU32, initial: u32) -> Self {
        Self {
            hz,
            previous: initial,
            micros: 0,
            remainder: 0,
        }
    }
    pub(crate) fn sample_counter(&mut self, current: u32) -> u64 {
        let delta = self.previous.wrapping_sub(current) & 0x00ff_ffff;
        let scaled = u64::from(delta) * 1_000_000 + self.remainder;
        self.micros = self
            .micros
            .saturating_add(scaled / u64::from(self.hz.get()));
        self.remainder = scaled % u64::from(self.hz.get());
        self.previous = current;
        self.micros
    }
}

use core::task::Waker;
/// All static alarm slots are occupied by distinct task wakers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AlarmsFull;
/// Bounded foreground alarms. Expired wakers are returned, never invoked while borrowed.
#[derive(Debug)]
pub struct PollAlarms<const N: usize> {
    slots: [Option<(u64, Waker)>; N],
}
impl<const N: usize> Default for PollAlarms<N> {
    fn default() -> Self {
        Self::new()
    }
}
impl<const N: usize> PollAlarms<N> {
    /// Creates empty static-capacity storage.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slots: [const { None }; N],
        }
    }
    /// Keeps the earliest deadline for each task waker.
    /// # Errors
    /// Returns `AlarmsFull` without replacing any older registration.
    pub fn schedule(&mut self, at: u64, waker: &Waker) -> Result<(), AlarmsFull> {
        for (deadline, w) in self.slots.iter_mut().flatten() {
            if w.will_wake(waker) {
                *deadline = (*deadline).min(at);
                return Ok(());
            }
        }
        if let Some(slot) = self.slots.iter_mut().find(|s| s.is_none()) {
            *slot = Some((at, waker.clone()));
            Ok(())
        } else {
            Err(AlarmsFull)
        }
    }
    /// Removes due registrations; callers wake them after releasing their locks.
    #[must_use]
    pub fn expire(&mut self, now: u64) -> [Option<Waker>; N] {
        let mut due = [const { None }; N];
        for (slot, wake) in self.slots.iter_mut().zip(due.iter_mut()) {
            if slot.as_ref().is_some_and(|(deadline, _)| *deadline <= now) {
                *wake = slot.take().map(|(_, w)| w);
            }
        }
        due
    }
}
