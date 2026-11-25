//! Optional health wire handlers backed only by explicitly injected real ports.
#![no_std]
#![forbid(unsafe_code)]
mod handler;
mod ports;
mod start_guard;
mod wire;
pub use handler::{HealthHandler, SharedHealthOwner};
pub use ports::*;
pub use wire::{CuffPressure, EventToken, ModuleRunStatus, ZoneSummary};

mod import;
pub use import::{HealthImport, HealthImportPort, ImportHealthHandler, ImportKey};
