//! Optional wellness wire services. Actual device acquisition, policy and storage remain required ports.
#![no_std]
#![forbid(unsafe_code)]
#![cfg_attr(
    not(any(feature = "rri", feature = "uv", feature = "womens-health")),
    doc = "Default build has no wellness service:
```compile_fail
use device_wellness_service::WellnessHandler;
```"
)]
#[cfg(any(feature = "rri", feature = "uv", feature = "womens-health"))]
mod ports;
#[cfg(any(feature = "rri", feature = "uv", feature = "womens-health"))]
pub use ports::*;
#[cfg(any(feature = "rri", feature = "uv", feature = "womens-health"))]
mod handler;
#[cfg(any(feature = "rri", feature = "uv", feature = "womens-health"))]
pub use handler::{WellnessHandler, WellnessOwner};
