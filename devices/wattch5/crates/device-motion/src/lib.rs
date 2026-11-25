//! Optional motion protocol state. Physical sensors, algorithms and durable storage are ports.
#![no_std]
/// Portable motion-service failure, mapped to existing WAP status by integration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionError {
    /// Invalid fields or identity.
    Invalid,
    /// Another session owns resources.
    Busy,
    /// Session or alias unknown.
    NotFound,
    /// Terminal or conflicting operation.
    Conflict,
    /// Backend configuration or persistence is not supported.
    Unavailable,
    /// Static capacity or counter/ID exhausted.
    Capacity,
    /// Monotonic input moved backwards.
    Time,
    /// Actual or unknown backend failure; do not claim a confirmed state.
    Backend,
}
#[cfg(feature = "sport")]
mod sport;
#[cfg(feature = "sport")]
pub use sport::{ContextAliases, SportBackend, SportEngine, SportSession, SportState};

#[cfg(any(
    feature = "sport",
    feature = "sleep",
    feature = "activity",
    feature = "wear"
))]
mod records;
#[cfg(feature = "activity")]
pub use records::ActivityRecord;
#[cfg(feature = "wear")]
pub use records::{PostureInterval, WearInterval};
#[cfg(feature = "sleep")]
pub use records::{SleepDetection, SleepInterval, SleepProvisional};
#[cfg(feature = "sport")]
pub use records::{
    SportCadenceSample, SportDistanceSample, SportGpsSample, SportHeartSample, SportPaceSample,
    SportRealtime, SportRespirationSample, SportRriSample, SportSummary,
};
