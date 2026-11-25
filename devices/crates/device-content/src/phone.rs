use crate::{
    AccessPort, Error, HeartbeatPolicy, Manifest, Operation, Property, Scope, encode_property,
};
/// Phone heartbeat monitoring state, derived only from externally supplied monotonic time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeartbeatState {
    /// Monitoring disabled.
    Disabled,
    /// Policy enabled but no actual heartbeat received.
    Waiting,
    /// Actual heartbeat still inside timeout.
    Healthy,
    /// Deadline passed but grace has not elapsed.
    Grace,
    /// Timeout and grace actually elapsed.
    TimedOut,
}
/// Pure phone heartbeat monitor; token echoes are not authentication or proof of network availability.
pub struct HeartbeatMonitor {
    scope: Scope,
    policy: HeartbeatPolicy,
    last_ms: u64,
    reference_ms: u64,
    received: bool,
}
impl HeartbeatMonitor {
    /// Empty disabled monitor for one authenticated scope.
    #[must_use]
    pub const fn new(scope: Scope) -> Self {
        Self {
            scope,
            policy: HeartbeatPolicy {
                enabled: false,
                timeout_s: 0,
                grace_s: 0,
            },
            last_ms: 0,
            reference_ms: 0,
            received: false,
        }
    }
    /// Replace validated monitor policy without performing a device action.
    /// # Errors
    /// Returns missing manifest bounds, invalid policy, clock reversal or denied source access.
    pub fn configure(
        &mut self,
        policy: HeartbeatPolicy,
        manifest: &Manifest<'_>,
        now_ms: u64,
        access: &impl AccessPort,
    ) -> Result<(), Error> {
        access.authorize(self.scope, Operation::Settle)?;
        let mut bytes = [0; 5];
        encode_property(Property::HeartbeatPolicy(policy), &mut bytes, manifest)?;
        self.clock(now_ms)?;
        self.policy = policy;
        self.reference_ms = now_ms;
        self.received = false;
        Ok(())
    }
    /// Accept a heartbeat token from the trusted session and return its exact value.
    /// # Errors
    /// Returns denied access or reversed monotonic time. Zero tokens are permitted and never confer authority.
    pub fn heartbeat(
        &mut self,
        token: u32,
        now_ms: u64,
        access: &impl AccessPort,
    ) -> Result<u32, Error> {
        access.authorize(self.scope, Operation::Settle)?;
        self.clock(now_ms)?;
        self.reference_ms = now_ms;
        self.received = true;
        Ok(token)
    }
    /// Decode and echo exact 07.C30's four bytes, without adding credentials or an undocumented body.
    /// # Errors
    /// Returns malformed input, short output, denied access or reversed time. Short output never refreshes monitoring.
    pub fn heartbeat_bytes(
        &mut self,
        request: &[u8],
        out: &mut [u8],
        now_ms: u64,
        access: &impl AccessPort,
    ) -> Result<usize, Error> {
        let token = u32::from_le_bytes(request.try_into().map_err(|_| Error::Invalid)?);
        let dest = out.get_mut(..4).ok_or(Error::Capacity)?;
        let reply = self.heartbeat(token, now_ms, access)?;
        dest.copy_from_slice(&reply.to_le_bytes());
        Ok(4)
    }
    /// Compute timing state from the actual last heartbeat or policy activation time.
    /// # Errors
    /// Returns `State` when external monotonic time reverses.
    pub fn poll(&mut self, now_ms: u64) -> Result<HeartbeatState, Error> {
        self.clock(now_ms)?;
        if !self.policy.enabled {
            return Ok(HeartbeatState::Disabled);
        }
        let elapsed = now_ms.checked_sub(self.reference_ms).ok_or(Error::State)?;
        let timeout = u64::from(self.policy.timeout_s) * 1000;
        let grace = u64::from(self.policy.grace_s) * 1000;
        if elapsed >= timeout + grace {
            Ok(HeartbeatState::TimedOut)
        } else if elapsed >= timeout {
            Ok(HeartbeatState::Grace)
        } else if self.received {
            Ok(HeartbeatState::Healthy)
        } else {
            Ok(HeartbeatState::Waiting)
        }
    }
    /// Clear monitor policy and all cached liveness when authorization, connection or CCCD ends.
    pub fn invalidate(&mut self) {
        self.policy.enabled = false;
        self.reference_ms = 0;
        self.received = false;
    }
    /// Replace the authenticated owner/connection, clearing old liveness.
    /// # Errors
    /// Rejects same-scope reset.
    pub fn replace_scope(&mut self, scope: Scope) -> Result<(), Error> {
        if scope == self.scope {
            return Err(Error::Conflict);
        }
        self.invalidate();
        self.scope = scope;
        self.last_ms = 0;
        Ok(())
    }
    fn clock(&mut self, now_ms: u64) -> Result<(), Error> {
        if now_ms < self.last_ms {
            return Err(Error::State);
        }
        self.last_ms = now_ms;
        Ok(())
    }
}
impl core::fmt::Debug for HeartbeatMonitor {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HeartbeatMonitor")
            .field("length", &usize::from(self.received))
            .finish_non_exhaustive()
    }
}
