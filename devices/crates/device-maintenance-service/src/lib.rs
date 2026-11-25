//! Feature-selected maintenance wire handlers and independent commercial device jobs.
#![no_std]
#![forbid(unsafe_code)]
mod common;
#[cfg(any(feature = "raw-data", feature = "self-test"))]
mod observed_ports;
pub use common::{Clock, Error, NamedRequest};
#[cfg(any(feature = "raw-data", feature = "self-test"))]
pub use observed_ports::*;
#[cfg(feature = "device-jobs")]
mod device_jobs;
#[cfg(feature = "device-jobs")]
pub use device_jobs::*;
#[cfg(feature = "raw-data")]
mod raw;
#[cfg(feature = "raw-data")]
pub use raw::*;
#[cfg(feature = "self-test")]
mod self_test;
#[cfg(feature = "self-test")]
pub use self_test::*;
#[cfg(any(feature = "engineering", feature = "factory-config"))]
mod persistent;
#[cfg(any(feature = "engineering", feature = "factory-config"))]
pub use persistent::*;

/// Compile selection only, never a declaration that a real backend or permission is available.
#[must_use]
pub const fn module_compiled(module: u8) -> bool {
    (module == 2 && cfg!(feature = "device-jobs"))
        || (module == 0x70 && cfg!(feature = "raw-data"))
        || (module == 0x71 && cfg!(feature = "self-test"))
        || (module == 0x72 && cfg!(feature = "engineering"))
        || (module == 0x73 && cfg!(feature = "factory-config"))
}

#[cfg(any(feature = "engineering", feature = "factory-config"))]
mod outputs;
#[cfg(feature = "engineering")]
pub use outputs::{AdcSamples, GestureIter, GestureSample, GestureSamples};
#[cfg(any(feature = "engineering", feature = "factory-config"))]
pub use outputs::{NamedOutput, NamedOutputValue};
