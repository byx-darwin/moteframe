use device_wellness::Timestamp;
#[cfg(feature = "womens-health")]
use device_wellness::WomensBackend;
#[cfg(feature = "rri")]
use device_wellness::{RriOrigin, RriTracker};
#[cfg(feature = "uv")]
use device_wellness::{
    UvAlarmEvent, UvEngine, UvProvenance, UvReportEvent, UvSample, UvValueOrigin,
};
/// Explicit local failures; successful absence is never represented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// Invalid request.
    Invalid,
    /// Missing operation/backend.
    Unsupported,
    /// Required actual identity/storage capability is unverified or unavailable.
    Unavailable,
    /// No real result/identity.
    NotFound,
    /// Invalid lifecycle.
    InvalidState,
    /// Busy checked borrow/resource.
    Busy,
    /// State binding changed.
    Conflict,
    /// Fixed capacity exhausted.
    Capacity,
    /// Trusted scope absent.
    Denied,
    /// Actual storage fault.
    Storage,
    /// Actual hardware/backend fault.
    Backend,
    /// Malformed typed source/metadata.
    Contract,
    /// Monotonic clock failure/regression.
    Clock,
    /// Permanently ended App connection.
    Closed,
}
/// Actual trusted operation scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operation {
    /// Real status/config policy state.
    Status,
    /// Actual snapshot.
    Latest,
    /// App subscribes to an existing device-owned source.
    Subscribe,
    /// Actual event notification.
    Event,
    /// Cancel exact current UV alarm after response admission.
    Cancel,
}
/// Full source identity; Debug never prints identities.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Resource {
    /// Module-level scope.
    Module,
    /// Full measurement plus optional full sport parent, never truncated.
    Measurement {
        /// Real device-owned measurement ID.
        id: u64,
        /// Actual full sport parent.
        parent: Option<u64>,
    },
    /// Original boot-scoped UV alarm.
    Alarm(u32),
}
impl core::fmt::Debug for Resource {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Resource").finish_non_exhaustive()
    }
}
/// Trusted source class from actual product/backend metadata, never extra wire flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    #[cfg(feature = "rri")]
    /// Actual RRI algorithm/import source.
    Rri(RriOrigin),
    #[cfg(feature = "uv")]
    /// Actual UV sensor/estimate/import class.
    Uv(UvValueOrigin),
}
/// Actual authorization, independent of informational access/profile.
pub trait Access {
    /// Live module/action/concrete resource scope.
    fn allowed(&self, module: u8, operation: Operation, resource: Resource) -> bool;
    /// Live source-class scope; imported/estimated values are not promoted to sensor truth.
    fn source_allowed(&self, module: u8, origin: Origin) -> bool;
}
/// Real monotonic clock plus independently trustworthy UTC.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ClockReading {
    /// Actual monotonic microseconds used by the device-owned RRI tracker.
    pub monotonic_us: u64,
    /// Actual trustworthy UTC or explicit unknown, never phone request time.
    pub timestamp: Timestamp,
}
impl core::fmt::Debug for ClockReading {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ClockReading")
            .field("encoded_length", &17)
            .finish_non_exhaustive()
    }
}
/// Integration's real clock; absence is not timestamp zero.
pub trait Clock {
    /// Read actual clock sources.
    /// # Errors
    /// Real failure remains explicit.
    fn now(&self) -> Result<ClockReading, Failure>;
}
/// Module STATUS body independent of physiological data.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ModuleStatus {
    /// 0unavailable/1ready/2active policy or collection/3fault.
    pub state: u8,
    /// Actual reason.
    pub reason: u16,
}
impl core::fmt::Debug for ModuleStatus {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ModuleStatus")
            .field("encoded_length", &3)
            .finish_non_exhaustive()
    }
}
#[cfg(feature = "rri")]
/// Borrow of an already running/finished validated device-owned tracker; never creates acquisition.
#[derive(Clone, Copy)]
pub struct RriView<'a> {
    /// Nonzero actual lease/measurement generation, never reused.
    pub generation: u64,
    /// Same tracker used by device acquisition; pending candidate already passed `accept()`.
    pub tracker: &'a RriTracker,
    /// Actual measurement timestamp, not read time.
    pub timestamp: Timestamp,
    /// Actual source class.
    pub origin: RriOrigin,
    /// Actual selected provider identity.
    pub provider_id: &'a str,
    /// Actual selected provider version.
    pub provider_version: &'a str,
}
#[cfg(feature = "rri")]
impl core::fmt::Debug for RriView<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RriView")
            .field("provider_id_length", &self.provider_id.len())
            .field("provider_version_length", &self.provider_version.len())
            .finish_non_exhaustive()
    }
}
#[cfg(feature = "uv")]
/// Read-only facade implemented directly by the existing policy engine.
pub trait UvState {
    /// Actual latest or `NotFound`.
    /// # Errors
    /// Real absence/source failures remain explicit.
    fn latest(&self) -> Result<UvSample, Failure>;
    /// Actual retained E80, without generating a sample.
    fn pending_report(&self) -> Option<UvReportEvent>;
    /// Actual retained E81, respecting the engine's response barrier.
    fn pending_alarm(&self) -> Option<UvAlarmEvent>;
}
#[cfg(feature = "uv")]
impl<const N: usize> UvState for UvEngine<N> {
    fn latest(&self) -> Result<UvSample, Failure> {
        UvEngine::latest(self).map_err(|_| Failure::NotFound)
    }
    fn pending_report(&self) -> Option<UvReportEvent> {
        UvEngine::pending_report(self)
    }
    fn pending_alarm(&self) -> Option<UvAlarmEvent> {
        UvEngine::pending_alarm(self)
    }
}
#[cfg(feature = "uv")]
/// Pure preflight receipt, bound by the actual control port to one real state revision.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CancelTicket {
    /// Nonzero real source generation.
    pub generation: u64,
    /// Actual serialized state revision, nonzero and never reused for a different state.
    pub revision: u64,
    /// Original nonzero alarm ID.
    pub event_id: u32,
    /// True only for an actual retained known-cancelled ID; never creates a tombstone.
    pub already_cancelled: bool,
}
#[cfg(feature = "uv")]
impl core::fmt::Debug for CancelTicket {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CancelTicket").finish_non_exhaustive()
    }
}
#[cfg(feature = "uv")]
/// Exact typed UV event, not unvalidated bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UvEvent {
    /// E80 report21B.
    Report(UvReportEvent),
    /// E81 transition22B.
    Alarm(UvAlarmEvent),
}
#[cfg(feature = "uv")]
/// Product identity binding, stronger than the optional wire's per-boot uniqueness.
#[cfg(feature = "uv")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UvIdentityPolicy {
    /// No verified persistent identity guarantee; UV service cannot be constructed.
    Unverified,
    /// Actual persistent high-watermark/reserved ranges never reuse report/alarm IDs across
    /// normal reboot. Stop before exhaustion; identity ledger survives ordinary user-data
    /// clearing and factory reset. This is an actual port guarantee, not a source-generation flag.
    PersistentNonReuse,
}
#[cfg(feature = "uv")]
/// Real single-source control/read adapter. No production implementation is installed.
pub trait UvControlPort {
    /// Explicit actual cross-reboot identity guarantee; no successful default exists.
    fn identity_policy(&self) -> UvIdentityPolicy;
    /// Actual boot/source identity, nonzero and never reset within a live connection.
    fn generation(&self) -> u64;
    /// Actual provider provenance.
    fn provenance(&self) -> UvProvenance<'_>;
    /// Borrow the same device-owned policy engine used by sampling/alarm tasks.
    fn state(&self) -> &dyn UvState;
    /// Pure validation only: no cancel, reservations, silencing, engine mutation or tombstones.
    /// # Errors
    /// Unknown/already transitioned/busy/capacity state is explicit.
    fn validate_cancel(&self, event_id: u32) -> Result<CancelTicket, Failure>;
    /// Pure recheck against the same real serialized state source; no hidden mutation.
    fn ticket_current(&self, ticket: CancelTicket) -> bool;
    /// Called only after exact successful C30 response admission. Atomically recheck ticket,
    /// live Cancel authority, then actual silence plus same-engine `request_cancel` and
    /// `confirm_cancel_response`. Real operation must be idempotent for its ticket/connection.
    /// # Errors
    /// Revocation/conflict/actual unknown outcome must fail; caller ends the connection.
    fn cancel_atomic(
        &mut self,
        connection: u64,
        ticket: CancelTicket,
        when: ClockReading,
        access: &dyn Access,
    ) -> Result<(), Failure>;
    /// Confirm only an actual complete control-event admission; not peer persistence.
    /// # Errors
    /// Wrong/stale event or real failure retains the original event.
    fn event_admitted(&mut self, event: UvEvent) -> Result<(), Failure>;
}
/// Actual product ports. There are no default successful implementations.
pub trait Backend {
    /// Actual selected module port exists; compile selection alone is insufficient.
    fn selected(&self, module: u8) -> bool;
    /// Actual runtime status/policy state.
    /// # Errors
    /// Real backend errors remain explicit.
    fn status(&self, module: u8) -> Result<ModuleStatus, Failure>;
    #[cfg(feature = "rri")]
    /// Borrow an actual validated subscription to the device-owned tracker. Never start or reset it.
    /// # Errors
    /// No actual lease/source means `NotFound` or Unsupported; no synthetic tracker.
    fn rri_view(&self, connection: u64) -> Result<RriView<'_>, Failure>;
    #[cfg(feature = "rri")]
    /// Confirm actual App queue admission, distinct from tracker/device durable history consumption.
    /// The real adapter must preserve other device/history consumers; App detach must not stop sampling.
    /// # Errors
    /// Wrong generation/measurement/index or actual backend failure retains pending bytes.
    fn rri_event_admitted(
        &mut self,
        connection: u64,
        generation: u64,
        measurement: u64,
        index: u32,
    ) -> Result<(), Failure>;
    #[cfg(feature = "uv")]
    /// Borrow real policy/source/control adapter.
    /// # Errors
    /// Actual absence is explicit.
    fn uv(&self) -> Result<&dyn UvControlPort, Failure>;
    #[cfg(feature = "uv")]
    /// Mutably borrow the exact same control source.
    /// # Errors
    /// Actual absence is explicit.
    fn uv_mut(&mut self) -> Result<&mut dyn UvControlPort, Failure>;
    #[cfg(feature = "womens-health")]
    /// Actual committed configuration storage, never a predicted physiological state.
    /// # Errors
    /// Actual absence is explicit.
    fn womens(&self) -> Result<&dyn WomensBackend, Failure>;
    /// Detach App leases/pending subscribers only; never stop device-owned RRI/UV or silence alarms.
    /// Must be idempotent; failed cleanup blocks a replacement App owner until retried.
    /// # Errors
    /// Actual cleanup failures remain explicit.
    fn detach(&mut self, connection: u64) -> Result<(), Failure>;
}
/// Opaque event receipt; full identities are never truncated into transport IDs.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct EventToken {
    pub(crate) connection: u64,
    pub(crate) serial: u64,
    pub(crate) module: u8,
    pub(crate) opcode: u8,
}
impl core::fmt::Debug for EventToken {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("EventToken").finish_non_exhaustive()
    }
}
/// Exact ordinary-event routing metadata; no response prefix or CRC.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OfferedEvent {
    /// Receipt to confirm only after actual complete event admission.
    pub token: EventToken,
    /// Real module.
    pub module: u8,
    /// E80/E81.
    pub opcode: u8,
    /// Exact body length.
    pub length: usize,
}
/// Pure event routing metadata; inspecting it does not prepare, consume or acknowledge a source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventRoute {
    /// Actual module.
    pub module: u8,
    /// Actual E80/E81 opcode.
    pub opcode: u8,
    /// Exact whole logical event body length.
    pub length: usize,
}
