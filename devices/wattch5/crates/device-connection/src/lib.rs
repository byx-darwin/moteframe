//! Bounded control transport orchestration. Actual BLE queue admission remains an adapter contract.
#![no_std]
use device_protocol::{
    Frame, HEADER_LENGTH, Header, Kind, LOGICAL_MAX, Progress, ProtocolError, Reassembler,
};
/// Unified service/lifecycle contract; no transport-specific business trait is required.
pub use device_services::ServiceHandler as AdmissionHandler;
use device_services::{BoundedSession, Config, ConfigError, SessionError};
mod channel;
pub use channel::{EventChannel, SharedControlChannel};
/// Explicit control lifecycle failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionError {
    /// Configuration is invalid.
    Config(ConfigError),
    /// Wire framing/receive lifecycle failed.
    Protocol(ProtocolError),
    /// Session/handler rejected the command.
    Session(SessionError),
    /// Outbound response must drain before another request may begin.
    Busy,
    /// The external physical-connection owner has already been bound or closed.
    ChannelInUse,
    /// No prepared notification or no response exists.
    InvalidState,
    /// Local output capacity is insufficient; pending bytes are retained.
    Capacity,
    /// The connection cannot be reopened after close/fault.
    Closed,
    /// Backend cleanup failed while ending the session.
    CleanupFailed(SessionError),
}
/// Result of admitting one incoming complete GATT value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Receive {
    /// More contiguous fragments required.
    Pending,
    /// A complete response now waits for transport admission.
    ResponseReady,
    /// Dispatch is staged or a prerequisite is draining; the complete request is retained locally.
    Deferred,
}
#[derive(Debug, Clone, Copy)]
struct Transmit {
    header: Header,
    offset: usize,
    length: usize,
    prepared: bool,
    callback_due: bool,
}
/// One connection's control channel, fixed receive/transmit buffers and the sole Session cache.
/// DATA producers use a separate shared `ConnectionDataLane`. Hardware adapters must serialize scope
/// revocation with actual notification queue admission; merely obtaining bytes is not admission.
pub type ControlConnection<'a> = BoundedControlConnection<'a, LOGICAL_MAX>;
/// CONTROL actor with bounded receive/transmit/replay storage. DATA remains separately bounded.
pub struct BoundedControlConnection<'a, const N: usize> {
    session: BoundedSession<'a, N>,
    rx: Reassembler<N>,
    tx_bytes: [u8; N],
    tx: Option<Transmit>,
    tx_frame_max: usize,
    rx_frame_max: usize,
    last_admitted: u16,
    waiting_handler: bool,
    waiting_event: bool,
    channel: Option<&'a SharedControlChannel>,
    request_started: Option<u64>,
    last_now: Option<u64>,
    response_budget: u64,
    closed: bool,
    cleanup_pending: Option<SessionError>,
}
impl<const N: usize> core::fmt::Debug for BoundedControlConnection<'_, N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ControlConnection")
            .field("response_pending", &self.tx.is_some())
            .field("closed", &self.closed)
            .finish_non_exhaustive()
    }
}
impl<'a, const N: usize> BoundedControlConnection<'a, N> {
    /// Create one fresh physical connection's control state.
    /// # Errors
    /// Rejects invalid session/transport limits.
    pub fn new(config: Config<'a>) -> Result<Self, ConnectionError> {
        let frame_max = usize::from(config.limits.rx_frame_max);
        let logical_max = usize::from(config.limits.control_max);
        let tx_frame_max = usize::from(config.limits.tx_frame_max);
        let rx = Reassembler::new(frame_max, logical_max).map_err(ConnectionError::Protocol)?;
        let response_budget = u64::from(config.limits.response_ms);
        let session = BoundedSession::new(config).map_err(ConnectionError::Config)?;
        Ok(Self {
            session,
            rx,
            tx_bytes: [0; N],
            tx: None,
            tx_frame_max,
            rx_frame_max: frame_max,
            last_admitted: 0,
            waiting_handler: false,
            waiting_event: false,
            channel: None,
            request_started: None,
            last_now: None,
            response_budget,
            closed: false,
            cleanup_pending: None,
        })
    }
    /// Pair one control connection with the sole event-sender token.
    /// # Errors
    /// Invalid actual configuration or an unavailable external owner is explicit.
    pub fn with_events(
        config: Config<'a>,
        owner: &'a SharedControlChannel,
    ) -> Result<(Self, EventChannel<'a>), ConnectionError> {
        let mut control = Self::new(config)?;
        if !owner.bind() {
            return Err(ConnectionError::ChannelInUse);
        }
        control.channel = Some(owner);
        Ok((
            control,
            EventChannel {
                owner,
                entries: config.modules,
                initial_limits: config.limits,
            },
        ))
    }
    /// Enable control notifications for a fresh connection; repeated enable is idempotent while open.
    /// # Errors
    /// Rejects a closed connection.
    pub fn notifications_enabled(&mut self) -> Result<(), ConnectionError> {
        if self.closed || self.channel.is_some_and(SharedControlChannel::is_closed) {
            return Err(ConnectionError::Closed);
        }
        self.session
            .control_notifications_enabled()
            .map_err(ConnectionError::Session)
    }
    /// Actual active HELLO limits for constructing bounded event/DATA transmit adapters.
    #[must_use]
    pub fn active_limits(&self) -> Option<device_protocol::ProtocolLimits> {
        if self.closed || self.channel.is_some_and(SharedControlChannel::is_closed) {
            None
        } else {
            self.session.active_limits()
        }
    }

