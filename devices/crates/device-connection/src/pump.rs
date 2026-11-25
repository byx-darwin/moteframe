//! Bounded event transport: reserve the real first FIFO slot before starting a DATA message.
use crate::{
    BodyStorage, CopiedBody, EventChannel, FirstFragmentQueue, FirstSlotError, FirstSlotOutcome,
    RetainedBody, RetainedEventSource,
};
use core::num::NonZeroU16;
use device_protocol::{
    Frame, HEADER_LENGTH, Header, Kind, ModuleEntry, ProtocolError, ProtocolLimits,
};
/// Physical negotiated event channel, independent of ordinary control-event and DATA ID spaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventPlane {
    /// Ordinary status/business control event.
    Control,
    /// Credit-controlled history/raw DATA event.
    Data,
}
/// Side-effect-free next-event routing metadata from a real typed source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventRoute {
    /// Registered stable module ID.
    pub module: u8,
    /// Exact event opcode.
    pub opcode: u8,
    /// Actual GATT channel required by its contract.
    pub plane: EventPlane,
}
/// Complete typed producer output; DATA identifiers come from the one shared lane owner.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct EventOffer<T> {
    /// Opaque source/context generation used for live admission checks.
    pub token: T,
    /// Complete body length actually encoded in caller storage.
    pub length: usize,
    /// DATA requires its reserved nonzero ID; ordinary events require None.
    pub data_message: Option<NonZeroU16>,
}
impl<T> core::fmt::Debug for EventOffer<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("EventOffer")
            .field("length", &self.length)
            .finish_non_exhaustive()
    }
}
/// Typed producer failure, with no private payload/error text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceFailure {
    /// No complete real event currently exists; no credit/ID was consumed.
    NotReady,
    /// Current resource scope was revoked.
    Denied,
    /// Real complete event does not fit the negotiated carrier.
    Capacity,
    /// Producer identity/schema/lifecycle is invalid.
    Invalid,
    /// Actual backend or cleanup failed.
    Backend,
}
/// Real event producer adapter. Body encoding/provenance/CRC follow its exact typed contract.
/// One serialized actor must own its resource authorization, DATA lane and queue publication.
pub trait EventSource {
    /// Opaque copyable resource/generation identity; no reference into a mutable payload may escape.
    type Token: Copy + Eq;
    /// Peek route only: must not reserve credit, consume IDs or start a logical DATA message.
    /// # Errors
    /// Actual source/context failure must be explicit rather than hidden as no event.
    fn route(&self) -> Result<Option<EventRoute>, SourceFailure>;
    /// Prepare a complete validated body ONLY inside the actual first-frame reservation callback.
    /// DATA sources reserve their shared message ID/whole credit here, never on an `is_ready` precheck.
    /// # Errors
    /// NotReady/capacity must not consume credit/ID. Other failures terminate this connection's work.
    fn prepare(&mut self, out: &mut [u8]) -> Result<EventOffer<Self::Token>, SourceFailure>;
    /// Recheck live scope and exact resource generation before every fragment's real admission.
    fn authorized(&self, token: Self::Token) -> bool;
    /// Whether independent scope/session changes require permanently ending this source connection.
    fn must_end_session(&self) -> bool;
    /// Confirm the actual final fragment's FIFO admission exactly once, never peer durable storage.
    /// # Errors
    /// Real completion/ownership failure is fatal, not a reason to resend the event.
    fn admitted(&mut self, token: Self::Token) -> Result<(), SourceFailure>;
    /// Mute old events, close this same shared DATA owner and attempt all required backend cleanup.
    /// The paired context is muted immediately; explicitly close the control actor/physical link too
    /// so every externally owned handler cleanup result is observed.
    /// # Errors
    /// Actual cleanup failure is retained separately from the primary transport/authorization fault.
    fn connection_closed(&mut self) -> Result<(), SourceFailure>;
}
/// Primary transport failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PumpFailure {
    /// Invalid actual negotiated limits/catalogue.
    Configuration,
    /// A request/response acquired the logical control carrier before this reservation.
    ControlBusy,
    /// This permanently closed sender cannot be reused.
    Closed,
    /// A real source offered an invalid route/ID/length.
    InvalidOffer,
    /// Source authority/generation was revoked.
    AuthorizationRevoked,
    /// Actual queue publication failed or its outcome is uncertain.
    Transport,
    /// Actual FIFO reservation contract could not be upheld.
    QueueContract,
    /// The source returned an actual typed error.
    Source(SourceFailure),
    /// The outgoing frame violated its wire contract.
    Protocol(ProtocolError),
    /// Trusted monotonic input moved backwards.
    Clock,
    /// Partial-message idle/absolute budget expired before its next admission.
    Timeout,
}
/// A primary failure plus any actual cleanup failure; neither result is discarded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PumpError {
    /// Primary cause.
    pub failure: PumpFailure,
    /// Backend cleanup failure after closing, if any.
    pub cleanup: Option<SourceFailure>,
}
#[derive(Clone, Copy)]
struct Pending<T> {
    route: EventRoute,
    token: T,
    length: usize,
    offset: usize,
    message: u16,
    started: u64,
    last: u64,
    budget: u64,
}
type FirstReservation<T> =
    Result<FirstSlotOutcome<(Pending<T>, usize)>, FirstSlotError<PumpFailure>>;
