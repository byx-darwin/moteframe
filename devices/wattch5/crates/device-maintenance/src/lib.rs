//! Optional development/factory maintenance engines; no wire router or hardware driver.
//! Defaults compile no engines. Engineering72 and provisioning73 expose durable-job
//! admission/query contracts; actual durable storage, recovery and executors must be supplied.
#![no_std]
#![forbid(unsafe_code)]
#![cfg_attr(
    not(feature = "raw-data"),
    doc = "Default/commercial builds expose no raw engine:
```compile_fail
use device_maintenance::raw::RawEngine;
```"
)]
#![cfg_attr(
    not(feature = "self-test"),
    doc = "Default/commercial builds expose no self-test engine:
```compile_fail
use device_maintenance::self_test::SelfTestEngine;
```"
)]
#![cfg_attr(
    not(any(feature = "engineering", feature = "factory-config")),
    doc = "Default/commercial builds expose no persistent maintenance engine:
```compile_fail
use device_maintenance::persistent::PersistentEngine;
```"
)]
#[cfg(any(
    feature = "raw-data",
    feature = "self-test",
    feature = "engineering",
    feature = "factory-config"
))]
mod common;
#[cfg(any(
    feature = "raw-data",
    feature = "self-test",
    feature = "engineering",
    feature = "factory-config"
))]
pub use common::{Authority, Capability, Error, Profile};
#[cfg(feature = "raw-data")]
pub mod raw;
#[cfg(feature = "self-test")]
pub mod self_test;

#[cfg(any(feature = "engineering", feature = "factory-config"))]
pub mod persistent;
