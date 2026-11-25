//! Chip GPIO vector entry and foreground event ownership.
#![allow(
    unsafe_code,
    reason = "The IRQ boundary owns a PRIMASK-protected static mailbox"
)]
use core::ptr::{addr_of_mut, read_volatile, write_volatile};
use syd881x_hal::hardware::{acknowledge_gpio_interrupt, critical};
static mut GPIO_EVENTS: u32 = 0;
/// Polls GPIO sources into a runtime-owned bitmap, for foreground dispatch.
/// # Safety
/// Must be entered as the actual GPIO IRQ on an initialized chip with configured
/// ROM-to-application routing. No callback is invoked in interrupt context.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn GPIO_IRQHandler() {
    critical(|| {
        // SAFETY: Real initialized IRQ entry with all GPIO RMW excluded by PRIMASK.
        let active = unsafe { acknowledge_gpio_interrupt() };
        if active != 0 {
            let pointer = addr_of_mut!(GPIO_EVENTS);
            // SAFETY: Mailbox is accessed only while the same chip critical section is held.
            unsafe {
                write_volatile(pointer, read_volatile(pointer) | active);
            }
        }
    });
}
/// Takes accumulated GPIO events atomically; the caller dispatches outside the ISR.
#[must_use]
pub fn take_gpio_events() -> u32 {
    critical(|| {
        let pointer = addr_of_mut!(GPIO_EVENTS);
        // SAFETY: Serialized with the real IRQ, with no references escaping the closure.
        unsafe {
            let events = read_volatile(pointer);
            write_volatile(pointer, 0);
            events
        }
    })
}
