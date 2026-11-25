//! Typed port boundaries. No byte passthrough or successful missing observation.
#[cfg(feature = "activity")]
use device_motion::ActivityRecord;
#[cfg(feature = "wear")]
use device_motion::{PostureInterval, WearInterval};
#[cfg(feature = "sleep")]
use device_motion::{SleepInterval, SleepProvisional};
#[cfg(feature = "sport")]
use device_motion::{SportBackend, SportRealtime, SportSummary};
/// Local explicit failure; no variant represents successful absent data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// Invalid request fields.
    Invalid,
    /// Unsupported opcode/selected backend.
    Unsupported,
    /// No actual observation or stable identity.
    NotFound,
    /// Invalid current resource lifecycle.
    InvalidState,
    /// Existing device or owner resources are busy.
    Busy,
    /// Revision, terminal state or real binding conflicts.
    Conflict,
    /// Actual fixed-buffer/ID/duration capacity exhausted.
    Capacity,
    /// Trusted scope absent/revoked.
    Denied,
    /// Real persistent operation failed.
    Storage,
    /// Actual physical backend failed or has an unknown outcome.
    Backend,
    /// Port returned malformed typed data or metadata.
    Contract,
    /// Trusted monotonic clock failed/regressed.
    Clock,
    /// This App connection ended; device-owned sport may continue.
    Closed,
}
/// Trusted action or read scope; the request never grants one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    /// Module runtime status.
    Status,
    /// Actual latest result.
    Latest,
    /// Begin device-owned sport.
    Start,
    /// Pause/resume/stop sport.
    Control,
    /// Detail persistence policy.
    Save,
    /// Allocate/resolve sport context alias.
    Context,
    /// Query actual UTC-window sleep statistics.
    SleepSummary,
    /// Atomically clear a selected live activity counter.
    Clear,
    /// Actual live or completed observation notification.
    Event,
}
/// Concrete resource identity without truncating sport IDs.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Resource {
    /// Module-level permission before parsing.
    Module,
    /// Stable full-width sport session.
    Session(u64),
    /// Explicit record type.
    Record(u16),
    /// Activity clear scope1day/2flexible segment.
    ActivityScope(u8),
}
impl core::fmt::Debug for Resource {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Resource").finish_non_exhaustive()
    }
}
/// Live trusted scope, bound by integration to the actual App principal/connection.
pub trait MotionAccess {
    /// Check actual module/action/resource scope; no default permission.
    fn allowed(&self, module: u8, operation: Operation, resource: Resource) -> bool;
}
/// Actual monotonic source, independent of phone timestamps and UTC adjustments.
pub trait MotionClock {
    /// Read trusted milliseconds; no zero fallback on failure.
    /// # Errors
    /// Missing/faulted source is explicit.
    fn now_ms(&self) -> Result<u64, Failure>;
}
/// Actual observed UTC metadata, independent of value validity.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Timestamp {
    /// Actual UTC milliseconds; zero if invalid.
    pub utc_ms: i64,
    /// True only when the source actually knows UTC.
    pub valid: bool,
}
impl core::fmt::Debug for Timestamp {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Timestamp")
            .field("encoded_length", &9)
            .finish_non_exhaustive()
    }
}
/// One immutable actual observation; generation is nonzero and never reused in its context.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Snapshot<T> {
    /// Actual immutable backend generation.
    pub generation: u64,
    /// Actual interval/result timestamp, never substituted with read time.
    pub timestamp: Timestamp,
    /// Typed validated business content.
    pub value: T,
}
impl<T> core::fmt::Debug for Snapshot<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Snapshot").finish_non_exhaustive()
    }
}
/// Exact runtime status body, separate from sport control states.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ModuleStatus {
    /// 0unavailable/1ready/2collecting/3fault.
    pub state: u8,
    /// Actual module reason, distinct from the response prefix.
    pub reason: u16,
}
/// Typed selection for an actual LATEST result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LatestSelector {
    #[cfg(feature = "sleep")]
    /// Sleep0021, final schema1 or provisional schema2.
    Sleep,
    #[cfg(feature = "sport")]
    /// Sport2201/schema2.
    Sport,
    #[cfg(feature = "sport")]
    /// Read existing0022/schema1 data only; not normal22.02 generation.
    LegacySport,
    #[cfg(feature = "activity")]
    /// Activity2301/schema1.
    Activity,
    #[cfg(feature = "wear")]
    /// Wear2401/schema2.
    Wear,
    #[cfg(feature = "wear")]
    /// Posture2402/schema2.
    Posture,
}
/// Actual typed results; a backend cannot smuggle an arbitrary byte layout through LATEST.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Observation {
    #[cfg(feature = "sleep")]
    /// Actual finalized sleep interval.
    SleepFinal(Snapshot<SleepInterval>),
    #[cfg(feature = "sleep")]
    /// Actual current sleep interval, provisional1.
    SleepCurrent(Snapshot<SleepProvisional>),
    #[cfg(feature = "sport")]
    /// Actual current or completed sport summary.
    Sport(Snapshot<SportSummary>),
    #[cfg(feature = "sport")]
    /// Actual existing frozen0022/schema1 content.
    LegacySport(Snapshot<crate::LegacySportSummary>),
    #[cfg(feature = "activity")]
    /// Actual complete activity snapshot.
    Activity(Snapshot<ActivityRecord>),
    #[cfg(feature = "wear")]
    /// Actual wear interval.
    Wear(Snapshot<WearInterval>),
    #[cfg(feature = "wear")]
    /// Actual posture interval.
    Posture(Snapshot<PostureInterval>),
}
#[cfg(feature = "sleep")]
/// Typed reliable UTC window, left-closed/right-open.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SleepWindow {
    /// Requested inclusive UTC start.
    pub start_ms: i64,
    /// Requested exclusive UTC end; greater than start.
    pub end_ms: i64,
}
#[cfg(feature = "sleep")]
/// Actual clipped statistics. Unsupported stages clear their mask bits.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SleepSummary {
    /// Sleep total=deep+light+supported REM; excludes awake/interrupted.
    pub total_ms: u32,
    /// Actual deep overlap milliseconds.
    pub deep_ms: u32,
    /// Actual light overlap milliseconds.
    pub light_ms: u32,
    /// Actual awake overlap milliseconds.
    pub awake_ms: u32,
    /// Actual REM overlap milliseconds, only with selected real capability.
    pub rem_ms: u32,
    /// Actual interrupted overlap milliseconds.
    pub interrupted_ms: u32,
    /// Bits0total/1deep/2light/3awake/4REM/5interrupted; reserved0.
    pub valid_mask: u16,
}
#[cfg(feature = "sleep")]
impl core::fmt::Debug for SleepSummary {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SleepSummary")
            .field("encoded_length", &26)
            .finish_non_exhaustive()
    }
}
#[cfg(feature = "wear")]
/// Explicit type of a wear/posture change event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WearKind {
    /// Record2401, values0unknown/1worn/2not-worn.
    Wear,
    /// Record2402, values0sitting/1standing/255unknown.
    Posture,
}
/// Exact typed event body selection. Ordinary events have no added CRC/status prefix.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EventBody {
    #[cfg(feature = "sleep")]
    /// E80 LATEST+actual sleep interval.
    Sleep(Observation),
    #[cfg(feature = "sport")]
    /// E80 fixed26B, no LATEST wrapper.
    SportRealtime(SportRealtime),
    #[cfg(feature = "sport")]
    /// E81 `end_reason+LATEST+46B` summary.
    SportEnd {
        /// Existing proposed reason0/0104/0105.
        reason: u16,
        /// Actual completed summary.
        result: Snapshot<SportSummary>,
    },
    #[cfg(feature = "activity")]
    /// E80 full activity snapshot.
    ActivityCurrent(Snapshot<ActivityRecord>),
    #[cfg(feature = "activity")]
    /// E81 finalized fixed/flexible activity snapshot.
    ActivityEnd(Snapshot<ActivityRecord>),
    #[cfg(feature = "wear")]
    /// E80 exact12B change, no LATEST wrapper.
    WearChange {
        /// Explicit selected record type.
        kind: WearKind,
        /// Actual UTC validity metadata.
        timestamp: Timestamp,
        /// Actual state from that selected type.
        value: u8,
    },
    #[cfg(feature = "wear")]
    /// E81 finalized wear interval.
    WearEnd(Snapshot<WearInterval>),
    #[cfg(feature = "wear")]
    /// E81 finalized posture interval.
    PostureEnd(Snapshot<PostureInterval>),
}
/// One actual immutable pending event, from a real producer rather than phone payload.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct BackendEvent {
    /// Nonzero producer generation, unique per module/context.
    pub generation: u64,
    /// Actual typed content.
    pub body: EventBody,
}
/// Pending whole control-event receipt, returned without inventing a transport message ID.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct EventToken {
    pub(crate) connection: u64,
    pub(crate) module: u8,
    pub(crate) opcode: u8,
    pub(crate) generation: u64,
    pub(crate) slot: usize,
}
impl core::fmt::Debug for EventToken {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("EventToken").finish_non_exhaustive()
    }
}
/// Actual event body offered to the runtime's ordered control-event queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OfferedEvent {
    /// Opaque receipt to confirm only after actual whole-message admission.
    pub token: EventToken,
    /// Actual wire module.
    pub module: u8,
    /// E80/E81.
    pub opcode: u8,
    /// Exact bytes copied, without transport header/status/CRC.
    pub length: usize,
}
/// Real selected motion backends. No simulated/default-success implementation exists.
pub trait MotionBackend {
    /// Whether a real product-selected port exists for this compiled module.
    fn selected(&self, module: u8) -> bool;
    /// # Errors
    /// Return actual runtime status or real backend failure.
    fn status(&self, module: u8) -> Result<ModuleStatus, Failure>;
    /// Read a consistent actual typed observation; absence is `NotFound`.
    /// # Errors
    /// Real unavailable/source/storage failures, never fabricated zero results.
    fn latest(&self, selector: LatestSelector) -> Result<Observation, Failure>;
    #[cfg(feature = "sport")]
    /// Actual sport resource/detail writer, never a simulated successful port.
    /// # Errors
    /// Missing selected backend is Unsupported; real faults are explicit.
    fn sport_backend(&mut self) -> Result<&mut dyn SportBackend, Failure>;
    #[cfg(feature = "sleep")]
    /// Intersect real UTC-valid stored intervals with [start,end), clip endpoints,
    /// prorate actual monotonic duration and round down. No reliable UTC ->`NotFound`.
    /// total excludes awake/interrupted; unsupported stages clear bits.
    /// # Errors
    /// Actual missing data, stable-read, range or storage failure.
    fn sleep_summary(&self, window: SleepWindow) -> Result<SleepSummary, Failure>;
    #[cfg(feature = "activity")]
    /// Atomic real clear scope1current-day/2current-flexible, then return the actual
    /// post-clear typed snapshot. Recheck live access at the same serialization point.
    /// Finalize old snapshot; never erase history or a running sport's counters.
    /// # Errors
    /// Error preserves old state or explicitly reports unknown backend outcome.
    fn clear_activity(
        &mut self,
        scope: u8,
        access: &dyn MotionAccess,
    ) -> Result<Snapshot<ActivityRecord>, Failure>;
    /// Return an actual immutable selected event after this context's last generation.
    /// No hardware sample or source result means None, never a generated measurement.
    /// # Errors
    /// Actual producer/context failure.
    fn pending_event(
        &self,
        connection: u64,
        module: u8,
        resource: Resource,
        after: u64,
    ) -> Result<Option<BackendEvent>, Failure>;
    /// Whether the immutable pending token remains valid for this source/principal context.
    fn event_current(&self, connection: u64, module: u8, generation: u64) -> bool;
    /// Confirm actual complete event queue admission; not phone persistence or history ACK.
    /// # Errors
    /// Real/stale source failures. Must not count a retransmission as a new sample.
    fn event_admitted(
        &mut self,
        connection: u64,
        module: u8,
        generation: u64,
    ) -> Result<(), Failure>;
    /// Drop an unstarted invalidated event; this is never a successful delivery/ACK.
    /// # Errors
    /// Real producer cleanup failure.
    fn discard_event(
        &mut self,
        connection: u64,
        module: u8,
        generation: u64,
    ) -> Result<(), Failure>;
    /// Remove only this App connection's subscriptions/pending events. Device-owned
    /// sport continues; never implicitly transition the physical sport to stopped.
    /// # Errors
    /// Actual cleanup failure remains retryable and blocks a new App owner.
    fn detach(&mut self, connection: u64) -> Result<(), Failure>;
}

