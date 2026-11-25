//! Hardware-independent bounded cooperative application runtime.
#![no_std]
#![forbid(unsafe_code)]
mod ports;
mod runtime;
pub use ports::{OwnedRx, PortError, RX_CAPACITY, Scope, SessionFactory, Transport, Work};
pub use runtime::{Budgets, Progress, Runtime, RuntimeError, StepReport};

mod events;
pub use events::{BoundedRx, EventRuntime, EventRuntimeError, EventStepReport, EventTransport};
