use crate::{Backend, ContentHandler, Error, EventToken, wire::local_error};
use core::cell::{Cell, RefCell};
use device_connection::{EventOffer, EventPlane, EventRoute, EventSource, SourceFailure};
use device_protocol::{Frame, ModuleEntry};
use device_services::{AccessSnapshot, ServiceHandler, SessionError};
/// Shared host for short request/event borrows of the exact same connection handler.
/// It does not replace the one boot-global content owner, actual FIFO, or serialized actor boundary.
pub struct ContentHost<
    'a,
    'data,
    B: Backend,
    const N: usize,
    const C: usize,
    const A: usize,
    const Q: usize,
> {
    inner: RefCell<ContentHandler<'a, 'data, B, N, C, A, Q>>,
    ended: Cell<bool>,
}
impl<B: Backend, const N: usize, const C: usize, const A: usize, const Q: usize> core::fmt::Debug
    for ContentHost<'_, '_, B, N, C, A, Q>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ContentHost")
            .field("length", &core::mem::size_of_val(self))
            .finish_non_exhaustive()
    }
}
impl<'a, 'data, B: Backend, const N: usize, const C: usize, const A: usize, const Q: usize>
    ContentHost<'a, 'data, B, N, C, A, Q>
{
    /// Move one authenticated handler into its request/event hosting cell.
    #[must_use]
    pub const fn new(handler: ContentHandler<'a, 'data, B, N, C, A, Q>) -> Self {
        Self {
            inner: RefCell::new(handler),
            ended: Cell::new(false),
        }
    }
    /// Borrow checked operations on the same handler without exposing a mutable replaceable handler.
    /// # Errors
    /// Returns Busy for reentrancy, closed scope, or the actual typed operation failure.
    pub fn with_handler<R>(
        &self,
        operation: impl FnOnce(&mut ContentOperations<'_, 'a, 'data, B, N, C, A, Q>) -> Result<R, Error>,
    ) -> Result<R, Error> {
        if self.ended.get() {
            return Err(Error::Closed);
        }
        let mut handler = self.inner.try_borrow_mut().map_err(|_| Error::Busy)?;
        let result = operation(&mut ContentOperations {
            handler: &mut handler,
        });
        if self.ended.get() {
            let _ = handler.disconnect();
            return Err(Error::Closed);
        }
        result
    }
    /// Create a short-borrow request router for this exact same handler.
    #[must_use]
    pub const fn requests(&self) -> ContentRequests<'_, 'a, 'data, B, N, C, A, Q> {
        ContentRequests { host: self }
    }
    /// Create the permanently bindable ordinary-control source for `device_connection::EventPump`.
    #[must_use]
    pub const fn events(&self) -> ContentEventSource<'_, 'a, 'data, B, N, C, A, Q> {
        ContentEventSource { host: self }
    }
    fn close(&self) -> Result<(), Error> {
        self.ended.set(true);
        self.inner
            .try_borrow_mut()
            .map_err(|_| Error::Busy)?
            .disconnect()
    }
    fn ended(&self) -> bool {
        self.ended.get()
            || self
                .inner
                .try_borrow()
                .map_or(true, |handler| handler.must_end_session())
    }
    fn service<R>(
        &self,
        operation: impl FnOnce(&mut ContentHandler<'a, 'data, B, N, C, A, Q>) -> Result<R, SessionError>,
    ) -> Result<R, SessionError> {
        if self.ended.get() {
            return Err(SessionError::AuthorizationRevoked);
        }
        let Ok(mut handler) = self.inner.try_borrow_mut() else {
            self.ended.set(true);
            return Err(SessionError::InvalidHandlerResponse);
        };
        let result = operation(&mut handler);
        if self.ended.get() {
            let _ = handler.disconnect();
            return Err(SessionError::AuthorizationRevoked);
        }
        result
    }
}
/// Request-side checked handle. Its mutable borrow never spans an event-pump call.
pub struct ContentRequests<
    'host,
    'a,
    'data,
    B: Backend,
    const N: usize,
    const C: usize,
    const A: usize,
    const Q: usize,
