//! Optional hardware-independent health service contracts. No algorithms or simulated production backend.
#![no_std]
mod codec;
pub use codec::{HealthStart, Latest, MeasurementMode, Module, ResultCarrier, Timestamp};
#[cfg(any(
    feature = "heart-rate",
    feature = "blood-pressure",
    feature = "oxygen",
    feature = "temperature"
))]
pub use codec::{ThresholdEvent, ThresholdSample, ThresholdTransition};
/// Explicit service failures, mapped to public protocol status by the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Incorrect exact length or excessive message size.
    InvalidLength,
    /// A field, ownership relation or validity bit is invalid.
    InvalidField,
    /// Feature or operation is not selected/supported.
    Unsupported,
    /// A real backend is unavailable; never a successful empty measurement.
    Unavailable,
    /// Required physical resources are already owned by another active job.
    Busy,
    /// All static slots are active or retain undelivered events.
    Capacity,
    /// Unknown, evicted, or wrong-module job.
    NotFound,
    /// An acknowledgement or transition is premature.
    InvalidState,
    /// Job/sample/count identifiers cannot advance without reuse.
    Exhausted,
    /// The backend reported an actual fault.
    Backend,
}
mod backend;
mod jobs;
pub use backend::{
    Backend, BackendError, Outcome, Reason, ResourceSet, ResultInfo, ResultSource, StopProgress,
    TriggerSource,
};
pub use jobs::{EndEvent, JobId, JobManager, JobSnapshot, JobState, Owner, ResultEvent};
#[cfg(feature = "heart-rate")]
mod zones;
#[cfg(feature = "heart-rate")]
pub use zones::{ZoneCounter, ZoneThresholds};
mod origin;
mod result;
pub use origin::{ResultOrigin, SourcedResult};
pub use result::{
    AcquisitionKind, MoodLabel, ProductDeclaration, Provenance, ResultBody, TemperatureMode,
    validate_result_body,
};
#[cfg(any(
    feature = "heart-rate",
    feature = "blood-pressure",
    feature = "oxygen",
    feature = "glucose",
    feature = "temperature",
    feature = "ecg",
    feature = "wellbeing",
    feature = "microcirculation"
))]
mod result_bodies;
#[cfg(feature = "microcirculation")]
pub use result_bodies::Alk;
#[cfg(feature = "blood-pressure")]
pub use result_bodies::BloodPressure;
#[cfg(feature = "ecg")]
pub use result_bodies::Ecg;
#[cfg(feature = "glucose")]
pub use result_bodies::Glucose;
#[cfg(feature = "heart-rate")]
pub use result_bodies::HeartRate;
#[cfg(feature = "oxygen")]
pub use result_bodies::Oxygen;
#[cfg(feature = "temperature")]
pub use result_bodies::Temperature;
#[cfg(feature = "wellbeing")]
pub use result_bodies::{FatigueSnapshot, FatigueTrigger, Wellbeing, WellbeingKind};
