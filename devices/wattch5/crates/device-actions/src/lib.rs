//! Compile-time optional action bodies and response-admission safety. No device I/O.
#![no_std]
/// Explicit portable action failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionError {
    /// Route not compiled or body does not match a supported route.
    Unsupported,
    /// Invalid field enum, identity or length.
    Invalid,
    /// Insufficient caller output buffer.
    Capacity,
    /// Scope is not currently granted.
    Denied,
    /// Another deferred system action is pending.
    Busy,
    /// No matching pending ticket exists.
    InvalidState,
    /// Backend returned a real failure or uncertain outcome.
    Backend,
}
#[cfg(any(
    feature = "system",
    feature = "find",
    feature = "phone",
    feature = "call",
    feature = "media",
    feature = "notification-actions"
))]
mod commands;
#[cfg(any(
    feature = "system",
    feature = "find",
    feature = "phone",
    feature = "call",
    feature = "media",
    feature = "notification-actions"
))]
pub use commands::Action;
#[cfg(feature = "system")]
mod system;
#[cfg(feature = "system")]
pub use system::{DeferredSystem, SystemBackend, SystemScope};
