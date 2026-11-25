//! Borrowed heterogeneous handler composition. The owning Session remains the sole request/cache authority.
use crate::{ServiceHandler, SessionError};
use device_protocol::{Frame, Kind, ModuleEntry};
/// Invalid connection-static module table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandlerTableError {
    /// Unsorted/duplicate/invalid entries or impossible response capacity.
    InvalidEntries,
}
/// Compose handlers without a heap, secondary request cache, or implicit authority fallback.
/// Registry snapshots must match the owning Session exactly. Duplicate route ownership fails closed.
pub struct HandlerTable<'a, 'h> {
    entries: &'a [ModuleEntry],
    handlers: &'a mut [&'h mut dyn ServiceHandler],
}
impl core::fmt::Debug for HandlerTable<'_, '_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HandlerTable")
            .field("module_count", &self.entries.len())
            .field("handler_count", &self.handlers.len())
            .finish_non_exhaustive()
    }
}
impl<'a, 'h> HandlerTable<'a, 'h> {
    /// Bind immutable capability metadata to borrowed handlers for a connection.
    /// # Errors
    /// Rejects duplicate/unsorted modules, invalid fields or insufficient success-status capacity.
    pub fn new(
        entries: &'a [ModuleEntry],
        handlers: &'a mut [&'h mut dyn ServiceHandler],
    ) -> Result<Self, HandlerTableError> {
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
        for (i, handler) in self.handlers.iter().enumerate() {
            if let Some(bits) = handler.required_features(entry, opcode) {
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
        let handler = &mut self.handlers[index];
        if !handler.authorized(request) {
            return Err(SessionError::AuthorizationRevoked);
        }
        let limit = out.len().min(usize::from(entry.output_max));
        let len = handler.dispatch(request, &mut out[..limit])?;
        if !handler.authorized(request) {
            return Err(SessionError::AuthorizationRevoked);
        }
        if len < 4 || len > usize::from(entry.output_max) || len > out.len() {
            return Err(SessionError::InvalidHandlerResponse);
        }
        Ok(len)
    }
}
impl ServiceHandler for HandlerTable<'_, '_> {
    fn advance(&mut self, now_ms: u64) -> Result<(), SessionError> {
        let mut first = None;
        for handler in self.handlers.iter_mut() {
            if let Err(error) = handler.advance(now_ms) {
                first.get_or_insert(error);
            }
        }
        first.map_or(Ok(()), Err)
    }
    fn informational_access(&self) -> Option<crate::AccessSnapshot> {
        let mut snapshot = None;
        for h in self.handlers.iter() {
            if let Some(next) = h.informational_access() {
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
            .is_some_and(|(i, _)| self.handlers[i].replay_valid(request, response))
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
        self.handlers[index].response_ready(request)
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
        self.handlers[index].response_admitted(request, response)
    }
    fn connection_closed(&mut self) -> Result<(), SessionError> {
        let mut first = None;
        for handler in self.handlers.iter_mut() {
            if let Err(error) = handler.connection_closed() {
                first.get_or_insert(error);
            }
        }
        first.map_or(Ok(()), Err)
    }
    fn must_end_session(&self) -> bool {
        self.handlers.iter().any(|h| h.must_end_session())
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
            .is_some_and(|(i, _)| self.handlers[i].authorized(request))
    }
    fn dispatch(&mut self, request: Frame<'_>, out: &mut [u8]) -> Result<usize, SessionError> {
        let result = self.dispatch_inner(request, out);
        if result.is_err() {
            out.fill(0);
        }
        result
    }
}
