//! Static heterogeneous routing; the tuple retains every concrete owner type.
use crate::{HandlerTableError, ServiceHandler, SessionError};
use device_protocol::{Frame, Kind, ModuleEntry};
mod sealed {
    pub trait Sealed {}
}
/// Sealed three/four-owner tuple. Indexed calls preserve the concrete callee type.
/// Indexes outside the tuple fail closed; callers never receive a trait object.
pub trait HandlerGroup: sealed::Sealed {
    /// Number of owners in this fixed tuple.
    fn handler_count(&self) -> usize;
    /// Advance one owner even if another owner failed.
    /// # Errors
    /// Owner failures or an invalid index remain explicit.
    fn advance(&mut self, index: usize, now: u64) -> Result<(), SessionError>;
    /// Read one informational access snapshot.
    fn informational_access(&self, index: usize) -> Option<crate::AccessSnapshot>;
    /// Read one owner's opcode/features declaration.
    fn required_features(&self, index: usize, entry: &ModuleEntry, opcode: u8) -> Option<u32>;
    /// Recheck one owner's actual request permission.
    fn authorized(&self, index: usize, request: Frame<'_>) -> bool;
    /// Check exact cached response context.
    fn replay_valid(&self, index: usize, request: Frame<'_>, response: &[u8]) -> bool;
    /// Poll a selected prerequisite without dispatching.
    /// # Errors
    /// Owner failures or an invalid index remain explicit.
    fn response_ready(&mut self, index: usize, request: Frame<'_>) -> Result<bool, SessionError>;
    /// Report exact complete response admission.
    /// # Errors
    /// Owner failures or an invalid index remain explicit.
    fn response_admitted(
        &mut self,
        index: usize,
        request: Frame<'_>,
        response: &[u8],
    ) -> Result<(), SessionError>;
    /// Close one owner without hiding its failure.
    /// # Errors
    /// Owner failures or an invalid index remain explicit.
    fn connection_closed(&mut self, index: usize) -> Result<(), SessionError>;
    /// Check a selected owner's terminal scope state.
    fn must_end_session(&self, index: usize) -> bool;
    /// Execute one selected owner without type erasure.
    /// # Errors
    /// Owner failures or an invalid index remain explicit.
    fn dispatch(
        &mut self,
        index: usize,
        request: Frame<'_>,
        out: &mut [u8],
    ) -> Result<usize, SessionError>;
    /// Execute a selected owner after its exact readiness admission.
    /// # Errors
    /// Missing readiness, owner failures or an invalid index remain explicit.
    fn dispatch_ready(
        &mut self,
        index: usize,
        request: Frame<'_>,
        out: &mut [u8],
    ) -> Result<usize, SessionError>;
}
macro_rules! tuple_group {
 ($count:expr; $( $T:ident : $i:tt ),+) => {
 impl<$($T: ServiceHandler),+> sealed::Sealed for ($(&mut $T,)+) {}
 impl<$($T: ServiceHandler),+> HandlerGroup for ($(&mut $T,)+) {
 fn handler_count(&self) -> usize { $count }
 fn advance(&mut self, index: usize, now: u64) -> Result<(), SessionError> {
 match index { $($i => self.$i.advance(now),)+ _ => Err(SessionError::InvalidHandlerResponse) }
 }
 fn informational_access(&self, index: usize) -> Option<crate::AccessSnapshot> {
 match index { $($i => self.$i.informational_access(),)+ _ => None }
 }
 fn required_features(&self, index: usize, entry: &ModuleEntry, opcode: u8) -> Option<u32> {
 match index { $($i => self.$i.required_features(entry, opcode),)+ _ => None }
 }
 fn authorized(&self, index: usize, request: Frame<'_>) -> bool {
 match index { $($i => self.$i.authorized(request),)+ _ => false }
 }
 fn replay_valid(&self, index: usize, request: Frame<'_>, response: &[u8]) -> bool {
 match index { $($i => self.$i.replay_valid(request, response),)+ _ => false }
 }
 fn response_ready(&mut self, index: usize, request: Frame<'_>) -> Result<bool, SessionError> {
 match index { $($i => self.$i.response_ready(request),)+ _ => Err(SessionError::InvalidHandlerResponse) }
 }
 fn response_admitted(&mut self, index: usize, request: Frame<'_>, response: &[u8]) -> Result<(), SessionError> {
 match index { $($i => self.$i.response_admitted(request, response),)+ _ => Err(SessionError::InvalidHandlerResponse) }
 }
 fn connection_closed(&mut self, index: usize) -> Result<(), SessionError> {
 match index { $($i => self.$i.connection_closed(),)+ _ => Err(SessionError::InvalidHandlerResponse) }
 }
 fn must_end_session(&self, index: usize) -> bool {
 match index { $($i => self.$i.must_end_session(),)+ _ => true }
 }
 fn dispatch(&mut self, index: usize, request: Frame<'_>, out: &mut [u8]) -> Result<usize, SessionError> {
 match index { $($i => self.$i.dispatch(request, out),)+ _ => Err(SessionError::InvalidHandlerResponse) }
 }
 fn dispatch_ready(&mut self, index: usize, request: Frame<'_>, out: &mut [u8]) -> Result<usize, SessionError> {
 match index { $($i => dispatch_one_ready(self.$i, request, out),)+ _ => Err(SessionError::InvalidHandlerResponse) }
 }
} };
}
tuple_group!(3; A:0,B:1,C:2);
tuple_group!(4; A:0,B:1,C:2,D:3);
/// Compose handlers without a heap, secondary request cache, or implicit authority fallback.
/// Registry snapshots must match the owning Session exactly. Duplicate route ownership fails closed.
pub struct TypedHandlerTable<'a, G: HandlerGroup> {
    entries: &'a [ModuleEntry],
    handlers: G,
}
impl<G: HandlerGroup> core::fmt::Debug for TypedHandlerTable<'_, G> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TypedHandlerTable")
            .field("module_count", &self.entries.len())
            .field("handler_count", &self.handlers.handler_count())
            .finish_non_exhaustive()
    }
}
impl<'a, G: HandlerGroup> TypedHandlerTable<'a, G> {
    /// Bind immutable capability metadata to borrowed handlers for a connection.
    /// # Errors
    /// Rejects duplicate/unsorted modules, invalid fields or insufficient success-status capacity.
    pub fn new(entries: &'a [ModuleEntry], handlers: G) -> Result<Self, HandlerTableError> {
        let mut previous = None;
        for e in entries {
            if previous.is_some_and(|p| p >= e.module)
                || e.encode(&mut [0; 12]).is_err()
                || (e.availability == 1 && e.output_max < 4)
            {
                return Err(HandlerTableError::InvalidEntries);
            }
            previous = Some(e.module);
        }
        Ok(Self { entries, handlers })
    }
    fn entry(&self, module: u8) -> Option<&ModuleEntry> {
        self.entries
            .iter()
            .find(|e| e.module == module && e.availability == 1)
    }
    fn selected(&self, entry: &ModuleEntry, opcode: u8) -> Option<(usize, u32)> {
        let mut found = None;
        for i in 0..self.handlers.handler_count() {
            if let Some(bits) = self.handlers.required_features(i, entry, opcode) {
                if found.is_some() || bits & !entry.features != 0 {
                    return None;
                }
                found = Some((i, bits));
            }
        }
        found
    }
    fn dispatch_inner(
        &mut self,
        request: Frame<'_>,
        out: &mut [u8],
        ready: bool,
    ) -> Result<usize, SessionError> {
        if request.header.kind != Kind::Request
            || request.header.offset != 0
            || request.header.message_id == 0
            || usize::from(request.header.total_length) != request.payload.len()
        {
            return Err(SessionError::InvalidHandlerResponse);
        }
        let entry = *self
            .entry(request.header.module)
            .ok_or(SessionError::InvalidHandlerResponse)?;
        if request.payload.len() > usize::from(entry.input_max) {
            return Err(SessionError::InvalidHandlerResponse);
        }
        if out.len() < 4 {
            return Err(SessionError::Capacity);
        }
        let (index, _) = self
            .selected(&entry, request.header.opcode)
            .ok_or(SessionError::InvalidHandlerResponse)?;
        if !self.handlers.authorized(index, request) {
            return Err(SessionError::AuthorizationRevoked);
        }
        let limit = out.len().min(usize::from(entry.output_max));
        let len = if ready {
            self.handlers
                .dispatch_ready(index, request, &mut out[..limit])?
        } else {
            self.handlers.dispatch(index, request, &mut out[..limit])?
        };
        if !self.handlers.authorized(index, request) {
            return Err(SessionError::AuthorizationRevoked);
        }
        if len < 4 || len > usize::from(entry.output_max) || len > out.len() {
            return Err(SessionError::InvalidHandlerResponse);
        }
        Ok(len)
    }
}
impl<G: HandlerGroup> ServiceHandler for TypedHandlerTable<'_, G> {
    fn advance(&mut self, now_ms: u64) -> Result<(), SessionError> {
        let mut first = None;
        for i in 0..self.handlers.handler_count() {
            if let Err(error) = self.handlers.advance(i, now_ms) {
                first.get_or_insert(error);
            }
        }
        first.map_or(Ok(()), Err)
    }
    fn informational_access(&self) -> Option<crate::AccessSnapshot> {
        let mut snapshot = None;
        for i in 0..self.handlers.handler_count() {
            if let Some(next) = self.handlers.informational_access(i) {
                if snapshot.is_some_and(|s| s != next) {
                    return None;
                }
                snapshot = Some(next);
            }
        }
        snapshot
    }
    fn replay_valid(&self, request: Frame<'_>, response: &[u8]) -> bool {
        self.entry(request.header.module)
            .and_then(|entry| self.selected(entry, request.header.opcode))
            .is_some_and(|(i, _)| self.handlers.replay_valid(i, request, response))
    }
    fn response_ready(&mut self, request: Frame<'_>) -> Result<bool, SessionError> {
        if self.must_end_session() {
            return Err(SessionError::AuthorizationRevoked);
        }
        let entry = self
            .entry(request.header.module)
            .ok_or(SessionError::InvalidHandlerResponse)?;
        let (index, _) = self
            .selected(entry, request.header.opcode)
            .ok_or(SessionError::InvalidHandlerResponse)?;
        self.handlers.response_ready(index, request)
    }
    fn response_admitted(
        &mut self,
        request: Frame<'_>,
        response: &[u8],
    ) -> Result<(), SessionError> {
        if !self.authorized(request) {
            return Err(SessionError::AuthorizationRevoked);
        }
        let entry = self
            .entry(request.header.module)
            .ok_or(SessionError::InvalidHandlerResponse)?;
        let (index, _) = self
            .selected(entry, request.header.opcode)
            .ok_or(SessionError::InvalidHandlerResponse)?;
        self.handlers.response_admitted(index, request, response)
    }
    fn connection_closed(&mut self) -> Result<(), SessionError> {
        let mut first = None;
        for i in 0..self.handlers.handler_count() {
            if let Err(error) = self.handlers.connection_closed(i) {
                first.get_or_insert(error);
            }
        }
        first.map_or(Ok(()), Err)
    }
    fn must_end_session(&self) -> bool {
        (0..self.handlers.handler_count()).any(|i| self.handlers.must_end_session(i))
    }
    fn required_features(&self, module: &ModuleEntry, opcode: u8) -> Option<u32> {
        if self.entry(module.module) != Some(module) {
            return None;
        }
        self.selected(module, opcode).map(|v| v.1)
    }
    fn authorized(&self, request: Frame<'_>) -> bool {
        if self.must_end_session() {
            return false;
        }
        self.entry(request.header.module)
            .and_then(|entry| self.selected(entry, request.header.opcode))
            .is_some_and(|(i, _)| self.handlers.authorized(i, request))
    }
    fn dispatch_ready(
        &mut self,
        request: Frame<'_>,
        out: &mut [u8],
    ) -> Result<usize, SessionError> {
        let result = self.dispatch_inner(request, out, true);
        if result.is_err() {
            out.fill(0);
        }
        result
    }
    fn dispatch(&mut self, request: Frame<'_>, out: &mut [u8]) -> Result<usize, SessionError> {
        let result = self.dispatch_inner(request, out, false);
        if result.is_err() {
            out.fill(0);
        }
        result
    }
}

// Prevent unrelated owners' staging buffers from sharing the router's frame.
// This remains a statically monomorphized call, with no function pointer or dyn.
#[inline(never)]
fn dispatch_one_ready<H: ServiceHandler>(
    handler: &mut H,
    request: Frame<'_>,
    out: &mut [u8],
) -> Result<usize, SessionError> {
    handler.dispatch_ready(request, out)
}
