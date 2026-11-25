//! Optional motion wire services; actual algorithms, clocks and persistence remain required ports.
#![no_std]
#![forbid(unsafe_code)]
#![cfg_attr(
    not(any(
        feature = "sport",
        feature = "sleep",
        feature = "activity",
        feature = "wear"
    )),
    doc = "Default build exposes no motion handler:
```compile_fail
use device_motion_service::MotionHandler;
```"
)]
#[cfg(feature = "sport")]
mod legacy;
#[cfg(feature = "sport")]
pub use legacy::LegacySportSummary;
#[cfg(any(
    feature = "sport",
    feature = "sleep",
    feature = "activity",
    feature = "wear"
))]
mod types;
#[cfg(any(
    feature = "sport",
    feature = "sleep",
    feature = "activity",
    feature = "wear"
))]
pub use types::*;
#[cfg(any(
    feature = "sport",
    feature = "sleep",
    feature = "activity",
    feature = "wear"
))]
mod handler;
#[cfg(any(
    feature = "sport",
    feature = "sleep",
    feature = "activity",
    feature = "wear"
))]
pub use handler::{MotionHandler, MotionOwner};

#[cfg(feature = "sport")]
mod sport_control;
#[cfg(feature = "sport")]
pub use sport_control::{SPORT_PROOF_CAPACITY, SportControl};
