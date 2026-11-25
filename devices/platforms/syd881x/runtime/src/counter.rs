//! Shared polling counter setup, independent of foreground profiling policy.
#![forbid(unsafe_code)]

/// Register operations supplied by the exclusive core-timer boundary.
pub trait CounterRegisters {
    /// Read control; on hardware this also clears the wrap flag.
    fn control(&mut self) -> u32;
    /// Write control without changing reload or current value.
    fn set_control(&mut self, value: u32);
    /// Set the 24-bit reload value.
    fn set_reload(&mut self, value: u32);
    /// Clear the current value and wrap flag.
    fn clear_current(&mut self);
}

/// Another subsystem enabled the counter or its interrupt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimerBusy;

fn configure(registers: &mut impl CounterRegisters) {
    registers.set_reload(0x00ff_ffff);
    registers.clear_current();
    registers.set_control(5);
}

/// Starts the stopped core counter at CPU clock rate, leaving its IRQ disabled.
/// # Errors
/// Rejects an enabled counter or interrupt before writing any register.
pub fn try_start(registers: &mut impl CounterRegisters) -> Result<(), TimerBusy> {
    if registers.control() & 3 != 0 {
        return Err(TimerBusy);
    }
    configure(registers);
    Ok(())
}

/// Stops and reloads an exclusively owned lab counter without an acquisition read.
/// The caller selects this force-restart policy; normal acquisition uses `try_start`.
pub fn restart(registers: &mut impl CounterRegisters) {
    registers.set_control(0);
    configure(registers);
}

/// Additional register operation for exclusive interrupt-backed timer setup.
pub trait IrqCounterRegisters: CounterRegisters {
    /// Clears ICSR.PENDSTSET before enabling a freshly initialized timer.
    fn clear_pending(&mut self);
}

/// Starts an exclusive full-period CPU-clocked counter with its IRQ enabled.
///
/// The caller must exclude its ISR until the clock state is initialized, install
/// valid forwarded vectors first, and service every wrap before another arrives.
/// # Errors
/// Rejects an enabled counter or interrupt without writes, including pending clear.
pub fn try_start_irq(registers: &mut impl IrqCounterRegisters) -> Result<(), TimerBusy> {
    if registers.control() & 3 != 0 {
        return Err(TimerBusy);
    }
    registers.set_reload(0x00ff_ffff);
    registers.clear_current();
    registers.clear_pending();
    registers.set_control(7);
    Ok(())
}
