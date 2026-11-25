//! Explicit ports for actual measurement backends. No backend is installed by this crate.
use crate::{HealthStart, JobId, Timestamp};
/// Physical resource ownership bits, defined by the product's selected backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceSet(u32);
impl ResourceSet {
    /// Construct a product-defined resource set; zero is valid for software/import operations.
    #[must_use]
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }
    /// Product-defined bits, not GPIO or memory addresses.
    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }
    /// Whether two sets need at least one shared physical resource.
    #[must_use]
    pub const fn conflicts(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
}
/// A real backend's explicit failure, never a successful default value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendError {
    /// Operation, mode, or schema not implemented by this selected backend.
    Unsupported,
    /// Sensor/algorithm/input source not currently available.
    Unavailable,
    /// Caller output buffer cannot hold the actual result.
    Capacity,
    /// Actual backend fault. A failed stop does not prove resources were released.
    Fault,
}
/// Definite stop versus asynchronous cleanup still holding physical resources.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopProgress {
    /// Hardware and resources have actually stopped.
    Stopped,
    /// Cleanup must be polled until a definite completion.
    Stopping,
}
/// Actual algorithm/import provenance, independent of the action's trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ResultSource {
    /// Selected device algorithm produced this result.
    DeviceAlgorithm = 1,
    /// Authorized phone supplied this value; not a device measurement.
    PhoneImport = 2,
}
impl ResultSource {
    /// Decode the explicitly proposed provenance dictionary.
    /// # Errors
    /// Unknown is not a valid measured/imported source.
    pub const fn from_wire(value: u8) -> Result<Self, crate::Error> {
        match value {
            1 => Ok(Self::DeviceAlgorithm),
            2 => Ok(Self::PhoneImport),
            _ => Err(crate::Error::InvalidField),
        }
    }
}
/// Why a job was triggered; this is not algorithm provenance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TriggerSource {
    /// App command.
    App = 1,
    /// Device UI/key.
    DeviceUser = 2,
    /// Local scheduled policy.
    Timer = 3,
    /// Explicit sport context.
    Sport = 4,
}
/// Proposed module-domain reasons; the caller maps these separately from core status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Reason {
    /// No additional reason.
    None = 0,
    /// Contact or wear insufficient.
    BadContact = 0x0101,
    /// No valid sample produced.
    NoSamples = 0x0102,
    /// Actual backend timeout.
    Timeout = 0x0103,
    /// Explicit user stop.
    UserCancelled = 0x0104,
    /// Actual backend fault.
    BackendFault = 0x0105,
    /// Application connection ended.
    Disconnected = 0x0106,
    /// Required capability disappeared.
    Unavailable = 0x0107,
    /// Module25 cuff pressure failed to reach30mmHg within7seconds.
    #[cfg(feature = "blood-pressure")]
    CuffPressureRiseFailure = 0x0201,
    /// Module25 cuff overpressure protection at295mmHg.
    #[cfg(feature = "blood-pressure")]
    CuffOverpressure = 0x0202,
    /// Module25 cuff found no pulse.
    #[cfg(feature = "blood-pressure")]
    CuffNoPulse = 0x0203,
    /// Module25 cuff observation was disturbed by interference.
    #[cfg(feature = "blood-pressure")]
    CuffInterference = 0x0204,
    /// Module25 cuff produced an invalid result.
    #[cfg(feature = "blood-pressure")]
    CuffInvalidResult = 0x0205,
    /// Module25 cuff reported insufficient voltage.
    #[cfg(feature = "blood-pressure")]
    CuffLowVoltage = 0x0206,
    /// Module25 cuff EEPROM fault.
    #[cfg(feature = "blood-pressure")]
    CuffEeprom = 0x0207,
}
impl Reason {
    /// Whether a reason belongs to this module. Cuff reasons belong only to25.
    #[must_use]
    pub const fn applies_to(self, module: u8) -> bool {
        (self as u16) < 0x0200 || module == 0x25
    }
    /// Convert only the original cuff-module raw1..7 error domain (legacy appendix7).
    /// The separate original3.11 domain is not accepted or inferred here; raw0 means no cuff error.
    /// # Errors
    /// Zero, unknown/reserved raw values or values outside1..7 are invalid.
    #[cfg(feature = "blood-pressure")]
    pub const fn from_legacy_cuff_error(raw: u8) -> Result<Self, crate::Error> {
        match raw {
            1 => Ok(Self::CuffPressureRiseFailure),
            2 => Ok(Self::CuffOverpressure),
            3 => Ok(Self::CuffNoPulse),
            4 => Ok(Self::CuffInterference),
            5 => Ok(Self::CuffInvalidResult),
            6 => Ok(Self::CuffLowVoltage),
            7 => Ok(Self::CuffEeprom),
            _ => Err(crate::Error::InvalidField),
        }
    }
}
/// A definite backend lifecycle outcome, not proof of a valid physiological result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Process completed; result absence remains explicit.
    Completed,
    /// Stopped with an explicit reason.
    Cancelled(Reason),
    /// Failed with an explicit reason.
    Failed(Reason),
}
/// Metadata for a real result retained by the backend until queue admission is acknowledged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResultInfo {
    /// Nonzero strictly increasing per-job sample identity; retransmission keeps identity/content.
    pub sample_id: u64,
    /// Actual result type; must match the request.
    pub record_type: u16,
    /// Concrete implemented payload schema revision; unknown schemas are rejected before events.
    pub schema: u16,
    /// Independent trustworthy UTC or explicit unknown.
    pub timestamp: Timestamp,
    /// Actual bytes written by `result` into caller output.
    pub length: usize,
    /// Actual source, never inferred from the trigger.
    pub source: ResultSource,
}
/// Hardware/algorithm/import port. Implementations belong to a product, not this pure service.
///
/// `resources` is side-effect free and must reject unavailable/unsupported mode/type combinations.
/// `start` returning Err must have rolled back acquired resources; uncertain cleanup instead remains
/// a managed accepted job and later reports a definite failed completion. `stop` Err retains ownership.
/// Result identity, metadata and bytes remain immutable until `acknowledge_result`; completion must
/// not discard an outstanding result. These methods neither authenticate nor authorize callers.
pub trait Backend {
    /// Resolve actual availability, supported mode and required resource set.
    /// # Errors
    /// Explicit unavailable/unsupported/fault, never an invented resource or sample.
    fn resources(&self, request: HealthStart) -> Result<ResourceSet, BackendError>;
    /// Check a nonzero sport alias against the currently active context lifecycle.
    fn context_is_valid(&self, context_id: u32) -> bool;
    /// Begin actual work after resource arbitration, without emitting transport events directly.
    /// # Errors
    /// Rejected start must leave no acquired resources; no successful job is returned.
    fn start(&mut self, job: JobId, request: HealthStart) -> Result<(), BackendError>;
    /// Stop actual work. A returned `Stopped` must mean resources are released.
    /// # Errors
    /// Error leaves the job/resource lease active for explicit recovery.
    fn stop(&mut self, job: JobId) -> Result<StopProgress, BackendError>;
    /// Poll a definite completion; None means still active, not a fake successful result.
    /// # Errors
    /// A polling fault does not establish cleanup; resources remain leased.
    fn completion(&mut self, job: JobId) -> Result<Option<Outcome>, BackendError>;
    /// Whether this job still has a real retained result. After definite completion no new
    /// production may appear; existing results remain until acknowledged. Side-effect free.
    /// # Errors
    /// A fault cannot be interpreted as an empty result queue.
    fn has_pending_result(&self, job: JobId) -> Result<bool, BackendError>;
    /// Copy an actual retained result into caller storage, or report no result.
    /// # Errors
    /// Buffer shortage/unavailability/fault are explicit errors. No data is synthesized.
    fn result(&mut self, job: JobId, output: &mut [u8])
    -> Result<Option<ResultInfo>, BackendError>;
    /// Acknowledge actual transport admission exactly once per sample.
    /// # Errors
    /// On failure the same retained result must remain available for retry.
    fn acknowledge_result(&mut self, job: JobId, sample_id: u64) -> Result<(), BackendError>;
}
