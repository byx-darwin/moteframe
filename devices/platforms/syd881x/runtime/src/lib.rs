//! `SYD881x` startup memory, IRQ dispatch and optional cooperative timer integration.
#![no_std]
mod memory;
pub use memory::initialize_ram;
pub mod counter;
#[cfg(target_arch = "arm")]
mod counter_mmio;
/// Pure interrupt clock arithmetic and ordered snapshot boundary.
pub mod irq_clock;
#[cfg(target_arch = "arm")]
pub use counter_mmio::LabCounter;
#[cfg(all(feature = "hardware", target_arch = "arm"))]
mod gpio;
#[cfg(all(feature = "hardware", target_arch = "arm"))]
pub use gpio::{GPIO_IRQHandler, take_gpio_events};
#[cfg(all(feature = "trouble-runtime", target_arch = "arm"))]
mod time;
#[cfg(all(feature = "trouble-runtime", target_arch = "arm"))]
pub use time::{SysTick_Handler, TimeError, poll_time, start_irq_time, start_time};

pub mod reset;
#[cfg(target_arch = "arm")]
mod reset_mmio;
#[cfg(target_arch = "arm")]
pub use reset_mmio::{
    disable_watchdog_for_vendor_lab, prepare_candidate_entry, prepare_native_lab,
    request_native_reset_for_lab,
};
