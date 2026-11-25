//! Legacy board paths to the chip MMIO and runtime implementations.
#![forbid(unsafe_code)]
pub use device_display::profiles::legacy_128::reset as reset_display;
#[cfg(feature = "native-radio")]
pub use syd881x_hal::hardware::RadioMmio;
pub use syd881x_hal::hardware::{AlreadyTaken, BusyDelay, Mmio};
pub use syd881x_runtime::{GPIO_IRQHandler, take_gpio_events};
#[cfg(feature = "trouble-runtime")]
pub use syd881x_runtime::{SysTick_Handler, TimeError, poll_time, start_irq_time, start_time};
