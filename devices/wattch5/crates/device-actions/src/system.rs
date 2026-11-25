//! System actions are not scheduled until the successful response was transport-admitted.
use crate::{Action, ActionError};
/// Trusted current system-control scope; never supplied by an unauthenticated request.
pub trait SystemScope {
    /// Whether the current principal may schedule system actions.
    fn allowed(&self) -> bool;
}
/// Hardware-independent delayed-action port. There is no production simulation backend.
pub trait SystemBackend {
    /// Schedule a reboot or poweroff after the admitted response and specified delay.
    /// # Errors
    /// Backend failure or unknown scheduling outcome. Never report success without accepting the action.
    /// The backend must recheck its authenticated principal lease when the timer actually executes;
    /// admission here does not make authorization permanent during the delay.
    fn schedule(&mut self, reboot: bool, delay_s: u32) -> Result<(), ActionError>;
}
/// One deferred system action and a connection-local, nonreusing ticket allocator.
#[derive(Debug)]
pub struct DeferredSystem {
    pending: Option<(u32, bool, u32)>,
    next: u32,
}
impl Default for DeferredSystem {
    fn default() -> Self {
        Self::new()
    }
}
impl DeferredSystem {
    /// Construct idle response-admission state.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            pending: None,
            next: 1,
        }
    }
    /// Reserve a delayed action without calling its backend. Returns a private admission ticket.
    /// # Errors
    /// Returns denied, busy, unsupported action or exhausted tickets.
    pub fn prepare(
        &mut self,
        action: Action,
        scope: &impl SystemScope,
    ) -> Result<u32, ActionError> {
        if !scope.allowed() {
            self.pending = None;
            return Err(ActionError::Denied);
        }
        if self.pending.is_some() {
            return Err(ActionError::Busy);
        }
        let (reboot, delay) = match action {
            Action::Reboot { delay_s } => (true, delay_s),
            Action::Poweroff { delay_s } => (false, delay_s),
            Action::RestoreName => return Err(ActionError::Unsupported),
            #[cfg(any(
                feature = "find",
                feature = "phone",
                feature = "call",
                feature = "media",
                feature = "notification-actions"
            ))]
            _ => return Err(ActionError::Unsupported),
        };
        if !(1..=3600).contains(&delay) {
            return Err(ActionError::Invalid);
        }
        let id = self.next;
        let next = id.checked_add(1).ok_or(ActionError::Capacity)?;
        self.next = next;
        self.pending = Some((id, reboot, delay));
        Ok(id)
    }
    /// Schedule exactly once after the adapter admits the complete successful response.
    /// # Errors
    /// Rejects stale ticket, fresh scope revocation, or backend failure. Every attempt consumes pending state.
    pub fn confirm_response(
        &mut self,
        ticket: u32,
        scope: &impl SystemScope,
        backend: &mut impl SystemBackend,
    ) -> Result<(), ActionError> {
        let (id, reboot, delay) = self.pending.ok_or(ActionError::InvalidState)?;
        if id != ticket {
            return Err(ActionError::InvalidState);
        }
        self.pending = None;
        if !scope.allowed() {
            return Err(ActionError::Denied);
        }
        backend.schedule(reboot, delay)
    }
    /// Discard a failed response's action. No action is scheduled.
    /// # Errors
    /// Returns invalid state for a missing or different ticket.
    pub fn response_failed(&mut self, ticket: u32) -> Result<(), ActionError> {
        if self.pending.is_none_or(|p| p.0 != ticket) {
            return Err(ActionError::InvalidState);
        }
        self.pending = None;
        Ok(())
    }
    /// Discard pending actions immediately at disconnect/revocation.
    pub fn disconnect(&mut self) {
        self.pending = None;
    }
}
