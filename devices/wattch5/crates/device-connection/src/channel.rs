//! One physical connection's control logical-message ownership.
use core::cell::Cell;
use device_protocol::{ModuleEntry, ProtocolLimits};
#[derive(Clone, Copy, PartialEq, Eq)]
enum Transmit {
    Idle,
    Response,
    Event,
}
/// External one-connection owner. It is deliberately not Sync and cannot be reopened.
/// Use exactly one owner for one physical connection and serialize real FIFO admission.
/// ```compile_fail
/// use device_connection::SharedControlChannel;
/// fn require_sync<T: Sync>() {}
/// require_sync::<SharedControlChannel>();
/// ```
pub struct SharedControlChannel {
    bound: Cell<bool>,
    closed: Cell<bool>,
    request: Cell<bool>,
    transmit: Cell<Transmit>,
    limits: Cell<Option<ProtocolLimits>>,
}
impl core::fmt::Debug for SharedControlChannel {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SharedControlChannel")
            .field("closed", &self.closed.get())
            .finish_non_exhaustive()
    }
}
impl Default for SharedControlChannel {
    fn default() -> Self {
        Self::new()
    }
}
impl SharedControlChannel {
    /// Construct a fresh owner. A bound owner can never issue a second pair.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            bound: Cell::new(false),
            closed: Cell::new(false),
            request: Cell::new(false),
            transmit: Cell::new(Transmit::Idle),
            limits: Cell::new(None),
        }
    }
    /// Mute both halves permanently. Explicit half-close still performs backend cleanup.
    pub fn close(&self) {
        self.closed.set(true);
        self.limits.set(None);
        self.request.set(false);
        self.transmit.set(Transmit::Idle);
    }
    /// Whether the physical connection context is permanently closed.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.closed.get()
    }
    pub(crate) fn bind(&self) -> bool {
        if self.bound.get() || self.closed.get() {
            false
        } else {
            self.bound.set(true);
            true
        }
    }
    pub(crate) fn begin_request(&self) {
        self.request.set(true);
    }
    pub(crate) fn event_active(&self) -> bool {
        self.transmit.get() == Transmit::Event
    }
    pub(crate) fn begin_response(&self, limits: Option<ProtocolLimits>) -> bool {
        if self.closed.get() || self.transmit.get() != Transmit::Idle {
            return false;
        }
        self.transmit.set(Transmit::Response);
        self.limits.set(limits);
        true
    }
    pub(crate) fn finish_response(&self) {
        self.transmit.set(Transmit::Idle);
        self.request.set(false);
    }
    pub(crate) fn limits(&self) -> Option<ProtocolLimits> {
        self.limits.get()
    }
    pub(crate) fn event_ready(&self) -> bool {
        !self.closed.get()
            && self.limits.get().is_some()
            && !self.request.get()
            && self.transmit.get() == Transmit::Idle
    }
    pub(crate) fn begin_event(&self) -> bool {
        if self.event_ready() {
            self.transmit.set(Transmit::Event);
            true
        } else {
            false
        }
    }
    pub(crate) fn finish_event(&self) {
        if self.transmit.get() == Transmit::Event {
            self.transmit.set(Transmit::Idle);
        }
    }
}
/// Unique non-Clone event-sender capability, issued only by paired control construction.
/// Its catalogue and live HELLO limits always come from that same control connection.
///
/// ```compile_fail
/// use device_connection::EventChannel;
/// fn duplicate(token: EventChannel<'_>) { let _ = token.clone(); }
/// ```
/// ```compile_fail
/// use device_connection::{EventChannel, SharedControlChannel};
/// fn manufacture(owner: &SharedControlChannel) { let _ = EventChannel { owner }; }
/// ```
/// ```compile_fail
/// use device_connection::{EventChannel, EventPump, EventSource};
/// fn duplicate<S: EventSource>(token: EventChannel<'_>, first:S, second:S) {
///     let _first = EventPump::new(token, first);
///     let _second = EventPump::new(token, second);
/// }
/// ```
pub struct EventChannel<'a> {
    pub(crate) owner: &'a SharedControlChannel,
    pub(crate) entries: &'a [ModuleEntry],
    pub(crate) initial_limits: ProtocolLimits,
}
impl core::fmt::Debug for EventChannel<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("EventChannel").finish_non_exhaustive()
    }
}
