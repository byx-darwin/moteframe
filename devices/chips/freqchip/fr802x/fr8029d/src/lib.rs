//! FR8029D peripheral drivers.
#![no_std]
#![deny(unsafe_op_in_unsafe_fn, clippy::undocumented_unsafe_blocks)]

#[cfg(feature = "qspi")]
pub mod qspi;
pub mod radio;
pub mod saradc;

#[cfg(target_arch = "arm")]
pub use fr802x_common::gpio;
pub use fr802x_common::{timer0, touchkey};
