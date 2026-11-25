//! Dedicated anonymous CONTROL and diagnostic readiness composition.
#![no_std]
#![forbid(unsafe_code)]
mod factory;
mod input;
mod readiness;
mod reset;
pub use factory::{CoreFactory, CoreOnly, UnboundStorage};
pub use input::{Event, InputState};
pub use readiness::{InputReport, InputSource, LinkStage, Readiness, ReadinessUi, TouchStatus};
pub use reset::{ObservedReset, ResetError, ResetGate};