/// One physical connection's sender, permanently bound to one source/aggregator by ownership.
/// There is no API to substitute another source's authorization for a pending private message.
/// One instance allocates ordinary control-event IDs; DATA IDs still belong to the shared lane.
pub struct EventPump<'a, S: EventSource, B: BodyStorage<S> = CopiedBody> {
    entries: &'a [ModuleEntry],
    channel: EventChannel<'a>,
    limits: ProtocolLimits,
    source: S,
    body: B,
    pending: Option<Pending<S::Token>>,
    next_control: u16,
    last_data: u16,
    cleanup_pending: Option<SourceFailure>,
    last_now: Option<u64>,
    closed: bool,
}
impl<S: EventSource, B: BodyStorage<S>> core::fmt::Debug for EventPump<'_, S, B> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("EventPump")
            .field("pending", &self.pending.is_some())
            .field("closed", &self.closed)
            .finish_non_exhaustive()
    }
}
impl<'a, S: EventSource> EventPump<'a, S> {
    /// Construct the compatibility sender owning a full body copy.
    /// # Errors
    /// Invalid negotiated channel/catalogue is rejected.
    pub fn new(channel: EventChannel<'a>, source: S) -> Result<Self, PumpError> {
        Self::with_storage(channel, source, CopiedBody::default())
    }
}
impl<'a, S: RetainedEventSource> EventPump<'a, S, RetainedBody> {
    /// Construct a sender borrowing the source's pinned body without a second copy.
    /// # Errors
    /// Invalid negotiated channel/catalogue is rejected.
    pub fn new_retained(channel: EventChannel<'a>, source: S) -> Result<Self, PumpError> {
        Self::with_storage(channel, source, RetainedBody)
    }
}
impl<'a, S: EventSource, B: BodyStorage<S>> EventPump<'a, S, B> {
    /// Bind one source to the same immutable catalogue and actual active HELLO limits as control.
    /// Shared service actors can be referenced by separate checked handles for control/event routing.
    /// # Errors
    /// Invalid limits, module ordering or encoded metadata.
    pub fn with_storage(channel: EventChannel<'a>, source: S, body: B) -> Result<Self, PumpError> {
        let entries = channel.entries;
        let limits = channel.initial_limits;
        let error = PumpError {
            failure: PumpFailure::Configuration,
            cleanup: None,
        };
        limits.encode(&mut [0; 14]).map_err(|_| error)?;
        if !entries.windows(2).all(|p| p[0].module < p[1].module)
            || entries.iter().any(|e| e.encode(&mut [0; 12]).is_err())
        {
            return Err(error);
        }
        Ok(Self {
            entries,
            channel,
            limits,
            source,
            body,
            pending: None,
            next_control: 1,
            last_data: 0,
            cleanup_pending: None,
            last_now: None,
            closed: false,
        })
    }
    /// Inspect the bound source without allowing it to be replaced mid-message.
    #[must_use]
    pub const fn source(&self) -> &S {
        &self.source
    }
    /// Whether a first fragment entered FIFO and its remaining fragments must drain.
    #[must_use]
    pub const fn pending(&self) -> bool {
        self.pending.is_some()
    }
    fn end(&mut self, failure: PumpFailure) -> PumpError {
        self.closed = true;
        self.channel.owner.close();
        self.pending = None;
        self.body.clear();
        self.cleanup_pending = self.source.connection_closed().err();
        PumpError {
            failure,
            cleanup: self.cleanup_pending,
        }
    }
    /// Attempt at most one real fragment admission without waiting. Pending control responses take
    /// priority over new events; an already-started DATA message still drains during CANCEL/CLOSE.
    /// # Errors
    /// Fatal errors close/mute the bound source and retain cleanup failure. `NotReady` is benign only
    /// when the source consumed no credit/ID. Queue scope/publication must be serialized.
    pub fn flush(
        &mut self,
        now_ms: u64,
        plane: EventPlane,
        queue: &mut impl FirstFragmentQueue,
    ) -> Result<bool, PumpError> {
        if self.closed {
            return Err(PumpError {
                failure: PumpFailure::Closed,
                cleanup: self.cleanup_pending,
            });
        }
        if self.channel.owner.is_closed() {
            return Err(self.end(PumpFailure::Closed));
        }
        if let Err(failure) = self.check(now_ms) {
            return Err(self.end(failure));
        }
        self.last_now = Some(now_ms);
        let Some(limits) = self.channel.owner.limits() else {
            return Ok(false);
        };
        self.limits = limits;
        if let Some(pending) = self.pending {
            if pending.route.plane != plane {
                return Ok(false);
            }
            return self.flush_pending(now_ms, pending, queue);
        }
        self.flush_first(now_ms, plane, queue)
    }
    #[inline(never)]
    fn flush_first(
        &mut self,
        now_ms: u64,
        plane: EventPlane,
        queue: &mut impl FirstFragmentQueue,
    ) -> Result<bool, PumpError> {
        if !self.channel.owner.event_ready() {
            return Ok(false);
        }
        let route = match self.source.route() {
            Ok(Some(route)) => route,
            Ok(None) | Err(SourceFailure::NotReady) => return Ok(false),
            Err(error) => return Err(self.end(PumpFailure::Source(error))),
        };
        if route.plane != plane {
            return Ok(false);
        }
        let limit = match self.route_limit(route) {
            Ok(limit) => limit,
            Err(failure) => return Err(self.end(failure)),
        };
        self.flush_routed_first(now_ms, plane, queue, route, limit)
    }
    #[inline(never)]
    fn flush_routed_first(
        &mut self,
        now_ms: u64,
        plane: EventPlane,
        queue: &mut impl FirstFragmentQueue,
        route: EventRoute,
        limit: usize,
    ) -> Result<bool, PumpError> {
        let mut claimed = false;
        let result = self.reserve_first(now_ms, queue, route, limit, &mut claimed);
        match result {
            Ok(FirstSlotOutcome::NoSlot) | Err(FirstSlotError::Fill(PumpFailure::ControlBusy)) => {
                Ok(false)
            }
            Ok(FirstSlotOutcome::Admitted((pending, end))) => {
                if plane == EventPlane::Control {
                    self.next_control = self.next_control.checked_add(1).unwrap_or(1);
                } else {
                    self.last_data = pending.message;
                }
                self.after_admitted(now_ms, pending, end)
            }
            Err(FirstSlotError::Fill(PumpFailure::Source(SourceFailure::NotReady))) => {
                if claimed {
                    self.channel.owner.finish_event();
                }
                self.body.clear();
                Ok(false)
            }
            Err(error) => Err(self.end(slot_failure(error))),
        }
    }
    // First-frame preparation/encoding and completion validation are serial.
    // Release the reservation callback's validation temporaries before charging
    // the admitted receipt/cleanup path; ownership and FIFO admission stay exact.
    #[inline(never)]
    fn reserve_first(
        &mut self,
        now_ms: u64,
        queue: &mut impl FirstFragmentQueue,
        route: EventRoute,
        limit: usize,
        claimed: &mut bool,
    ) -> FirstReservation<S::Token> {
        let frame_max = usize::from(self.limits.tx_frame_max);
        queue.with_reserved_slot(|slot| {
            if slot.len() < frame_max {
                return Err(PumpFailure::QueueContract);
            }
            if !self.channel.owner.event_ready() {
                return Err(PumpFailure::ControlBusy);
            }
            if route.plane == EventPlane::Control {
                if !self.channel.owner.begin_event() {
                    return Err(PumpFailure::ControlBusy);
                }
                *claimed = true;
            }
            let offer = self
                .body
                .prepare(&mut self.source, limit)
                .map_err(PumpFailure::Source)?;
            if offer.length > limit {
                return Err(PumpFailure::InvalidOffer);
            }
            if self.source.must_end_session() || !self.source.authorized(offer.token) {
                return Err(PumpFailure::AuthorizationRevoked);
            }
            let message = match (route.plane, offer.data_message) {
                (EventPlane::Control, None) => self.next_control,
                (EventPlane::Data, Some(mid)) if mid.get() > self.last_data => mid.get(),
                _ => return Err(PumpFailure::InvalidOffer),
            };
            let fragments = offer.length.div_ceil(frame_max - HEADER_LENGTH).max(1);
            let pending = Pending {
                route,
                token: offer.token,
                length: offer.length,
                offset: 0,
                message,
                started: now_ms,
                last: now_ms,
                budget: 2000
                    + u64::try_from(fragments - 1).map_err(|_| PumpFailure::InvalidOffer)? * 100,
            };
            let (length, end) = self
                .body
                .with_body(&self.source, pending.token, |body| {
                    encode(self.limits, limit, pending, body, slot)
                })
                .map_err(PumpFailure::Source)??;
            Ok((length, (pending, end)))
        })
    }
    #[inline(never)]
    fn check(&self, now: u64) -> Result<(), PumpFailure> {
        if self.last_now.is_some_and(|last| now < last) {
            return Err(PumpFailure::Clock);
        }
        if self.source.must_end_session() {
            return Err(PumpFailure::AuthorizationRevoked);
        }
        if let Some(p) = self.pending {
            if !self.source.authorized(p.token) {
                return Err(PumpFailure::AuthorizationRevoked);
            }
            if now.saturating_sub(p.last) > 2000 || now.saturating_sub(p.started) > p.budget {
                return Err(PumpFailure::Timeout);
            }
        }
        Ok(())
    }
    fn route_limit(&self, route: EventRoute) -> Result<usize, PumpFailure> {
        let is_data = matches!((route.module, route.opcode), (0x10 | 0x70, 0x80));
        if (route.plane == EventPlane::Data) != is_data {
            return Err(PumpFailure::InvalidOffer);
        }
        let entry = self
            .entries
            .iter()
            .find(|e| e.module == route.module && e.availability == 1)
            .ok_or(PumpFailure::InvalidOffer)?;
        let max = match route.plane {
            EventPlane::Control => self.limits.control_max,
            EventPlane::Data => self.limits.data_max,
        };
        if max == 0 {
            return Err(PumpFailure::InvalidOffer);
        }
        Ok(usize::from(entry.output_max.min(max)))
    }
    #[inline(never)]
    fn flush_pending(
        &mut self,
        now: u64,
        p: Pending<S::Token>,
        queue: &mut impl FirstFragmentQueue,
    ) -> Result<bool, PumpError> {
        let limit = match self.route_limit(p.route) {
            Ok(limit) => limit,
            Err(error) => return Err(self.end(error)),
        };
        let result = queue.with_reserved_slot(|slot| {
            if self.source.must_end_session() || !self.source.authorized(p.token) {
                return Err(PumpFailure::AuthorizationRevoked);
            }
            let (length, end) = self
                .body
                .with_body(&self.source, p.token, |body| {
                    encode(self.limits, limit, p, body, slot)
                })
                .map_err(PumpFailure::Source)??;
            Ok((length, end))
        });
        match result {
            Ok(FirstSlotOutcome::NoSlot) => Ok(false),
            Ok(FirstSlotOutcome::Admitted(end)) => self.after_admitted(now, p, end),
            Err(error) => Err(self.end(slot_failure(error))),
        }
    }
    fn after_admitted(
        &mut self,
        now: u64,
        mut pending: Pending<S::Token>,
        end: usize,
    ) -> Result<bool, PumpError> {
        if end == pending.length {
            self.pending = None;
            self.body.clear();
            if let Err(error) = self.source.admitted(pending.token) {
                return Err(self.end(PumpFailure::Source(error)));
            }
            if pending.route.plane == EventPlane::Control {
                self.channel.owner.finish_event();
            }
        } else {
            pending.offset = end;
            pending.last = now;
            self.pending = Some(pending);
        }
        Ok(true)
    }
    /// Close/mute on disconnect or CCCD loss; explicit repeat can retry failed actual cleanup.
    /// # Errors
    /// Source cleanup failed; private bytes remain cleared and old sender stays permanently closed.
    pub fn close(&mut self) -> Result<(), PumpError> {
        if self.closed {
            if self.cleanup_pending.is_some() {
                self.cleanup_pending = self.source.connection_closed().err();
            }
            return if self.cleanup_pending.is_some() {
                Err(PumpError {
                    failure: PumpFailure::Closed,
                    cleanup: self.cleanup_pending,
                })
            } else {
                Ok(())
            };
        }
        let error = self.end(PumpFailure::Closed);
        if error.cleanup.is_some() {
            Err(error)
        } else {
            Ok(())
        }
    }
}
fn encode<T: Copy>(
    limits: ProtocolLimits,
    limit: usize,
    p: Pending<T>,
    body: &[u8],
    out: &mut [u8],
) -> Result<(usize, usize), PumpFailure> {
    if p.length > limit || p.offset > p.length || p.length > body.len() {
        return Err(PumpFailure::InvalidOffer);
    }
    let end = (p.offset + usize::from(limits.tx_frame_max) - HEADER_LENGTH).min(p.length);
    let frame = Frame {
        header: Header {
            kind: Kind::Event,
            module: p.route.module,
            opcode: p.route.opcode,
            message_id: p.message,
            offset: u16::try_from(p.offset).map_err(|_| PumpFailure::InvalidOffer)?,
            total_length: u16::try_from(p.length).map_err(|_| PumpFailure::InvalidOffer)?,
        },
        payload: &body[p.offset..end],
    };
    let length = frame.encode(out).map_err(PumpFailure::Protocol)?;
    Ok((length, end))
}
fn slot_failure(error: FirstSlotError<PumpFailure>) -> PumpFailure {
    match error {
        FirstSlotError::Fill(error) => error,
        FirstSlotError::Transport => PumpFailure::Transport,
        FirstSlotError::Contract => PumpFailure::QueueContract,
    }
}

impl<S: EventSource, B: BodyStorage<S>> Drop for EventPump<'_, S, B> {
    fn drop(&mut self) {
        self.channel.owner.close();
        self.body.clear();
    }
}
