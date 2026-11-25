//! A persistent connection actor; durable work belongs to the separate boot owner.
use crate::{AsyncPropertyStore, MutationToken, PropertySchema};
use core::fmt;
use device_protocol::{Frame, Header, Kind, LOGICAL_MAX, ModuleEntry, PropertyRequest};
use device_services::{
    PropertyAccess, PropertyBackend, PropertyConfigError, PropertyDispatchError, PropertyFailure,
    PropertyOperation, PropertyService, RevisionChange, ServiceHandler, SessionError,
};
use device_storage::incremental::AsyncMedium;

#[derive(Debug, Clone, Copy)]
enum State {
    Idle,
    Waiting(MutationToken),
    Ready {
        token: Option<MutationToken>,
        result: Result<RevisionChange, PropertyFailure>,
    },
}
/// Persistent async property actor bound to one real connection lifecycle.
///
/// Session performs identity, declared-version/features, authorization, input and
/// configured-output checks before readiness. The constructor must receive that
/// same effective output maximum: min(module output, negotiated control maximum).
/// R stores exact request bytes and must cover the configured logical maximum;
/// selected 128-byte settings sessions need no 512-byte actor cache. Drop detaches
/// delivery through a borrow-free owner marker; accepted storage always continues.
/// S bounds temporary property encoding only; its default preserves the legacy
/// 512-byte backend read/staging capacity. Smaller bounds are explicit opt-ins.
pub struct AsyncPropertyHandler<
    'owner,
    'auth,
    'catalogue,
    M: AsyncMedium,
    C: PropertySchema,
    const N: usize,
    const R: usize = 128,
    const S: usize = LOGICAL_MAX,
> {
    owner: &'owner AsyncPropertyStore<'catalogue, M, C, N>,
    access: &'auth dyn PropertyAccess,
    module: u8,
    version: (u8, u8),
    logical_max: usize,
    output_max: usize,
    header: Option<Header>,
    request: [u8; R],
    length: usize,
    state: State,
    closed: bool,
}
impl<M: AsyncMedium, C: PropertySchema, const N: usize, const R: usize, const S: usize> fmt::Debug
    for AsyncPropertyHandler<'_, '_, '_, M, C, N, R, S>
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AsyncPropertyHandler")
            .field("module", &self.module)
            .field("request_length", &self.length)
            .field("closed", &self.closed)
            .finish_non_exhaustive()
    }
}
impl<
    'owner,
    'auth,
    'catalogue,
    M: AsyncMedium,
    C: PropertySchema,
    const N: usize,
    const R: usize,
    const S: usize,
