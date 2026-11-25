//! Composition of real DATA producers and persistent identity; no generated dataset fallback.
use device_protocol::DataKind;
use device_services::{
    AccessSnapshot, CoreRequest, CoreResources, DatasetIdentity, GrantRequest, ResourceFailure,
    TrustedAccessProvider,
};
/// A current connection's actual cumulative whole-message credit owner.
pub trait CreditPort {
    /// Live resource scope, independent of reported ACCESS state and stream validity.
    fn authorized(&self) -> bool;
    /// Check current stream generation, phase and monotonic cumulative credit without mutation.
    /// # Errors
    /// Missing stream, drain, scope or credit conflict is explicit.
    fn validate(&self, stream: u16, total: u32) -> Result<(), ResourceFailure>;
    /// Serialized grant boundary must recheck scope/context before committing credit.
    /// # Errors
    /// An error must preserve previous credit; no stream may be allocated here.
    fn grant(&mut self, stream: u16, total: u32) -> Result<u32, ResourceFailure>;
    /// Verify an accepted historical credit response still names the same live stream generation.
    fn replay_valid(&self, stream: u16, accepted: u32) -> bool;
}
/// Actual persistent dataset identity owner. Implementations must not synthesize a connection UUID.
pub trait DatasetPort {
    /// Current identity scope; informational Authorized alone is insufficient.
    fn authorized(&self) -> bool;
    /// Read and verify actual storage identity under the same live identity scope.
    /// # Errors
    /// Missing, stale, corrupt or inaccessible identity must be an error.
    fn identity(&self) -> Result<DatasetIdentity, ResourceFailure>;
}
/// Core00.07/08 bridge to the same owners used by history/raw business services.
/// Board integration serializes calls and notification admission with authorization revocation.
pub struct ResourceRouter<'a> {
    info: &'a dyn TrustedAccessProvider,
    history: &'a mut dyn CreditPort,
    dataset: &'a mut dyn DatasetPort,
    #[cfg(feature = "raw-data")]
    raw: Option<&'a mut dyn CreditPort>,
}
impl core::fmt::Debug for ResourceRouter<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ResourceRouter").finish_non_exhaustive()
    }
}
impl<'a> ResourceRouter<'a> {
    /// Bind actual owners; this does not advertise capabilities or confer permissions.
    pub fn new(
        info: &'a dyn TrustedAccessProvider,
        history: &'a mut dyn CreditPort,
        dataset: &'a mut dyn DatasetPort,
    ) -> Self {
        Self {
            info,
            history,
            dataset,
            #[cfg(feature = "raw-data")]
            raw: None,
        }
    }
    /// Bind an explicitly compiled development raw producer to the shared DATA lane.
    #[cfg(feature = "raw-data")]
    #[must_use]
    pub fn with_raw(mut self, raw: &'a mut dyn CreditPort) -> Self {
        self.raw = Some(raw);
        self
    }
    fn credit(&self, kind: DataKind) -> Option<&dyn CreditPort> {
        match kind {
            DataKind::History => Some(&*self.history),
            DataKind::Raw => {
                #[cfg(feature = "raw-data")]
                {
                    self.raw.as_deref()
                }
                #[cfg(not(feature = "raw-data"))]
                {
                    None
                }
            }
        }
    }
    fn credit_mut(&mut self, kind: DataKind) -> Option<&mut (dyn CreditPort + 'a)> {
        match kind {
            DataKind::History => Some(&mut *self.history),
            DataKind::Raw => {
                #[cfg(feature = "raw-data")]
                {
                    self.raw.as_deref_mut()
                }
                #[cfg(not(feature = "raw-data"))]
                {
                    None
                }
            }
        }
    }
}
impl TrustedAccessProvider for ResourceRouter<'_> {
    fn access_snapshot(&self) -> AccessSnapshot {
        self.info.access_snapshot()
    }
}
impl CoreResources for ResourceRouter<'_> {
    fn authorized(&self, request: CoreRequest) -> bool {
        match request {
            CoreRequest::Dataset => self.dataset.authorized(),
            CoreRequest::Grant(g) => self.credit(g.kind).is_some_and(CreditPort::authorized),
        }
    }
    fn validate(&self, request: CoreRequest) -> Result<(), ResourceFailure> {
        if !self.authorized(request) {
            return Err(ResourceFailure::AccessDenied);
        }
        match request {
            CoreRequest::Dataset => self.dataset.identity().map(|_| ()),
            CoreRequest::Grant(g) => self
                .credit(g.kind)
                .ok_or(ResourceFailure::Unsupported)?
                .validate(g.stream_id, g.cumulative),
        }
    }
    fn grant(&mut self, g: GrantRequest) -> Result<u32, ResourceFailure> {
        if !self.authorized(CoreRequest::Grant(g)) {
            return Err(ResourceFailure::AccessDenied);
        }
        self.credit_mut(g.kind)
            .ok_or(ResourceFailure::Unsupported)?
            .grant(g.stream_id, g.cumulative)
    }
    fn dataset(&mut self) -> Result<DatasetIdentity, ResourceFailure> {
        if !self.dataset.authorized() {
            return Err(ResourceFailure::AccessDenied);
        }
        self.dataset.identity()
    }
    fn replay_valid(&self, request: CoreRequest, body: &[u8]) -> bool {
        if !self.authorized(request) {
            return false;
        }
        match request {
            CoreRequest::Dataset => self.dataset.identity().is_ok_and(|id| id.bytes() == body),
            CoreRequest::Grant(g) => {
                let kind = match g.kind {
                    DataKind::History => 1,
                    DataKind::Raw => 2,
                };
                if body.len() != 7 || body[0] != kind || body[1..3] != g.stream_id.to_le_bytes() {
                    return false;
                }
                let total = u32::from_le_bytes([body[3], body[4], body[5], body[6]]);
                total <= g.cumulative
                    && self
                        .credit(g.kind)
                        .is_some_and(|p| p.replay_valid(g.stream_id, total))
            }
        }
    }
}
