//! FR802x GPIO setup for a push-pull output and capacitive input pins.

use core::ptr::{read_volatile, write_volatile};

const GPIO_A: usize = 0x5001_0000;
const GPIO_C: usize = 0x5021_0000;
const PORT_A: usize = 0xe005_2000;
const PORT_C: usize = PORT_A + 2 * 0x30;
const APB0_GATES: usize = 0xe005_3024;
const APB2_GATES: usize = 0xe005_303c;

fn write(address: usize, value: u32) {
    // SAFETY: Each caller supplies an aligned FR802x GPIO or system register.
    unsafe { write_volatile(address as *mut u32, value) };
}

fn update(address: usize, clear: u32, set: u32) {
    // SAFETY: Each caller supplies an aligned, readable system register.
    let value = unsafe { read_volatile(address as *const u32) };
    write(address, (value & !clear) | set);
}

/// Configure a port A GPIO as a push-pull output with its initial latch set.
///
/// # Safety
///
/// The caller must own GPIOA pin 0 through 15, its pad and relevant clock bits.
/// Driving the chosen pin to the supplied voltage must be safe for the board.
pub unsafe fn configure_output(pin: u8, high: bool) {
    let bit = 1 << pin;
    let mux = PORT_A + 0x1c + usize::from(pin / 8) * 4;
    update(APB0_GATES, 0, 1 << 1);
    update(GPIO_A, 0, bit);
    // SAFETY: The calling board has reserved this pin.
    unsafe { set_output(pin, high) };
    update(PORT_A + 8, bit, 0);
    update(PORT_A + 0x28, bit, 0);
    update(PORT_A + 0x2c, bit, 0);
    update(mux, 15 << ((pin % 8) * 4), 0);
    update(GPIO_A, bit, 0);
}

/// Drive the latch of an initialized port A output.
///
/// # Safety
///
/// The caller must own this configured output and supply a pin in 0..16.
#[inline]
pub unsafe fn set_output(pin: u8, high: bool) {
    write(GPIO_A + if high { 0x10 } else { 0x14 }, 1 << pin);
}

pub(crate) fn isolate_capacitive_inputs(mask: u32) {
    update(APB2_GATES, 0, 1 << 2);
    update(GPIO_C, 0, mask);
    update(PORT_C + 8, mask, 0);
    update(PORT_C + 0x28, mask, 0);
}