> {
    host: &'host ContentHost<'a, 'data, B, N, C, A, Q>,
}
impl<B: Backend, const N: usize, const C: usize, const A: usize, const Q: usize> core::fmt::Debug
    for ContentRequests<'_, '_, '_, B, N, C, A, Q>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ContentRequests")
            .field("length", &core::mem::size_of_val(self))
            .finish_non_exhaustive()
    }
}
impl<B: Backend, const N: usize, const C: usize, const A: usize, const Q: usize> ServiceHandler
    for ContentRequests<'_, '_, '_, B, N, C, A, Q>
{
    fn informational_access(&self) -> Option<AccessSnapshot> {
        self.host
            .inner
            .try_borrow()
            .ok()
            .and_then(|handler| handler.informational_access())
    }
    fn required_features(&self, entry: &ModuleEntry, opcode: u8) -> Option<u32> {
        if self.host.ended.get() {
            return None;
        }
        self.host
            .inner
            .try_borrow()
            .ok()
            .and_then(|handler| handler.required_features(entry, opcode))
    }
    fn authorized(&self, request: Frame<'_>) -> bool {
        !self.host.ended.get()
            && self
                .host
                .inner
                .try_borrow()
                .is_ok_and(|handler| handler.authorized(request))
    }
    fn replay_valid(&self, request: Frame<'_>, response: &[u8]) -> bool {
        !self.host.ended.get()
            && self
                .host
                .inner
                .try_borrow()
                .is_ok_and(|handler| handler.replay_valid(request, response))
    }
    fn must_end_session(&self) -> bool {
        self.host.ended()
    }
    fn response_ready(&mut self, request: Frame<'_>) -> Result<bool, SessionError> {
        self.host.service(|handler| handler.response_ready(request))
    }
    fn dispatch(&mut self, request: Frame<'_>, out: &mut [u8]) -> Result<usize, SessionError> {
        let result = self.host.service(|handler| handler.dispatch(request, out));
        if result.is_err() {
            out.fill(0);
        }
        result
    }
    fn response_admitted(
        &mut self,
        request: Frame<'_>,
        response: &[u8],
    ) -> Result<(), SessionError> {
        self.host
            .service(|handler| handler.response_admitted(request, response))
    }
    fn connection_closed(&mut self) -> Result<(), SessionError> {
        self.host.close().map_err(local_error)
    }
}
/// Event-source checked handle, safe to own inside one `EventPump` while requests use another handle.
/// It always publishes ordinary control events: DATA message IDs and credit are never invented here.
pub struct ContentEventSource<
    'host,
    'a,
    'data,
    B: Backend,
    const N: usize,
    const C: usize,
    const A: usize,
    const Q: usize,
