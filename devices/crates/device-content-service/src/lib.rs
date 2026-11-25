//! Optional real-backend content command handlers and one response-first event FIFO.
#![no_std]
#![forbid(unsafe_code)]
/// Local service failures, separate from four-byte wire status responses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Underlying validated content contract failure.
    Content(device_content::Error),
    /// Invalid packet, receipt or integration configuration.
    Invalid,
    /// Well-formed field contains an unsupported enumeration value.
    EnumUnsupported,
    /// A known immutable terminal result was changed.
    TerminalConflict,
    /// Fixed capacity exhausted.
    Capacity,
    /// A queued response/event or shared owner is busy.
    Busy,
    /// Trusted permission or generation was revoked.
    Denied,
    /// Session ended permanently.
    Closed,
    /// External monotonic time failed or reversed.
    Clock,
    /// Real backend failed or cleanup remained uncertain.
    Backend,
}
impl From<device_content::Error> for Error {
    fn from(value: device_content::Error) -> Self {
        Self::Content(value)
    }
}
#[cfg(any(
    feature = "phone",
    feature = "media",
    feature = "find",
    feature = "notifications",
    feature = "companion-radio",
    feature = "gimbal"
))]
mod ports;
#[cfg(any(
    feature = "phone",
    feature = "media",
    feature = "find",
    feature = "notifications",
    feature = "companion-radio",
    feature = "gimbal"
))]
pub use ports::*;
#[cfg(any(
    feature = "phone",
    feature = "media",
    feature = "find",
    feature = "notifications",
    feature = "companion-radio",
    feature = "gimbal"
))]
mod wire;
#[cfg(any(
    feature = "phone",
    feature = "media",
    feature = "find",
    feature = "notifications",
    feature = "companion-radio",
    feature = "gimbal"
))]
pub use wire::{Command, EventToken};
#[cfg(any(
    feature = "phone",
    feature = "media",
    feature = "find",
    feature = "notifications",
    feature = "companion-radio",
    feature = "gimbal"
))]
mod state;
#[cfg(any(
    feature = "phone",
    feature = "media",
    feature = "find",
    feature = "notifications",
    feature = "companion-radio",
    feature = "gimbal"
))]
pub use state::{CleanupReport, SharedContentOwner};
#[cfg(any(
    feature = "phone",
    feature = "media",
    feature = "find",
    feature = "notifications",
    feature = "companion-radio",
    feature = "gimbal"
))]
mod handler;
#[cfg(any(
    feature = "phone",
    feature = "media",
    feature = "find",
    feature = "notifications",
    feature = "companion-radio",
    feature = "gimbal"
))]
pub use handler::{Binding, ContentHandler};

#[cfg(all(
    feature = "event-pump",
    any(
        feature = "phone",
        feature = "media",
        feature = "find",
        feature = "notifications",
        feature = "companion-radio",
        feature = "gimbal"
    )
))]
mod pump;
#[cfg(all(
    feature = "event-pump",
    any(
        feature = "phone",
        feature = "media",
        feature = "find",
        feature = "notifications",
        feature = "companion-radio",
        feature = "gimbal"
    )
))]
pub use pump::{ContentEventSource, ContentHost, ContentOperations, ContentRequests};
