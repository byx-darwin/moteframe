//! Optional2C/2D/2E concrete codecs and bounded offline lifecycle. No installed backend or algorithm.
#![no_std]
/// Explicit failures; runtime maps these to existing protocol status/reason without new core codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Missing,extra or overflowing bytes.
    InvalidLength,
    /// Unknown enum,reservedmask,noncanonical absence or contradictory fields.
    InvalidField,
    /// Feature/operation not selected.
    Unsupported,
    /// Actual backend/capability unavailable.
    Unavailable,
    /// No actual value or unknown identity.
    NotFound,
    /// Undrained events or an incompatible active transition.
    Busy,
    /// Finite static retention is full; nothing is silently forgotten.
    Capacity,
    /// Monotonic identity/count/time cannot advance without wrap.
    Exhausted,
    /// Index,identity or sample contents violate the retained sequence.
    Sequence,
    /// Invalid lifecycle transition.
    InvalidState,
    /// Monotonic clock moved backwards or a sample is in the future.
    Time,
    /// Actual backend fault.
    Backend,
}
/// Actual backend errors,never a successful zero-filled value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendError {
    /// No selected real capability.
    Unavailable,
    /// Operation unavailable in this selected implementation.
    Unsupported,
    /// Actual backend fault.
    Fault,
}
impl From<BackendError> for Error {
    fn from(value: BackendError) -> Self {
        match value {
            BackendError::Unavailable => Self::Unavailable,
            BackendError::Unsupported => Self::Unsupported,
            BackendError::Fault => Self::Backend,
        }
    }
}
/// Compile selection only; this does not advertise a real available backend.
#[must_use]
pub const fn module_selected(module: u8) -> bool {
    match module {
        #[cfg(feature = "rri")]
        0x2c => true,
        #[cfg(feature = "womens-health")]
        0x2d => true,
        #[cfg(feature = "uv")]
        0x2e => true,
        _ => false,
    }
}
macro_rules! redacted {
    ($ty:ty,$label:literal,$length:expr) => {
        impl core::fmt::Debug for $ty {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.debug_struct($label)
                    .field("encoded_length", &$length)
                    .finish()
            }
        }
    };
}
/// UTC validity separate from any measurement validity. Unknown is not the Unix epoch.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Timestamp {
    utc_ms: i64,
    valid: bool,
}
redacted!(Timestamp, "Timestamp", 9);
impl Timestamp {
    /// Explicit unknown UTC.
    #[must_use]
    pub const fn unknown() -> Self {
        Self {
            utc_ms: 0,
            valid: false,
        }
    }
    /// Construct canonical UTC. Valid epoch0 is allowed.
    /// # Errors
    /// Invalid time with nonzero UTC is rejected.
    pub const fn new(utc_ms: i64, valid: bool) -> Result<Self, Error> {
        if !valid && utc_ms != 0 {
            Err(Error::InvalidField)
        } else {
            Ok(Self { utc_ms, valid })
        }
    }
    /// UTC milliseconds,meaningful only if valid.
    #[must_use]
    pub const fn utc_ms(self) -> i64 {
        self.utc_ms
    }
    /// Independent trustworthy-time flag.
    #[must_use]
    pub const fn is_valid(self) -> bool {
        self.valid
    }
}
#[cfg(any(feature = "rri", feature = "womens-health", feature = "uv"))]
fn exact(bytes: &[u8], length: usize) -> Result<(), Error> {
    if bytes.len() == length {
        Ok(())
    } else {
        Err(Error::InvalidLength)
    }
}
#[cfg(any(feature = "rri", feature = "womens-health", feature = "uv"))]
fn boolean(value: u8) -> Result<bool, Error> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(Error::InvalidField),
    }
}
#[cfg(feature = "rri")]
mod rri;
#[cfg(feature = "rri")]
pub use rri::*;
#[cfg(feature = "womens-health")]
mod women;
#[cfg(feature = "womens-health")]
pub use women::*;
#[cfg(feature = "uv")]
mod uv;
#[cfg(feature = "uv")]
pub use uv::*;