    /// Admit one GATT value. Incoming requests are backpressured while any response fragment is pending.
    /// A request received during an ongoing control EVENT is retained completely and deferred until
    /// that event drains, without executing its handler. On Busy the occupied request/response slot
    /// consumes no input: the transport must retain/reject that write, never silently drop it.
    /// # Errors
    /// Malformed input closes the session; output backpressure returns Busy without consuming input.
    pub fn receive(
        &mut self,
        bytes: &[u8],
        now_ms: u64,
        handler: &mut impl AdmissionHandler,
    ) -> Result<Receive, ConnectionError> {
        self.receive_mode::<false>(bytes, now_ms, handler)
    }
    /// Reassemble into this owner without retaining the receive frame during dispatch.
    /// Call `dispatch_received` after returning from the receive helper in the same
    /// serialized executor pass; later passes must use `poll_pending` to check time.
    /// # Errors
    /// Same framing, scope and backpressure failures as `receive`.
    pub fn receive_staged(
        &mut self,
        bytes: &[u8],
        now_ms: u64,
        handler: &mut impl AdmissionHandler,
    ) -> Result<Receive, ConnectionError> {
        self.receive_mode::<true>(bytes, now_ms, handler)
    }
    fn receive_mode<const STAGED: bool>(
        &mut self,
        bytes: &[u8],
        now_ms: u64,
        handler: &mut impl AdmissionHandler,
    ) -> Result<Receive, ConnectionError> {
        if self.closed {
            return Err(ConnectionError::Closed);
        }
        self.tick(now_ms, handler)?;
        if self.tx.is_some() || self.waiting_handler || self.waiting_event {
            return Err(ConnectionError::Busy);
        }
        if let Some(channel) = self.channel {
            channel.begin_request();
        }
        let result = self.receive_inner::<STAGED>(bytes, now_ms, handler);
        match result {
            Ok(value) => Ok(value),
            Err(error) => {
                self.terminate(handler)?;
                Err(error)
            }
        }
    }
    fn receive_inner<const STAGED: bool>(
        &mut self,
        bytes: &[u8],
        now: u64,
        handler: &mut impl AdmissionHandler,
    ) -> Result<Receive, ConnectionError> {
        // Reassembler validates negotiated receive limits again even for public Frame construction.
        let frame = Frame::decode(bytes, self.rx_frame_max, LOGICAL_MAX)
            .map_err(ConnectionError::Protocol)?;
        if self
            .rx
            .push(frame, now)
            .map_err(ConnectionError::Protocol)?
            == Progress::Pending
        {
            return Ok(Receive::Pending);
        }
        self.request_started = Some(now);
        if self.channel.is_some_and(SharedControlChannel::event_active) {
            self.waiting_event = true;
            return Ok(Receive::Deferred);
        }
        if STAGED {
            self.waiting_handler = true;
            Ok(Receive::Deferred)
        } else {
            self.dispatch_complete(handler)
        }
    }
    fn dispatch_complete(
        &mut self,
        handler: &mut impl AdmissionHandler,
    ) -> Result<Receive, ConnectionError> {
        self.waiting_event = false;
        let request = self.rx.message().map_err(ConnectionError::Protocol)?;
        let request_header = request.header;
        let length =
            match self
                .session
                .dispatch_with_typed_handler(request, handler, &mut self.tx_bytes)
            {
                Ok(length) => length,
                Err(SessionError::Deferred) => {
                    self.waiting_handler = true;
                    return Ok(Receive::Deferred);
                }
                Err(error) => return Err(ConnectionError::Session(error)),
            };
        let header = Header {
            kind: Kind::Response,
            module: request_header.module,
            opcode: request_header.opcode,
            message_id: request_header.message_id,
            offset: 0,
            total_length: u16::try_from(length).map_err(|_| ConnectionError::Capacity)?,
        };
        self.waiting_handler = false;
        if self
            .channel
            .is_some_and(|channel| !channel.begin_response(self.session.active_limits()))
        {
            return Err(ConnectionError::InvalidState);
        }
        self.tx = Some(Transmit {
            header,
            offset: 0,
            length,
            prepared: false,
            callback_due: header.message_id != self.last_admitted
                && header.module != 0
                && self.tx_bytes.get(..2) == Some(&[0, 0]),
        });
        Ok(Receive::ResponseReady)
    }
    /// Whether a control event or handler gate holds the complete request without a wire response.
    #[must_use]
    pub const fn waiting_for_handler(&self) -> bool {
        self.waiting_handler || self.waiting_event
    }
    /// Poll the same deferred request once; no sleep, RTOS or busy wait is performed.
    /// # Errors
    /// Rejects missing pending work, closes on deadline, scope or real backend failures.
    pub fn poll_pending(
        &mut self,
        now_ms: u64,
        handler: &mut impl AdmissionHandler,
    ) -> Result<Receive, ConnectionError> {
        self.tick(now_ms, handler)?;
        self.dispatch_received(handler)
    }
    /// Dispatch the request staged earlier in this same serialized executor pass.
    /// This does not advance time again; use `poll_pending` after yielding or waiting.
    /// # Errors
    /// Missing staging, closed scope and handler failures remain explicit.
    pub fn dispatch_received(
        &mut self,
        handler: &mut impl AdmissionHandler,
    ) -> Result<Receive, ConnectionError> {
        if self.closed {
            return Err(ConnectionError::Closed);
        }
        if self.channel.is_some_and(SharedControlChannel::is_closed) {
            self.terminate(handler)?;
            return Err(ConnectionError::Closed);
        }
        if !self.waiting_handler && !self.waiting_event {
            return Err(ConnectionError::InvalidState);
        }
        if self.waiting_event && self.channel.is_some_and(SharedControlChannel::event_active) {
            return Ok(Receive::Deferred);
        }
        match self.dispatch_complete(handler) {
            Ok(v) => Ok(v),
            Err(error) => {
                self.terminate(handler)?;
                Err(error)
            }
        }
    }
    /// Advance silent partial-request and response/drain deadlines on the application executor.
    /// Call before queue attempts even when no new GATT packet arrives.
    /// # Errors
    /// Deadline expiry or backwards adapter time closes work and reports cleanup failures explicitly.
    pub fn tick(
        &mut self,
        now_ms: u64,
        handler: &mut impl AdmissionHandler,
    ) -> Result<(), ConnectionError> {
        if self.closed {
            return Err(ConnectionError::Closed);
        }
        if self.channel.is_some_and(SharedControlChannel::is_closed) {
            self.terminate(handler)?;
            return Err(ConnectionError::Closed);
        }
        let error = if self.last_now.is_some_and(|previous| now_ms < previous) {
            Some(ProtocolError::InvalidState)
        } else if self
            .request_started
            .is_some_and(|start| now_ms.saturating_sub(start) > self.response_budget)
        {
            Some(ProtocolError::Timeout)
        } else {
            self.rx.advance(now_ms).err()
        };
        if let Some(error) = error {
            self.terminate(handler)?;
            return Err(ConnectionError::Protocol(error));
        }
        self.last_now = Some(now_ms);
        if let Err(error) = handler.advance(now_ms) {
            self.terminate(handler)?;
            return Err(ConnectionError::Session(error));
        }
        if handler.must_end_session() {
            self.terminate(handler)?;
            return Err(ConnectionError::Session(SessionError::AuthorizationRevoked));
        }
        Ok(())
    }
    /// Whether a response is still waiting for fragment queue admission.
    #[must_use]
    pub const fn response_pending(&self) -> bool {
        self.tx.is_some()
    }
    /// Copy current fragment without advancing. A full FIFO may retry the exact same bytes.
    /// Recheck trusted scope before every attempt; the adapter must recheck under queue admission too.
    /// # Errors
    /// Returns capacity/no response, or closes on live scope revocation.
    pub fn next_frame(
        &mut self,
        handler: &mut impl AdmissionHandler,
        out: &mut [u8],
    ) -> Result<usize, ConnectionError> {
        let result = self.prepare_frame(handler, out);
        if result.is_err() {
            out.fill(0);
        }
        result
    }
    fn prepare_frame(
        &mut self,
        handler: &mut impl AdmissionHandler,
        out: &mut [u8],
    ) -> Result<usize, ConnectionError> {
        if self.closed {
            return Err(ConnectionError::Closed);
        }
        if self.channel.is_some_and(SharedControlChannel::is_closed) {
            self.terminate(handler)?;
            return Err(ConnectionError::Closed);
        }
        let tx = self.tx.ok_or(ConnectionError::InvalidState)?;
        if !self.authorized(handler)? {
            self.terminate(handler)?;
            return Err(ConnectionError::Session(SessionError::AuthorizationRevoked));
        }
        let end = (tx.offset + self.tx_frame_max - HEADER_LENGTH).min(tx.length);
        let mut header = tx.header;
        header.offset = u16::try_from(tx.offset).map_err(|_| ConnectionError::Capacity)?;
        if let Some(pending) = self.tx.as_mut() {
            pending.prepared = false;
        }
        let length = Frame {
            header,
            payload: &self.tx_bytes[tx.offset..end],
        }
        .encode(out)
        .map_err(|e| {
            if e == ProtocolError::Capacity {
                ConnectionError::Capacity
            } else {
                ConnectionError::Protocol(e)
            }
        })?;
        if let Some(pending) = self.tx.as_mut() {
            pending.prepared = true;
        }
        Ok(length)
    }
    /// Confirm that the adapter admitted the prepared fragment into its real notification queue.
    /// This must never be called for FIFO-full attempts. It does not mean peer receipt or persistence.
    /// # Errors
    /// Rejects missing preparation; closes on scope revocation or deferred-operation failure.
    pub fn admit_frame(
        &mut self,
        handler: &mut impl AdmissionHandler,
    ) -> Result<(), ConnectionError> {
        if self.closed {
            return Err(ConnectionError::Closed);
        }
        if self.channel.is_some_and(SharedControlChannel::is_closed) {
            self.terminate(handler)?;
            return Err(ConnectionError::Closed);
        }
        let tx = self.tx.ok_or(ConnectionError::InvalidState)?;
        if !tx.prepared {
            return Err(ConnectionError::InvalidState);
        }
        if !self.authorized(handler)? {
            self.terminate(handler)?;
            return Err(ConnectionError::Session(SessionError::AuthorizationRevoked));
        }
        let end = (tx.offset + self.tx_frame_max - HEADER_LENGTH).min(tx.length);
        if end < tx.length {
            if let Some(p) = self.tx.as_mut() {
                p.offset = end;
                p.prepared = false;
            }
            return Ok(());
        }
        if tx.callback_due {
            let request = self.rx.message().map_err(ConnectionError::Protocol)?;
            if let Err(error) = handler.response_admitted(request, &self.tx_bytes[..tx.length]) {
                self.terminate(handler)?;
                return Err(ConnectionError::Session(error));
            }
        }
        if let Some(channel) = self.channel {
            channel.finish_response();
        }
        self.last_admitted = tx.header.message_id;
        self.tx = None;
        self.request_started = None;
        self.tx_bytes.fill(0);
        self.rx.reset();
        Ok(())
    }
    /// Disable notifications or end the physical connection; both require a fresh application connection.
    /// # Errors
    /// Backend cleanup failures are reported while state remains permanently closed.
    pub fn close(&mut self, handler: &mut impl AdmissionHandler) -> Result<(), ConnectionError> {
        self.terminate(handler)
    }
    fn authorized(&self, handler: &impl AdmissionHandler) -> Result<bool, ConnectionError> {
        let request = self.rx.message().map_err(ConnectionError::Protocol)?;
        if handler.must_end_session() {
            return Ok(false);
        }
        // All wire failures are exactly the public status prefix; no protected
        // value or successful operation is released when sending that prefix.
        if self
            .tx
            .is_some_and(|tx| tx.length == 4 && self.tx_bytes[..2] != [0, 0])
        {
            return Ok(true);
        }
        let public_core = request.header.module == 0 && !matches!(request.header.opcode, 7 | 8);
        let allowed = public_core || handler.authorized(request);
        let context_valid = public_core
            || self
                .tx
                .is_some_and(|tx| handler.replay_valid(request, &self.tx_bytes[..tx.length]));
        Ok(allowed && context_valid && !handler.must_end_session())
    }
    fn terminate(&mut self, handler: &mut impl AdmissionHandler) -> Result<(), ConnectionError> {
        if self.closed {
            if self.cleanup_pending.is_some() {
                self.cleanup_pending = handler.connection_closed().err();
            }
            return self
                .cleanup_pending
                .map_or(Ok(()), |error| Err(ConnectionError::CleanupFailed(error)));
        }
        self.closed = true;
        if let Some(channel) = self.channel {
            channel.close();
        }
        self.waiting_handler = false;
        self.waiting_event = false;
        self.request_started = None;
        self.session.disconnect();
        self.tx = None;
        self.tx_bytes.fill(0);
        self.rx.reset();
        self.cleanup_pending = handler.connection_closed().err();
        self.cleanup_pending
            .map_or(Ok(()), |error| Err(ConnectionError::CleanupFailed(error)))
    }
}

mod queue;
pub use queue::{FirstFragmentQueue, FirstSlotError, FirstSlotOutcome};

mod body;
pub use body::{BodyStorage, CopiedBody, RetainedBody, RetainedEventSource};
mod pump;
pub use pump::{
    EventOffer, EventPlane, EventPump, EventRoute, EventSource, PumpError, PumpFailure,
    SourceFailure,
};

impl<const N: usize> Drop for BoundedControlConnection<'_, N> {
    fn drop(&mut self) {
        if let Some(channel) = self.channel {
            channel.close();
        }
    }
}