> AsyncPropertyHandler<'owner, 'auth, 'catalogue, M, C, N, R, S>
{
    /// Bind the stable boot owner and live scope for one connection.
    /// # Errors
    /// Reject unavailable modules/catalogues or logical/request/output bounds.
    pub fn new(
        owner: &'owner AsyncPropertyStore<'catalogue, M, C, N>,
        module: u8,
        access: &'auth dyn PropertyAccess,
        version: (u8, u8),
        logical_max: usize,
        effective_output_max: usize,
    ) -> Result<Self, PropertyConfigError> {
        if !(1..=LOGICAL_MAX).contains(&S)
            || logical_max > S
            || !(1..=LOGICAL_MAX).contains(&R)
            || !(128..=LOGICAL_MAX).contains(&logical_max)
            || logical_max > R
            || !(4..=logical_max).contains(&effective_output_max)
        {
            return Err(PropertyConfigError::InvalidLimit);
        }
        let mut adapter = owner
            .adapter(module)
            .map_err(|_| PropertyConfigError::InvalidCatalogue)?;
        let _ = PropertyService::new(module, &mut adapter, logical_max)?;
        if adapter
            .catalogue()
            .iter()
            .any(|entry| entry.access & 4 == 0 && usize::from(entry.value_max) > S)
        {
            return Err(PropertyConfigError::InvalidCatalogue);
        }
        Ok(Self {
            owner,
            access,
            module,
            version,
            logical_max,
            output_max: effective_output_max,
            header: None,
            request: [0; R],
            length: 0,
            state: State::Idle,
            closed: false,
        })
    }
    fn validate_frame(&self, request: Frame<'_>) -> Result<(), SessionError> {
        if request.header.kind != Kind::Request
            || request.header.message_id == 0
            || request.header.offset != 0
            || usize::from(request.header.total_length) != request.payload.len()
            || request.payload.len() > self.logical_max
            || request.payload.len() > R
        {
            Err(SessionError::InvalidFrame)
        } else {
            Ok(())
        }
    }
    fn matches(&self, request: Frame<'_>) -> bool {
        self.header == Some(request.header) && self.request[..self.length] == *request.payload
    }
    fn retained(&self) -> Option<Frame<'_>> {
        self.header.map(|header| Frame {
            header,
            payload: &self.request[..self.length],
        })
    }
    fn clear(&mut self) {
        self.header = None;
        self.request.fill(0);
        self.length = 0;
        self.state = State::Idle;
    }
    fn token(&self) -> Option<MutationToken> {
        match self.state {
            State::Waiting(token) => Some(token),
            State::Ready { token, .. } => token,
            State::Idle => None,
        }
    }
    fn close(&mut self) -> Result<(), SessionError> {
        self.closed = true;
        let result = self.token().map_or(Ok(()), |token| {
            self.owner
                .detach(token)
                .map_err(|_| SessionError::InvalidHandlerResponse)
        });
        self.clear();
        result
    }
    fn prepare(&self, request: Frame<'_>) -> Result<MutationToken, PropertyFailure> {
        let PropertyRequest::Set {
            key,
            expected_revision,
            value,
        } = PropertyRequest::decode(request.header.opcode, request.payload)
            .map_err(|_| PropertyFailure::InvalidValue)?
        else {
            return Err(PropertyFailure::InvalidValue);
        };
        if self.output_max < 8 {
            return Err(PropertyFailure::LimitExceeded);
        }
        self.owner
            .begin_set(self.module, key, expected_revision, value, self.access)
    }
    fn poll_retained(&mut self) -> Result<bool, SessionError> {
        let State::Waiting(token) = self.state else {
            return Ok(true);
        };
        match self
            .owner
            .result(token)
            .map_err(|_| SessionError::InvalidHandlerResponse)?
        {
            core::task::Poll::Pending => Ok(false),
            core::task::Poll::Ready(result) => {
                let result = result
                    .and_then(|receipt| receipt.change.ok_or(PropertyFailure::InternalFailure));
                self.state = State::Ready {
                    token: Some(token),
                    result,
                };
                Ok(true)
            }
        }
    }
    fn dispatch_set(&mut self, request: Frame<'_>, out: &mut [u8]) -> Result<usize, SessionError> {
        if !self.matches(request) {
            return Err(SessionError::InvalidHandlerResponse);
        }
        let State::Ready { token, result } = self.state else {
            return Err(SessionError::InvalidHandlerResponse);
        };
        let length = if result.is_ok() { 8 } else { 4 };
        if out.len() < length {
            return Err(SessionError::Capacity);
        }
        if let Some(token) = token {
            let actual = self
                .owner
                .take_result(token)
                .map_err(|_| SessionError::InvalidHandlerResponse)?
                .and_then(|receipt| receipt.change.ok_or(PropertyFailure::InternalFailure));
            if actual != result {
                return Err(SessionError::InvalidHandlerResponse);
            }
        }
        out[..length].fill(0);
        match result {
            Ok(change) => {
                if change.previous == 0 || change.previous.checked_add(1) != Some(change.current) {
                    return Err(SessionError::InvalidHandlerResponse);
                }
                out[4..8].copy_from_slice(&change.current.to_le_bytes());
            }
            Err(error) => {
                let (status, reason) = error.status();
                out[..2].copy_from_slice(&(status as u16).to_le_bytes());
                out[2..4].copy_from_slice(&(reason as u16).to_le_bytes());
            }
        }
        self.clear();
        Ok(length)
    }
}
impl<M: AsyncMedium, C: PropertySchema, const N: usize, const R: usize, const S: usize> Drop
    for AsyncPropertyHandler<'_, '_, '_, M, C, N, R, S>
{
    fn drop(&mut self) {
        // Detach uses only Cell. It cannot fail from a recursive owner borrow;
        // stale tokens already have no delivery reservation to release.
        if let Some(token) = self.token() {
            let _ = self.owner.detach(token);
        }
    }
}
impl<M: AsyncMedium, C: PropertySchema, const N: usize, const R: usize, const S: usize>
    ServiceHandler for AsyncPropertyHandler<'_, '_, '_, M, C, N, R, S>
{
    fn advance(&mut self, _now_ms: u64) -> Result<(), SessionError> {
        if self
            .retained()
            .is_some_and(|request| !self.authorized(request))
        {
            self.close()?;
            return Err(SessionError::AuthorizationRevoked);
        }
        Ok(())
    }
    fn required_features(&self, module: &ModuleEntry, opcode: u8) -> Option<u32> {
        (module.module == self.module
            && (module.major, module.minor) == self.version
            && usize::from(module.output_max).min(self.logical_max) == self.output_max
            && matches!(opcode, 0x10..=0x12))
        .then_some(0)
    }
    fn authorized(&self, request: Frame<'_>) -> bool {
        if self.closed || request.header.module != self.module {
            return false;
        }
        let operation = match request.header.opcode {
            0x10 => PropertyOperation::Get,
            0x11 => PropertyOperation::Set,
            _ => PropertyOperation::Describe,
        };
        if !self.access.allowed(self.module, operation, None) {
            return false;
        }
        let Ok(body) = PropertyRequest::decode(request.header.opcode, request.payload) else {
            return true;
        };
        match body {
            PropertyRequest::Get { key } | PropertyRequest::Set { key, .. } => {
                self.access.allowed(self.module, operation, Some(key))
            }
            PropertyRequest::Describe { start, max } => self
                .owner
                .catalogue
                .iter()
                .find(|m| m.module == self.module)
                .is_some_and(|m| {
                    m.properties
                        .iter()
                        .filter(|d| d.key >= start)
                        .take(usize::from(max) + 1)
                        .all(|d| self.access.allowed(self.module, operation, Some(d.key)))
                }),
        }
    }
    #[inline(never)]
    fn response_ready(&mut self, request: Frame<'_>) -> Result<bool, SessionError> {
        self.validate_frame(request)?;
        if !self.authorized(request) {
            return Err(SessionError::AuthorizationRevoked);
        }
        if !matches!(self.state, State::Idle) {
            if !self.matches(request) {
                return Err(SessionError::InvalidHandlerResponse);
            }
            return self.poll_retained();
        }
        if request.header.opcode != 0x11 {
            return Ok(true);
        }
        self.header = Some(request.header);
        self.length = request.payload.len();
        self.request[..self.length].copy_from_slice(request.payload);
        self.state = match self.prepare(request) {
            Ok(token) => State::Waiting(token),
            Err(error) => State::Ready {
                token: None,
                result: Err(error),
            },
        };
        self.poll_retained()
    }
    fn dispatch(&mut self, request: Frame<'_>, out: &mut [u8]) -> Result<usize, SessionError> {
        self.validate_frame(request)?;
        if !self.authorized(request) {
            out.fill(0);
            return Err(SessionError::AuthorizationRevoked);
        }
        if request.header.opcode == 0x11 {
            return self.dispatch_set(request, out);
        }
        if !matches!(self.state, State::Idle) {
            return Err(SessionError::InvalidHandlerResponse);
        }
        let mut adapter = self
            .owner
            .adapter(self.module)
            .map_err(|_| SessionError::InvalidHandlerResponse)?;
        let mut service = PropertyService::new(self.module, &mut adapter, self.logical_max)
            .map_err(|_| SessionError::InvalidHandlerResponse)?;
        service
            .dispatch_bounded::<S>(request, self.access, out)
            .map_err(|error| match error {
                PropertyDispatchError::Capacity => SessionError::Capacity,
                PropertyDispatchError::InvalidFrame => SessionError::InvalidFrame,
                PropertyDispatchError::AccessRevoked => SessionError::AuthorizationRevoked,
            })
    }
    fn connection_closed(&mut self) -> Result<(), SessionError> {
        self.close()
    }
    fn must_end_session(&self) -> bool {
        self.closed
    }
}
