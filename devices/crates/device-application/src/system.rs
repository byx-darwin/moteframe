//! System commands accept a delay before scheduling; no action is replayed after reconnect.
use device_actions::{Action, ActionError, DeferredSystem, SystemBackend, SystemScope};
use device_protocol::{Frame, Header, Kind, ModuleEntry};
use device_services::{Reason, ServiceHandler, SessionError, Status};
/// Trusted device-control scope. Requests and build profiles never grant it.
pub trait SystemAccess {
    /// Whether the current actor can execute this exact opcode30..32.
    fn allowed(&self, opcode: u8) -> bool;
}
/// Actual system/persistent-name backend failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemFailure {
    /// No backend/default name exists.
    Unsupported,
    /// Scope is absent or has been revoked.
    Denied,
    /// Real resource/timer is in use.
    Busy,
    /// Capacity is exhausted.
    Capacity,
    /// Persistent name update failed.
    Storage,
    /// Physical action failed or its outcome is uncertain.
    Hardware,
}
/// Actual board timer and trusted default-name store. No successful default implementation exists.
pub trait SystemPort: SystemBackend {
    /// Verify actual scheduling capability/default-name availability without executing an action.
    /// # Errors
    /// An unbound backend must return Unsupported, never accept a simulated action.
    fn available(&self, action: Action) -> Result<(), SystemFailure>;
    /// Restore the actual manifest default name atomically and durably before returning success.
    /// Recheck scope inside the same commit boundary; publish E90 only for the actual committed revision.
    /// # Errors
    /// Missing manifest default, capacity, authorization or storage failure must be explicit.
    fn restore_default_name(&mut self, access: &dyn SystemScope) -> Result<(), SystemFailure>;
    /// Discard connection-owned pending timers and release real resources. Must be retryable after error.
    /// Timer execution must independently recheck the authenticated lease; delay does not extend scope.
    /// # Errors
    /// Actual/uncertain cleanup failure remains explicit, even though the session is closed.
    fn connection_closed(&mut self) -> Result<(), SystemFailure>;
}
struct Scope<'a> {
    access: &'a dyn SystemAccess,
    opcode: u8,
}
impl SystemScope for Scope<'_> {
    fn allowed(&self) -> bool {
        self.access.allowed(self.opcode)
    }
}
#[derive(Clone, Copy)]
struct Pending {
    header: Header,
    action: Action,
    ticket: u32,
}
/// Module02 C30/31/32 adapter. Session remains the sole request-ID/replay authority.
pub struct SystemHandler<'a, P: SystemPort> {
    metadata: ModuleEntry,
    access: &'a dyn SystemAccess,
    port: &'a mut P,
    deferred: DeferredSystem,
    pending: Option<Pending>,
    admitted: Option<Header>,
    used: u8,
    closed: bool,
    cleanup_done: bool,
}
impl<P: SystemPort> core::fmt::Debug for SystemHandler<'_, P> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SystemHandler")
            .field("pending", &self.pending.is_some())
            .field("closed", &self.closed)
            .finish_non_exhaustive()
    }
}
impl<'a, P: SystemPort> SystemHandler<'a, P> {
    /// Bind real resources and a connection-static module02 capability entry.
    /// # Errors
    /// Rejects invalid metadata; constructing this handler does not advertise any backend.
    pub fn new(
        metadata: ModuleEntry,
        access: &'a dyn SystemAccess,
        port: &'a mut P,
    ) -> Result<Self, SessionError> {
        if metadata.module != 2
            || (metadata.major, metadata.minor) != (1, 0)
            || metadata.availability != 1
            || metadata.features != 0
            || metadata.input_max < 4
            || metadata.output_max < 4
        {
            return Err(SessionError::InvalidHandlerResponse);
        }
        Ok(Self {
            metadata,
            access,
            port,
            deferred: DeferredSystem::new(),
            pending: None,
            admitted: None,
            used: 0,
            closed: false,
            cleanup_done: false,
        })
    }
    fn revoked(&self) -> bool {
        (0..3).any(|i| self.used & (1 << i) != 0 && !self.access.allowed(0x30 + i))
    }
    fn fail(out: &mut [u8], status: Status, reason: Reason) -> Result<usize, SessionError> {
        if out.len() < 4 {
            return Err(SessionError::Capacity);
        }
        out[..2].copy_from_slice(&(status as u16).to_le_bytes());
        out[2..4].copy_from_slice(&(reason as u16).to_le_bytes());
        Ok(4)
    }
    fn failure(out: &mut [u8], error: SystemFailure) -> Result<usize, SessionError> {
        let (status, reason) = match error {
            SystemFailure::Unsupported => (Status::UnsupportedOpcode, Reason::None),
            SystemFailure::Denied => (Status::AccessDenied, Reason::RoleRequired),
            SystemFailure::Busy => (Status::Busy, Reason::None),
            SystemFailure::Capacity => (Status::LimitExceeded, Reason::None),
            SystemFailure::Storage => (Status::StorageFailure, Reason::None),
            SystemFailure::Hardware => (Status::HardwareFailure, Reason::None),
        };
        Self::fail(out, status, reason)
    }
}
impl<P: SystemPort> ServiceHandler for SystemHandler<'_, P> {
    fn required_features(&self, module: &ModuleEntry, opcode: u8) -> Option<u32> {
        (*module == self.metadata && matches!(opcode, 0x30..=0x32)).then_some(0)
    }
    fn authorized(&self, request: Frame<'_>) -> bool {
        !self.must_end_session()
            && request.header.module == 2
            && self.access.allowed(request.header.opcode)
    }
    fn must_end_session(&self) -> bool {
        self.closed || self.revoked()
    }
    fn dispatch(&mut self, request: Frame<'_>, out: &mut [u8]) -> Result<usize, SessionError> {
        if out.len() < 4 {
            return Err(SessionError::Capacity);
        }
        if self.must_end_session() {
            out.fill(0);
            return Err(SessionError::AuthorizationRevoked);
        }
        if request.header.kind != Kind::Request
            || request.header.module != 2
            || request.header.offset != 0
            || request.header.message_id == 0
            || usize::from(request.header.total_length) != request.payload.len()
        {
            return Err(SessionError::InvalidFrame);
        }
        if !self.access.allowed(request.header.opcode) {
            return Self::failure(out, SystemFailure::Denied);
        }
        let action = match Action::decode(2, request.header.opcode, request.payload) {
            Ok(a) => a,
            Err(ActionError::Unsupported) if !matches!(request.header.opcode, 0x30..=0x32) => {
                return Self::failure(out, SystemFailure::Unsupported);
            }
            Err(_) => return Self::fail(out, Status::InvalidArgument, Reason::None),
        };
        if let Some(pending) = self.pending {
            return if pending.header == request.header && pending.action == action {
                Self::fail(out, Status::Ok, Reason::None)
            } else {
                Self::failure(out, SystemFailure::Busy)
            };
        }
        if let Err(error) = self.port.available(action) {
            return Self::failure(out, error);
        }
        if !self.access.allowed(request.header.opcode) {
            self.closed = true;
            out.fill(0);
            return Err(SessionError::AuthorizationRevoked);
        }
        let scope = Scope {
            access: self.access,
            opcode: request.header.opcode,
        };
        if action == Action::RestoreName {
            if let Err(error) = self.port.restore_default_name(&scope) {
                return Self::failure(out, error);
            }
            self.admitted = Some(request.header);
        } else {
            let ticket = match self.deferred.prepare(action, &scope) {
                Ok(t) => t,
                Err(ActionError::Denied) => return Self::failure(out, SystemFailure::Denied),
                Err(ActionError::Busy) => return Self::failure(out, SystemFailure::Busy),
                Err(ActionError::Capacity) => return Self::failure(out, SystemFailure::Capacity),
                Err(_) => return Self::fail(out, Status::InvalidArgument, Reason::None),
            };
            self.pending = Some(Pending {
                header: request.header,
                action,
                ticket,
            });
        }
        self.used |= 1 << (request.header.opcode - 0x30);
        if self.revoked() {
            self.closed = true;
            out.fill(0);
            return Err(SessionError::AuthorizationRevoked);
        }
        Self::fail(out, Status::Ok, Reason::None)
    }
    fn response_admitted(
        &mut self,
        request: Frame<'_>,
        response: &[u8],
    ) -> Result<(), SessionError> {
        if response != [0; 4] {
            return Err(SessionError::InvalidHandlerResponse);
        }
        if self.must_end_session() {
            self.deferred.disconnect();
            self.pending = None;
            self.closed = true;
            return Err(SessionError::AuthorizationRevoked);
        }
        if self.admitted == Some(request.header) {
            return Ok(());
        }
        let pending = self
            .pending
            .filter(|p| p.header == request.header)
            .ok_or(SessionError::InvalidHandlerResponse)?;
        let action = Action::decode(2, request.header.opcode, request.payload)
            .map_err(|_| SessionError::InvalidHandlerResponse)?;
        if pending.action != action {
            return Err(SessionError::InvalidHandlerResponse);
        }
        let scope = Scope {
            access: self.access,
            opcode: request.header.opcode,
        };
        self.pending = None;
        if self
            .deferred
            .confirm_response(pending.ticket, &scope, self.port)
            .is_err()
        {
            self.closed = true;
            return Err(SessionError::InvalidHandlerResponse);
        }
        self.admitted = Some(request.header);
        Ok(())
    }
    fn connection_closed(&mut self) -> Result<(), SessionError> {
        self.closed = true;
        self.pending = None;
        self.deferred.disconnect();
        if !self.cleanup_done {
            self.port
                .connection_closed()
                .map_err(|_| SessionError::InvalidHandlerResponse)?;
            self.cleanup_done = true;
        }
        Ok(())
    }
}
