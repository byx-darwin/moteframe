use crate::Error;
use device_content::{AccessPort, Scope};
/// Trusted source phone and module/resource policy, supplied by the authenticated session layer.
/// Implementations must bind to the actual transport source and never trust peer credential fields.
pub trait ContentAccess: AccessPort {
    /// Current trusted authenticated owner/connection.
    /// # Errors
    /// Returns absent/revoked scope.
    fn scope(&self) -> Result<Scope, Error>;
    /// Nonzero authorization/provider generation; changes invalidate all retained responses/events.
    /// # Errors
    /// Returns unavailable or revoked authority.
    fn generation(&self) -> Result<u64, Error>;
    /// Check compiled module/opcode and, when present, exact action/notification/find identity.
    fn allowed(&self, module: u8, opcode: u8, id: Option<u32>) -> bool;
    /// Confirm actual committed 08.P0001 revision/generation in the same real property store.
    #[cfg(feature = "companion-radio")]
    fn radio_current(&self, revision: u32, generation: u64) -> bool;
    /// Independently authorize a committed phone-owned object hook, outside the command router.
    #[cfg(feature = "notifications")]
    fn notification_commit_allowed(&self, object_id: u32, notification: u32) -> bool;
    /// Check the exact actual committed notification generation; never accept a peer claim.
    #[cfg(feature = "notifications")]
    fn notification_current(&self, notification: u32, generation: u64) -> bool;
}
/// Actual external monotonic milliseconds; peer UTC is never substituted.
pub trait ContentClock: core::fmt::Debug {
    /// Read monotonic time.
    /// # Errors
    /// Returns absent/faulted clock.
    fn now_ms(&self) -> Result<u64, Error>;
}
/// Explicit real port bundle. Disabled features have no associated port or production fallback.
pub trait Backend: core::fmt::Debug {
    /// Real camera/music/SOS/status backend.
    #[cfg(any(feature = "phone", feature = "media"))]
    type Actions: device_content::ActionPort;
    /// Borrow actual action backend.
    #[cfg(any(feature = "phone", feature = "media"))]
    fn actions(&mut self) -> &mut Self::Actions;
    /// Real find backend with actual duration/pattern capabilities.
    #[cfg(feature = "find")]
    type Find: device_content::FindPort;
    /// Borrow actual find backend.
    #[cfg(feature = "find")]
    fn find(&mut self) -> &mut Self::Find;
    /// Real source-phone notification/UI backend.
    #[cfg(feature = "notifications")]
    type Notifications: device_content::NotificationPort;
    /// Borrow actual notification backend.
    #[cfg(feature = "notifications")]
    fn notifications(&mut self) -> &mut Self::Notifications;
    /// Real independent Classic/call-radio backend.
    #[cfg(feature = "companion-radio")]
    type Radio: ClassicPort;
    /// Borrow actual call-radio backend.
    #[cfg(feature = "companion-radio")]
    fn radio(&mut self) -> &mut Self::Radio;
    /// Actual configuration property store for independent radio readback.
    #[cfg(feature = "companion-radio")]
    type RadioState: RadioStateStore;
    /// Borrow the same actual readback store used by configuration properties.
    #[cfg(feature = "companion-radio")]
    fn radio_state(&mut self) -> &mut Self::RadioState;
    /// Real gimbal controller.
    #[cfg(feature = "gimbal")]
    type Gimbal: GimbalLifecyclePort;
    /// Borrow actual gimbal backend.
    #[cfg(feature = "gimbal")]
    fn gimbal(&mut self) -> &mut Self::Gimbal;
}
/// Actual committed property snapshot identity returned by the independent radio state store.
#[cfg(feature = "companion-radio")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PropertySnapshot {
    /// Actual nonzero property revision.
    pub revision: u32,
    /// Actual nonzero store generation; never reused while cached/event data can exist.
    pub generation: u64,
}
/// Actual property backend shared with the configuration handler's 08.P0001 readback.
#[cfg(feature = "companion-radio")]
pub trait RadioStateStore: core::fmt::Debug {
    /// Atomically publish actual validated radio observation, deduplicating unchanged observations.
    /// # Errors
    /// Returns actual storage/capacity failure; no fabricated revision is permitted.
    fn publish(
        &mut self,
        scope: Scope,
        value: device_content::ClassicBluetooth,
    ) -> Result<PropertySnapshot, Error>;
}
/// Real independent radio observation outside a pending `SET_ENABLED` transition.
/// This prevents a previously stable ticket snapshot from overwriting newer actual link state.
#[cfg(feature = "companion-radio")]
pub trait ClassicPort: device_content::CompanionRadioPort {
    /// Observe actual hardware/link state without fabricating a transition ticket.
    /// # Errors
    /// Returns missing hardware or actual observation failure.
    fn observe(
        &mut self,
        scope: Scope,
    ) -> Result<device_content::ClassicBluetooth, device_content::Error>;
}
/// Actual controller lifecycle beyond the frozen gimbal command/state DTO.
/// Cancellation is scope-specific and idempotent; actual drain is independently observed.
#[cfg(feature = "gimbal")]
pub trait GimbalLifecyclePort: device_content::GimbalPort {
    /// Nonblocking actual resource readiness for these fully decoded parameters; never actuates/reserves.
    /// # Errors
    /// Returns unavailable hardware or real prerequisite failure.
    fn ready(
        &self,
        scope: Scope,
        request: device_content::GimbalRequest,
    ) -> Result<bool, device_content::Error>;
    /// Request cancellation of every unconfirmed operation for this exact application scope,
    /// including a ticket whose observation failed or whose submit returned an uncertain error.
    /// # Errors
    /// Returns actual controller cancellation failure; success alone is not proof of drain.
    fn cancel_scope(&mut self, scope: Scope) -> Result<(), device_content::Error>;
    /// Observe actual completion/drain of all old-scope actuator operations.
    /// # Errors
    /// Returns actual observation failure, never a synthesized true value.
    fn drained(&self, scope: Scope) -> Result<bool, device_content::Error>;
}