#[cfg(feature = "sleep")]
impl core::fmt::Debug for SleepWindow {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SleepWindow")
            .field("encoded_length", &16)
            .finish_non_exhaustive()
    }
}
impl core::fmt::Debug for EventBody {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let length = match self {
            #[cfg(feature = "sleep")]
            Self::Sleep(Observation::SleepFinal(_)) => 22,
            #[cfg(feature = "sleep")]
            Self::Sleep(Observation::SleepCurrent(_)) => 23,
            #[cfg(all(
                feature = "sleep",
                any(feature = "sport", feature = "activity", feature = "wear")
            ))]
            Self::Sleep(_) => 0,
            #[cfg(feature = "sport")]
            Self::SportRealtime(_) => 26,
            #[cfg(feature = "sport")]
            Self::SportEnd { .. } => 63,
            #[cfg(feature = "activity")]
            Self::ActivityCurrent(_) | Self::ActivityEnd(_) => 46,
            #[cfg(feature = "wear")]
            Self::WearChange { .. } => 12,
            #[cfg(feature = "wear")]
            Self::WearEnd(_) | Self::PostureEnd(_) => 34,
        };
        f.debug_struct("EventBody")
            .field("encoded_length", &length)
            .finish_non_exhaustive()
    }
}
impl core::fmt::Debug for BackendEvent {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BackendEvent").finish_non_exhaustive()
    }
}

impl core::fmt::Debug for ModuleStatus {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ModuleStatus")
            .field("encoded_length", &3)
            .finish_non_exhaustive()
    }
}
