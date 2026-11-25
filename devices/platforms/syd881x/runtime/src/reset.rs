//! Exact lab reset sequencing; callers select the appropriate existing boot mode.
#![forbid(unsafe_code)]
/// Primitive reset operations supplied by the chip boundary or a host fixture.
pub trait ResetRegisters {
    /// Disable all configurable interrupt sources.
    fn disable_interrupts(&mut self);
    /// Clear all pending configurable interrupts.
    fn clear_pending(&mut self);
    /// Stop the core timer without reloading its value.
    fn stop_systick(&mut self);
    /// Read the byte-wide watchdog control register.
    fn watchdog(&mut self) -> u8;
    /// Write the byte-wide watchdog control register.
    fn set_watchdog(&mut self, value: u8);
}
/// Preserves the existing native lab's interrupt/timer/watchdog preparation order.
pub fn prepare_native_lab(registers: &mut impl ResetRegisters) {
    registers.disable_interrupts();
    registers.clear_pending();
    registers.stop_systick();
    disable_watchdog(registers);
}
/// Additional core state owned by the candidate's privileged Thread-mode entry.
pub trait CandidateResetRegisters: ResetRegisters {
    /// Clear the writable `PendSV` and `SysTick` pending bits, without touching NMI.
    fn clear_system_pending(&mut self, value: u32);
    /// Disable inherited sleep-on-exit, deep sleep and event-on-pending policy.
    fn clear_sleep_configuration(&mut self);
    /// Reset configurable system-handler priorities to their reset value.
    fn clear_system_priorities(&mut self);
}
/// Normalize inherited exception state before application RAM or clock startup.
/// The assembly entry must first mask interrupts and select privileged Thread/MSP.
pub fn prepare_candidate_entry(registers: &mut impl CandidateResetRegisters) {
    registers.disable_interrupts();
    registers.clear_pending();
    registers.stop_systick();
    registers.clear_system_pending((1 << 27) | (1 << 25));
    registers.clear_sleep_configuration();
    registers.clear_system_priorities();
    disable_watchdog(registers);
}
/// Preserves the distinct vendor lab's watchdog-only preparation.
pub fn disable_watchdog(registers: &mut impl ResetRegisters) {
    let value = registers.watchdog();
    registers.set_watchdog(value & !3);
}

/// Native reset operations, distinct from startup and watchdog preparation.
pub trait PowerResetRegisters {
    /// Mask configurable CPU interrupts before touching reset control.
    fn mask_interrupts(&mut self);
    /// Read the byte-wide native power/reset control register.
    fn power_control(&mut self) -> u8;
    /// Write native power/reset control while preserving unrelated bits.
    fn set_power_control(&mut self, value: u8);
}

/// Requests the existing lab native reset, preserving the other seven PWR bits.
pub fn request_native_reset(registers: &mut impl PowerResetRegisters) {
    registers.mask_interrupts();
    let previous = registers.power_control();
    registers.set_power_control(previous & 0x7f);
}
