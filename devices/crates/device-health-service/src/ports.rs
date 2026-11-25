use device_health::{
    HealthStart, Owner, ProductDeclaration, Provenance, ResultInfo, ResultSource, TriggerSource,
};
/// Explicit local failure. No variant represents a successful absent measurement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Health(device_health::Error),
    Busy,
    Denied,
    Closed,
    Clock,
    Invalid,
    Capacity,
    Backend,
}
impl From<device_health::Error> for Error {
    fn from(e: device_health::Error) -> Self {
        Self::Health(e)
    }
}
/// Trusted resource operation, never granted by request bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    /// Authorized App ingestion, never an actual sensor start.
    Import,
    Status,
    CuffEvent,
    Start,
    Stop,
    Query,
    Latest,
    ResultEvent,
    EndEvent,
    Zones,
}
/// Live authorization bound to the actual connection identity. Some(id) denotes a job/context;
/// Latest uses `Some(record_type)`. Implementations must reject jobs owned by other connections.
pub trait HealthAccess {
    fn allowed(&self, module: u8, operation: Operation, id: Option<u32>) -> bool;
}
/// Trusted monotonic time, independent of peer timestamps.
pub trait HealthClock {
    /// # Errors
    /// Reports a missing/faulted trusted clock; peer time is never substituted.
    fn now_ms(&self) -> Result<u64, Error>;
}
/// Audited immutable product declarations for one handler lifecycle. Reclassification/provider
/// replacement requires a new lifecycle; declarations and provenance are not peer assertions.
pub trait HealthProduct: HealthSource {
    /// # Errors
    /// Reports unsupported/missing audited product declarations.
    fn declaration(&self, module: u8, record_type: u16) -> Result<ProductDeclaration<'_>, Error>;
    /// Select a trusted declaration for a stored/local or imported result.
    /// Existing single-provider products keep their original declaration.
    /// # Errors
    /// Missing or unsupported actual sources remain explicit.
    fn declaration_for_source(
        &self,
        module: u8,
        record_type: u16,
        _source: ResultSource,
    ) -> Result<ProductDeclaration<'_>, Error> {
        self.declaration(module, record_type)
    }
    /// # Errors
    /// Reports missing actual provider/source metadata.
    fn provenance(
        &self,
        module: u8,
        record_type: u16,
        source: ResultSource,
    ) -> Result<Provenance<'_>, Error>;
    /// Select actual ownership and trigger. Device ownership must be an independently authorized
    /// product policy decision; ordinary App starts use Application/App.
    /// # Errors
    /// Reports missing policy or unsupported ownership/trigger selection.
    fn start_binding(&self, request: HealthStart) -> Result<(Owner, TriggerSource), Error>;
}
/// Metadata for a real committed LATEST snapshot.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct StoredLatest {
    pub info: ResultInfo,
    pub generation: u64,
}
/// Actual storage port. No default memory/default-result implementation exists. Reads are one
/// committed snapshot; missing data is `NotFound`. Publish atomically stores actual validated bytes,
/// deduplicated by(job,sample), and must retain previous state on errors. Generation is nonzero,
/// immutable for a snapshot, and never reused while a cached response can exist.
pub trait LatestStore {
    /// # Errors
    /// Returns `NotFound` for no actual committed value; storage/capacity failures remain explicit.
    fn load(&self, module: u8, record_type: u16, out: &mut [u8]) -> Result<StoredLatest, Error>;
    /// # Errors
    /// Reports atomic storage/capacity failures and preserves the previous committed value.
    fn publish(
        &mut self,
        module: u8,
        job: u32,
        info: ResultInfo,
        payload: &[u8],
    ) -> Result<(), Error>;
    fn current(&self, module: u8, record_type: u16, generation: u64) -> bool;
}
/// Optional actual five-zone context port. Selection does not create algorithm samples or contexts.
/// Snapshot thresholds remain fixed for that live sport context. pending/confirm refer to one
/// immutable revision; implementations count actual sample IDs once, not BLE retry calls.
pub trait ZonePort {
    /// # Errors
    /// Reports unknown/expired real contexts and backend errors.
    fn snapshot(&self, context: u32) -> Result<crate::ZoneSummary, Error>;
    /// # Errors
    /// Reports expired contexts or actual backend errors, never a generated sample.
    fn pending(&self, context: u32) -> Result<Option<(u64, crate::ZoneSummary)>, Error>;
    /// # Errors
    /// Reports stale revisions or actual admission acknowledgement errors.
    fn confirm(&mut self, context: u32, revision: u64) -> Result<(), Error>;
    fn current(&self, context: u32, revision: u64) -> bool;
}
/// Actual module state provider. It must consult the selected real acquisition source, rather than
/// infer readiness from a vacant manager slot. The source is immutable for the connection lifecycle.
pub trait HealthSource {
    /// # Errors
    /// Missing actual source/state is explicit Unsupported/Unavailable, never synthetic Ready.
    fn module_run_status(&self, module: u8) -> Result<crate::ModuleRunStatus, Error>;
}
/// Optional real cuff acquisition port for25.E82. PPG-derived blood pressure is not a cuff.
/// Samples have nonzero increasing immutable IDs and remain retained until actual confirmation.
/// Every method must bind to the same physical cuff job in the shared Backend.
pub trait CuffPort {
    fn running_cuff_job(&self, job: u32) -> bool;
    /// # Errors
    /// Unknown/non-cuff/nonrunning jobs and actual backend failures are explicit.
    fn pending(&self, job: u32) -> Result<Option<(u64, crate::CuffPressure)>, Error>;
    /// # Errors
    /// Actual acknowledgement failures retain the identical sample for retry.
    fn confirm(&mut self, job: u32, sample: u64) -> Result<(), Error>;
    fn current(&self, job: u32, sample: u64) -> bool;
}

impl core::fmt::Debug for StoredLatest {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("StoredLatest")
            .field("payload_length", &self.info.length)
            .finish_non_exhaustive()
    }
}
