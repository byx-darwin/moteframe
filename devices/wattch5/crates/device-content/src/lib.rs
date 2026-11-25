//! Optional typed content codecs and connection-scoped action services.
#![no_std]
#![forbid(unsafe_code)]
/// Content operation failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Endpoint is not compiled.
    Unknown,
    /// Invalid field, length, range or encoding.
    Invalid,
    /// Required manifest or real backend is absent.
    Unavailable,
    /// Fixed capacity is exhausted.
    Capacity,
    /// Unknown identity.
    NotFound,
    /// Identity was reused with different data or result.
    Conflict,
    /// Trusted access policy denied the operation.
    Denied,
    /// Call order or monotonic clock is invalid.
    State,
    /// Backend could not complete the operation.
    Backend,
}

#[cfg(any(
    feature = "phone",
    feature = "companion-radio",
    feature = "weather",
    feature = "contacts",
    feature = "memo",
    feature = "find",
    feature = "notifications",
    feature = "wallet",
    feature = "watchface"
))]
mod codecs;
#[cfg(any(
    feature = "phone",
    feature = "companion-radio",
    feature = "weather",
    feature = "contacts",
    feature = "memo",
    feature = "find",
    feature = "notifications",
    feature = "wallet",
    feature = "watchface",
    feature = "media",
    feature = "gimbal"
))]
mod wire;
#[cfg(any(
    feature = "phone",
    feature = "companion-radio",
    feature = "weather",
    feature = "contacts",
    feature = "memo",
    feature = "find",
    feature = "notifications",
    feature = "wallet",
    feature = "watchface"
))]
pub use codecs::*;
#[cfg(any(
    feature = "phone",
    feature = "media",
    feature = "companion-radio",
    feature = "gimbal",
    feature = "find",
    feature = "notifications"
))]
mod access;
#[cfg(any(
    feature = "phone",
    feature = "media",
    feature = "companion-radio",
    feature = "gimbal",
    feature = "find",
    feature = "notifications"
))]
pub use access::*;
#[cfg(any(feature = "phone", feature = "media"))]
mod actions;
#[cfg(any(feature = "phone", feature = "media"))]
pub use actions::*;
#[cfg(feature = "find")]
mod find;
#[cfg(feature = "find")]
pub use find::*;
#[cfg(feature = "notifications")]
mod notifications;
#[cfg(feature = "notifications")]
pub use notifications::*;
#[cfg(feature = "phone")]
mod phone;
#[cfg(feature = "phone")]
pub use phone::*;

#[cfg(any(
    feature = "companion-radio",
    feature = "weather",
    feature = "contacts",
    feature = "memo"
))]
mod phone_content;
#[cfg(any(
    feature = "companion-radio",
    feature = "weather",
    feature = "contacts",
    feature = "memo"
))]
pub use phone_content::*;

#[cfg(any(feature = "companion-radio", feature = "gimbal"))]
mod backend;
#[cfg(any(feature = "companion-radio", feature = "gimbal"))]
pub use backend::*;
#[cfg(feature = "gimbal")]
mod gimbal;
#[cfg(feature = "gimbal")]
pub use gimbal::*;
#[cfg(feature = "companion-radio")]
mod companion_radio;
#[cfg(feature = "companion-radio")]
pub use companion_radio::*;
