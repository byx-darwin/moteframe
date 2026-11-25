//! `SYD881x` typed chip peripherals; product wiring belongs to a board crate.
#![no_std]
pub mod clock;
pub mod drivers;
#[cfg(all(feature = "hardware", target_arch = "arm"))]
pub mod hardware;
pub mod registers;