> {
    host: &'host ContentHost<'a, 'data, B, N, C, A, Q>,
}
impl<B: Backend, const N: usize, const C: usize, const A: usize, const Q: usize> core::fmt::Debug
    for ContentEventSource<'_, '_, '_, B, N, C, A, Q>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ContentEventSource")
            .field("length", &core::mem::size_of_val(self))
            .finish_non_exhaustive()
    }
}
impl<B: Backend, const N: usize, const C: usize, const A: usize, const Q: usize> EventSource
    for ContentEventSource<'_, '_, '_, B, N, C, A, Q>
{
    type Token = EventToken;
    fn route(&self) -> Result<Option<EventRoute>, SourceFailure> {
        if self.host.ended.get() {
            return Err(SourceFailure::Denied);
        }
        let handler = self
            .host
            .inner
            .try_borrow()
            .map_err(|_| SourceFailure::NotReady)?;
        handler
            .pending_event_route()
            .map(|route| {
                route.map(|(module, opcode)| EventRoute {
                    module,
                    opcode,
                    plane: EventPlane::Control,
                })
            })
            .map_err(source_error)
    }
    fn prepare(&mut self, out: &mut [u8]) -> Result<EventOffer<EventToken>, SourceFailure> {
        self.host
            .with_handler(|operations| operations.handler.prepare_event(out, out.len()))
            .map_err(source_error)?
            .map(|(token, length)| EventOffer {
                token,
                length,
                data_message: None,
            })
            .ok_or(SourceFailure::NotReady)
    }
    fn authorized(&self, token: EventToken) -> bool {
        !self.host.ended.get()
            && self
                .host
                .inner
                .try_borrow()
                .is_ok_and(|handler| !handler.must_end_session() && handler.event_authorized(token))
    }
    fn must_end_session(&self) -> bool {
        self.host.ended()
    }
    fn admitted(&mut self, token: EventToken) -> Result<(), SourceFailure> {
        self.host
            .with_handler(|operations| operations.handler.confirm_event(token))
            .map_err(source_error)
    }
    fn connection_closed(&mut self) -> Result<(), SourceFailure> {
        self.host.close().map_err(source_error)
    }
}
fn source_error(error: Error) -> SourceFailure {
    match error {
        Error::Busy => SourceFailure::NotReady,
        Error::Capacity | Error::Content(device_content::Error::Capacity) => {
            SourceFailure::Capacity
        }
        Error::Denied | Error::Closed | Error::Content(device_content::Error::Denied) => {
            SourceFailure::Denied
        }
        Error::Backend
        | Error::Clock
        | Error::Content(device_content::Error::Backend | device_content::Error::Unavailable) => {
            SourceFailure::Backend
        }
        _ => SourceFailure::Invalid,
    }
}

/// Scoped product operations, with no access to replace the handler bound to a pending sender.
/// There is deliberately no Deref/DerefMut or raw mutable-handler accessor.
pub struct ContentOperations<
    'borrow,
    'a,
    'data,
    B: Backend,
    const N: usize,
    const C: usize,
    const A: usize,
    const Q: usize,
