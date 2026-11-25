//! Checked sharing of a single application service on a serialized foreground executor.
use core::cell::RefCell;
use device_protocol::{Frame, ModuleEntry};
use device_services::{AccessSnapshot, ServiceHandler, SessionError};
/// The service is already borrowed; no panic, heap allocation or recursive dispatch occurs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellBusy;
/// One owner, shared by control routing and actual DATA/resource adapters.
/// This type is deliberately not Sync. The board executor must serialize ISR access externally.
pub struct ServiceCell<T> {
    inner: RefCell<T>,
}
impl<T> core::fmt::Debug for ServiceCell<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ServiceCell").finish_non_exhaustive()
    }
}
impl<T> ServiceCell<T> {
    /// Own the service exactly once. Construct a new cell only for genuinely new connection work.
    pub const fn new(value: T) -> Self {
        Self {
            inner: RefCell::new(value),
        }
    }
    /// Inspect state without allowing a reference to escape the checked borrow.
    /// # Errors
    /// Reentrant mutable access returns `CellBusy`.
    pub fn with<R>(&self, inspect: impl FnOnce(&T) -> R) -> Result<R, CellBusy> {
        let owner = self.inner.try_borrow().map_err(|_| CellBusy)?;
        Ok(inspect(&owner))
    }
    /// Execute one serialized service operation.
    /// # Errors
    /// Any outstanding borrow returns `CellBusy` before the operation runs.
    pub fn with_mut<R>(&self, operation: impl FnOnce(&mut T) -> R) -> Result<R, CellBusy> {
        let mut owner = self.inner.try_borrow_mut().map_err(|_| CellBusy)?;
        Ok(operation(&mut owner))
    }
}
impl<T: ServiceHandler> ServiceCell<T> {
    /// Borrow a router adapter; no secondary request cache or service owner is created.
    pub const fn handle(&self) -> ServiceHandle<'_, T> {
        ServiceHandle { owner: self }
    }
}
/// Borrowed service route that forwards all lifecycle hooks to its single checked owner.
/// Reentrant calls fail closed; applications must not hold a cell borrow while routing a request.
pub struct ServiceHandle<'a, T: ServiceHandler> {
    owner: &'a ServiceCell<T>,
}
impl<T: ServiceHandler> core::fmt::Debug for ServiceHandle<'_, T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ServiceHandle").finish_non_exhaustive()
    }
}
impl<T: ServiceHandler> ServiceHandler for ServiceHandle<'_, T> {
    fn advance(&mut self, now_ms: u64) -> Result<(), SessionError> {
        self.owner
            .with_mut(|handler| handler.advance(now_ms))
            .map_err(|_| SessionError::InvalidHandlerResponse)?
    }
    fn informational_access(&self) -> Option<AccessSnapshot> {
        self.owner
            .with(ServiceHandler::informational_access)
            .ok()
            .flatten()
    }
    fn required_features(&self, module: &ModuleEntry, opcode: u8) -> Option<u32> {
        self.owner
            .with(|h| h.required_features(module, opcode))
            .ok()
            .flatten()
    }
    fn authorized(&self, request: Frame<'_>) -> bool {
        self.owner.with(|h| h.authorized(request)).unwrap_or(false)
    }
    fn replay_valid(&self, request: Frame<'_>, response: &[u8]) -> bool {
        self.owner
            .with(|h| h.replay_valid(request, response))
            .unwrap_or(false)
    }
    fn must_end_session(&self) -> bool {
        self.owner
            .with(ServiceHandler::must_end_session)
            .unwrap_or(true)
    }
    fn response_ready(&mut self, request: Frame<'_>) -> Result<bool, SessionError> {
        self.owner
            .with_mut(|h| h.response_ready(request))
            .map_err(|_| SessionError::InvalidHandlerResponse)?
    }
    fn response_admitted(
        &mut self,
        request: Frame<'_>,
        response: &[u8],
    ) -> Result<(), SessionError> {
        self.owner
            .with_mut(|h| h.response_admitted(request, response))
            .map_err(|_| SessionError::InvalidHandlerResponse)?
    }
    fn connection_closed(&mut self) -> Result<(), SessionError> {
        self.owner
            .with_mut(ServiceHandler::connection_closed)
            .map_err(|_| SessionError::InvalidHandlerResponse)?
    }
    fn dispatch_ready(
        &mut self,
        request: Frame<'_>,
        out: &mut [u8],
    ) -> Result<usize, SessionError> {
        if let Ok(result) = self.owner.with_mut(|h| h.dispatch_ready(request, out)) {
            result
        } else {
            out.fill(0);
            Err(SessionError::InvalidHandlerResponse)
        }
    }
    fn dispatch(&mut self, request: Frame<'_>, out: &mut [u8]) -> Result<usize, SessionError> {
        if let Ok(result) = self.owner.with_mut(|h| h.dispatch(request, out)) {
            result
        } else {
            out.fill(0);
            Err(SessionError::InvalidHandlerResponse)
        }
    }
}
