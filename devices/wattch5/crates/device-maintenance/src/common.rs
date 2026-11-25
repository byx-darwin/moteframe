//! Trusted integration policy and local engine errors.
/// Build policy, supplied by integration rather than request bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    /// Commercial builds cannot construct maintenance engines.
    Commercial,
    /// Development build.
    Development,
    /// Factory build.
    Factory,
}
/// Independently granted station scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    /// Raw acquisition.
    Capture,
    /// Self-test operations and result reading.
    SelfTest,
    /// Destructive self-test additionally needs this scope.
    DangerousSelfTest,
    /// Trusted station external evidence submission.
    ExternalEvidence,
    /// Versioned engineering named operations.
    Engineering,
    /// Versioned factory configuration operations.
    FactoryConfig,
    /// Independently granted persistent dangerous-write scope.
    DangerousMaintenance,
}
/// Live trusted authorization, never a payload-supplied role.
pub trait Authority {
    /// Recheck current station permission.
    fn allows(&self, capability: Capability) -> bool;
    /// Current authenticated station identity; none means no station identity.
    fn station_id(&self) -> Option<u32> {
        None
    }
}
/// Local service errors; map to frozen wire status in an authorized router.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Maintenance unavailable in commercial builds.
    Commercial,
    /// Required backend is missing or unsupported.
    Unsupported,
    /// Real execution, durable ledger or reboot-recovery prerequisite is unavailable.
    Unavailable,
    /// Trusted scope is absent/revoked.
    Denied,
    /// Invalid fields or backend descriptor.
    Invalid,
    /// Conflicting active work or shared DATA lane.
    Busy,
    /// Unknown/evicted identity.
    NotFound,
    /// Binding or immutable terminal outcome conflicts.
    Conflict,
    /// Fixed buffer/counter/identifier capacity exhausted.
    Capacity,
    /// Wrong lifecycle.
    InvalidState,
    /// Backend operation failed; no success is implied.
    Backend,
    /// No whole-message credit remains.
    NoCredit,
    /// Wrong stream/message identity.
    Identity,
    /// Session has ended.
    Closed,
    /// Product pass criterion is missing or incomplete.
    CriterionUndefined,
    /// Monotonic time regressed/overflowed.
    Clock,
    /// STOP drain deadline expired; close the connection.
    Timeout,
}
