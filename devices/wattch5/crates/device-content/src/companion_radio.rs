use crate::{AccessPort, BackendTicket, ClassicBluetooth, Error, Operation, Scope};
/// Independent call-radio transition lifecycle; unrelated to BLE connection state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompanionRadioState {
    /// No actual backend state has been read yet.
    Uninitialized,
    /// Waiting for successful response admission.
    Prepared,
    /// Actual backend ticket is pending.
    Switching,
    /// Backend confirmed desired stable state.
    Stable,
    /// Actual backend failure.
    Failed,
    /// Source scope ended before completion was confirmed.
    Unknown,
}
/// Actual independent Classic/call-radio backend.
pub trait CompanionRadioPort: core::fmt::Debug {
    /// Whether an actual independent call-radio backend is selected.
    fn available(&self) -> bool;
    /// Submit an admitted desired switch and return its actual ticket.
    /// # Errors
    /// Returns real backend failure or unavailable hardware.
    fn set_enabled(&mut self, scope: Scope, enabled: bool) -> Result<BackendTicket, Error>;
    /// Read actual state from the ticket; never synthesize it from BLE.
    /// # Errors
    /// Returns actual readback/backend failure.
    fn poll(&mut self, scope: Scope, ticket: BackendTicket) -> Result<ClassicBluetooth, Error>;
    /// Cancel a pending transition during scope cleanup; does not itself prove the radio is disabled.
    /// # Errors
    /// Returns actual cancellation failure.
    fn cancel(&mut self, scope: Scope, ticket: BackendTicket) -> Result<(), Error>;
}
/// Response-first state machine for 08.C30.
pub struct CompanionRadioService {
    scope: Scope,
    desired: Option<bool>,
    ticket: Option<BackendTicket>,
    state: CompanionRadioState,
    readback: Option<ClassicBluetooth>,
}
impl core::fmt::Debug for CompanionRadioService {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CompanionRadioService")
            .field("readback_length", &usize::from(self.readback.is_some()))
            .finish_non_exhaustive()
    }
}
impl CompanionRadioService {
    /// Empty authenticated source scope, without a fabricated radio readback.
    #[must_use]
    pub const fn new(scope: Scope) -> Self {
        Self {
            scope,
            desired: None,
            ticket: None,
            state: CompanionRadioState::Uninitialized,
            readback: None,
        }
    }
    /// Accept a desired state without changing real hardware.
    /// # Errors
    /// Returns denied access, missing independent backend or reversal conflict during transition.
    pub fn prepare(
        &mut self,
        enabled: bool,
        access: &impl AccessPort,
        port: &impl CompanionRadioPort,
    ) -> Result<CompanionRadioState, Error> {
        access.authorize(self.scope, Operation::Submit)?;
        if !port.available() {
            return Err(Error::Unavailable);
        }
        if self.desired == Some(enabled)
            && matches!(
                self.state,
                CompanionRadioState::Prepared
                    | CompanionRadioState::Switching
                    | CompanionRadioState::Stable
            )
        {
            return Ok(self.state);
        }
        if matches!(
            self.state,
            CompanionRadioState::Prepared | CompanionRadioState::Switching
        ) {
            return Err(Error::Conflict);
        }
        self.desired = Some(enabled);
        self.ticket = None;
        self.state = CompanionRadioState::Prepared;
        Ok(self.state)
    }
    /// Parse the exact one-byte C30 request and reserve it.
    /// # Errors
    /// Rejects malformed/unknown boolean, denied access, missing backend or reversal conflict.
    pub fn prepare_bytes(
        &mut self,
        bytes: &[u8],
        access: &impl AccessPort,
        port: &impl CompanionRadioPort,
    ) -> Result<CompanionRadioState, Error> {
        let enabled = match bytes {
            [0] => false,
            [1] => true,
            _ => return Err(Error::Invalid),
        };
        self.prepare(enabled, access, port)
    }
    /// Perform the actual switch after complete successful response admission.
    /// # Errors
    /// Returns denied access, missing backend, invalid ordering or actual switch failure.
    pub fn response_admitted(
        &mut self,
        access: &impl AccessPort,
        port: &mut impl CompanionRadioPort,
    ) -> Result<CompanionRadioState, Error> {
        access.authorize(self.scope, Operation::Submit)?;
        if self.state != CompanionRadioState::Prepared {
            return Ok(self.state);
        }
        if !port.available() {
            return Err(Error::Unavailable);
        }
        let desired = self.desired.ok_or(Error::State)?;
        self.state = CompanionRadioState::Unknown;
        match port.set_enabled(self.scope, desired) {
            Ok(ticket) => {
                self.ticket = Some(ticket);
                self.state = CompanionRadioState::Switching;
                Ok(self.state)
            }
            Err(error) => {
                self.state = CompanionRadioState::Failed;
                Err(error)
            }
        }
    }
    /// Observe actual independent radio state; only real stable readback confirms completion.
    /// # Errors
    /// Returns denial, invalid call order/readback, absent real backend or observation failure.
    pub fn poll(
        &mut self,
        access: &impl AccessPort,
        port: &mut impl CompanionRadioPort,
    ) -> Result<CompanionRadioState, Error> {
        access.authorize(self.scope, Operation::Submit)?;
        if self.state != CompanionRadioState::Switching {
            return Ok(self.state);
        }
        if !port.available() {
            return Err(Error::Unavailable);
        }
        let ticket = self.ticket.ok_or(Error::State)?;
        let value = match port.poll(self.scope, ticket) {
            Ok(value) => value,
            Err(error) => {
                self.state = CompanionRadioState::Failed;
                return Err(error);
            }
        };
        if value.state > 6
            || (!value.valid && value.address != [0; 6])
            || Some(value.enabled) != self.desired
        {
            self.state = CompanionRadioState::Failed;
            return Err(Error::Invalid);
        }
        self.readback = Some(value);
        if value.state == 6 {
            self.state = CompanionRadioState::Failed;
        } else if (value.enabled && matches!(value.state, 3 | 4))
            || (!value.enabled && value.state == 1)
        {
            self.state = CompanionRadioState::Stable;
        }
        Ok(self.state)
    }
    /// Read only actually observed P0001 content; no all-zero synthetic state is returned.
    /// # Errors
    /// Returns `NotFound` before an actual observation or after scope invalidation.
    pub fn readback(&self) -> Result<ClassicBluetooth, Error> {
        self.readback.ok_or(Error::NotFound)
    }
    /// Current lifecycle; acceptance is separate from connection or audio readiness.
    #[must_use]
    pub const fn state(&self) -> CompanionRadioState {
        self.state
    }
    /// Revoke pending source scope/CCCD work and request cancellation without claiming physical completion.
    /// # Errors
    /// Returns actual backend cleanup failure; pending state becomes Unknown regardless.
    pub fn invalidate(
        &mut self,
        port: &mut impl CompanionRadioPort,
    ) -> Result<CompanionRadioState, Error> {
        let pending = matches!(
            self.state,
            CompanionRadioState::Prepared | CompanionRadioState::Switching
        );
        let ticket = self.ticket.take();
        self.readback = None;
        self.desired = None;
        if pending {
            self.state = CompanionRadioState::Unknown;
            if let Some(ticket) = ticket {
                port.cancel(self.scope, ticket)?;
            }
        } else {
            self.state = CompanionRadioState::Uninitialized;
        }
        Ok(self.state)
    }
    /// Replace owner/connection after explicit cleanup.
    /// # Errors
    /// Rejects same scope or unresolved transition.
    pub fn replace_scope(&mut self, scope: Scope) -> Result<(), Error> {
        if scope == self.scope {
            return Err(Error::Conflict);
        }
        if matches!(
            self.state,
            CompanionRadioState::Prepared | CompanionRadioState::Switching
        ) {
            return Err(Error::State);
        }
        self.scope = scope;
        self.desired = None;
        self.ticket = None;
        self.readback = None;
        self.state = CompanionRadioState::Uninitialized;
        Ok(())
    }
}
