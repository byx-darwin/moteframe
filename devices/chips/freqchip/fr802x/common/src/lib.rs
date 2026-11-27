//! Peripheral blocks shared by the FR802x register map.
#![no_std]
#![deny(unsafe_op_in_unsafe_fn, clippy::undocumented_unsafe_blocks)]

#[cfg(target_arch = "arm")]
pub mod gpio;
#[cfg(target_arch = "arm")]
pub mod pmu;
pub mod timer0;
pub mod touchkey;