> {
    handler: &'borrow mut ContentHandler<'a, 'data, B, N, C, A, Q>,
}
impl<B: Backend, const N: usize, const C: usize, const A: usize, const Q: usize> core::fmt::Debug
    for ContentOperations<'_, '_, '_, B, N, C, A, Q>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ContentOperations")
            .field("length", &core::mem::size_of_val(self))
            .finish_non_exhaustive()
    }
}
impl<B: Backend, const N: usize, const C: usize, const A: usize, const Q: usize>
    ContentOperations<'_, '_, '_, B, N, C, A, Q>
{
    /// Queue a device-originated action through the permanently bound handler.
    /// # Errors
    /// Returns the actual handler's capability, permission, capacity or lifecycle failure.
    #[cfg(any(feature = "phone", feature = "media"))]
    pub fn request_action(&mut self, action: device_content::Action) -> Result<u32, Error> {
        self.handler.request_action(action)
    }
    /// Queue actual device-originated FIND parameters.
    /// # Errors
    /// Returns the actual handler's validation, capability or lifecycle failure.
    #[cfg(feature = "find")]
    pub fn request_find(&mut self, request: device_content::FindRequest) -> Result<u32, Error> {
        self.handler.request_find(request)
    }
    /// Configure actual product-selected heartbeat policy.
    /// # Errors
    /// Returns the actual handler's range or source failure.
    #[cfg(feature = "phone")]
    pub fn configure_heartbeat(
        &mut self,
        policy: device_content::HeartbeatPolicy,
    ) -> Result<(), Error> {
        self.handler.configure_heartbeat(policy)
    }
    /// Observe actual monotonic heartbeat timing.
    /// # Errors
    /// Returns actual source/clock/lifecycle failure.
    #[cfg(feature = "phone")]
    pub fn poll_heartbeat(&mut self) -> Result<device_content::HeartbeatState, Error> {
        self.handler.poll_heartbeat()
    }
    /// Validate an actual source-phone P0002 push.
    /// # Errors
    /// Returns malformed fields, unknown identity or actual source failure.
    #[cfg(feature = "phone")]
    pub fn phone_status_received(&mut self, id: u32, bytes: &[u8]) -> Result<(), Error> {
        self.handler.phone_status_received(id, bytes)
    }
    /// Observe an actual find deadline.
    /// # Errors
    /// Returns the actual backend, FIFO or lifecycle failure.
    #[cfg(feature = "find")]
    pub fn poll_find(&mut self, id: u32) -> Result<device_content::FindState, Error> {
        self.handler.poll_find(id)
    }
    /// Confirm an actual local find observation.
    /// # Errors
    /// Returns the actual backend, FIFO or lifecycle failure.
    #[cfg(feature = "find")]
    pub fn found_device(
        &mut self,
        id: u32,
        result: device_content::FindResult,
    ) -> Result<(), Error> {
        self.handler.found_device(id, result)
    }
    /// Observe an actual gimbal ticket.
    /// # Errors
    /// Returns the actual backend, FIFO or lifecycle failure.
    #[cfg(feature = "gimbal")]
    pub fn poll_gimbal(&mut self, id: u32) -> Result<device_content::GimbalState, Error> {
        self.handler.poll_gimbal(id)
    }
    /// Observe actual independent radio state and its committed property revision.
    /// # Errors
    /// Returns the actual hardware/store/FIFO or lifecycle failure.
    #[cfg(feature = "companion-radio")]
    pub fn poll_radio(&mut self) -> Result<device_content::CompanionRadioState, Error> {
        self.handler.poll_radio()
    }
    /// Present real committed notification content after actual response admission.
    /// # Errors
    /// Returns the actual source, object or UI backend failure.
    #[cfg(feature = "notifications")]
    pub fn notification_response_admitted(&mut self, object: u32) -> Result<(), Error> {
        self.handler.notification_response_admitted(object)
    }
    /// Queue a supported real source-phone call action.
    /// # Errors
    /// Returns the actual source, notification, capability or FIFO failure.
    #[cfg(feature = "notifications")]
    pub fn request_notification_action(
        &mut self,
        notification: u32,
        action: u8,
    ) -> Result<(), Error> {
        self.handler
            .request_notification_action(notification, action)
    }
    /// Check actual notification deletion prerequisites.
    /// # Errors
    /// Returns actual source/object failure.
    #[cfg(feature = "notifications")]
    pub fn notification_delete_ready(&self, object: u32) -> Result<bool, Error> {
        self.handler.notification_delete_ready(object)
    }
    /// Apply an actual committed deletion after response admission.
    /// # Errors
    /// Returns actual source/object/UI backend failure.
    #[cfg(feature = "notifications")]
    pub fn notification_deleted(&mut self, object: u32) -> Result<(), Error> {
        self.handler.notification_deleted(object)
    }
    /// Close the bound handler and attempt its actual cleanup.
    /// # Errors
    /// Reports actual cleanup failure while keeping old work muted.
    pub fn disconnect(&mut self) -> Result<(), Error> {
        self.handler.disconnect()
    }
}
#[cfg(feature = "notifications")]
impl<'data, B: Backend, const N: usize, const C: usize, const A: usize, const Q: usize>
    ContentOperations<'_, '_, 'data, B, N, C, A, Q>
{
    /// Stage actual schema2 bytes from the stable committed storage view.
    /// # Errors
    /// Returns the actual decoder, scope, generation or capacity failure.
    pub fn stage_notification_bytes(
        &mut self,
        object: u32,
        bytes: &'data [u8],
        generation: u64,
    ) -> Result<device_content::NotificationChange, Error> {
        self.handler
            .stage_notification_bytes(object, bytes, generation)
    }
}
