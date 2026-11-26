//! Bounded portable link state and acknowledgement accounting.
use crate::{
    Advertisement, ConnectionParameters, FrameKind, LinkBackend, LinkEvent, LinkFrame, Peer,
    TxAdmission,
};

#[cfg(not(feature = "data-length"))]
use crate::OwnedQueue;
#[cfg(feature = "data-length")]
use crate::dle_queues::{EventQueue, TxFragment, TxQueue};
#[cfg(feature = "data-length")]
use crate::{
    DataLength,
    data_length::{LengthState, Procedure},
};
/// Bluetooth version identity supplied by the integrator, never a product service.
#[derive(Clone, Copy, Debug)]
pub struct Version {
    number: u8,
    company: u16,
    subversion: u16,
}
impl Version {
    /// Creates the identity used in an LL version response.
    #[must_use]
    pub const fn new(number: u8, company: u16, subversion: u16) -> Self {
        Self {
            number,
            company,
            subversion,
        }
    }
}
#[cfg(feature = "data-length")]
type EventFrame = ();
#[cfg(not(feature = "data-length"))]
type EventFrame = LinkFrame;

/// A connection handle plus local generation, preventing stale queued work.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConnectionId {
    handle: u16,
    generation: u32,
}
impl ConnectionId {
    /// HCI handle for this active generation.
    #[must_use]
    pub const fn handle(self) -> u16 {
        self.handle
    }
}
/// Host-facing observations, before HCI encoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControllerEvent<F = LinkFrame> {
    /// One peripheral connection became active.
    Connected {
        /// Connection identity.
        id: ConnectionId,
        /// Peer identity.
        peer: Peer,
        /// Applied timing.
        parameters: ConnectionParameters,
    },
    /// Host ACL fragment received from the active connection.
    Acl {
        /// Connection identity.
        id: ConnectionId,
        /// Complete owned link frame.
        frame: F,
    },
    /// Number of host ACL packets acknowledged, never FIFO admissions.
    Completed {
        /// Connection identity.
        id: ConnectionId,
        /// Completed host packet count.
        packets: u16,
    },
    /// Connection ended and all its queued work was invalidated.
    Disconnected {
        /// Connection identity.
        id: ConnectionId,
        /// HCI reason.
        reason: u8,
    },
    /// Negotiation changed effective data octets or times.
    #[cfg(feature = "data-length")]
    DataLengthChanged {
        /// Active connection identity.
        id: ConnectionId,
        /// Effective limits in peripheral order.
        limits: DataLength,
    },
    /// The PHY changed at the scheduled connection event.
    PhyUpdated {
        /// Active connection identity.
        id: ConnectionId,
        /// Peripheral transmit PHY, in HCI encoding.
        tx: u8,
        /// Peripheral receive PHY, in HCI encoding.
        rx: u8,
    },
    /// Hardware applied new parameters at the scheduled instant.
    ParametersUpdated {
        /// Connection identity.
        id: ConnectionId,
        /// Applied timing.
        parameters: ConnectionParameters,
    },
}
/// Controller failure; hardware uncertainty latches until reset succeeds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ControllerError<E> {
    /// Underlying backend failure.
    Backend(E),
    /// Successful reset is required.
    Faulted,
    /// Operation is invalid for the current state or connection.
    State,
    /// Static storage or transmit tokens have been exhausted.
    Capacity,
    /// Unexpected or malformed link procedure.
    Protocol,
}
#[derive(Clone, Copy, Debug)]
#[cfg(not(feature = "data-length"))]
struct Pending {
    frame: LinkFrame,
    host: bool,
    terminate: bool,
}
#[derive(Clone, Copy, Debug)]
struct Inflight {
    token: u32,
    host: bool,
    terminate: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RunState {
    Fresh,
    Ready,
    Advertising,
    Faulted,
}
/// Serialized, single-peripheral Controller with four host ACL credits.
#[derive(Debug)]
pub struct Controller<B: LinkBackend> {
    backend: B,
    version: Version,
    state: RunState,
    connection: Option<ConnectionId>,
    generation: u32,
    next_handle: u16,
    token: u32,
    #[cfg(not(feature = "data-length"))]
    tx: OwnedQueue<Pending, 8>,
    #[cfg(feature = "data-length")]
    tx: TxQueue,
    #[cfg(not(feature = "data-length"))]
    events: OwnedQueue<ControllerEvent, 16>,
    #[cfg(feature = "data-length")]
    events: EventQueue,
    inflight: Option<Inflight>,
    pending_acl: u8,
    version_sent: bool,
    #[cfg(feature = "data-length")]
    lengths: LengthState,
    #[cfg(feature = "data-length")]
    pub(crate) suggested_length: DataLength,
}
impl<B: LinkBackend> Controller<B> {
    /// Owns the backend; call reset before the first operation.
    #[must_use]
    pub const fn new(backend: B, version: Version) -> Self {
        Self {
            backend,
            version,
            state: RunState::Fresh,
            connection: None,
            generation: 0,
            next_handle: 1,
            token: 0,
            #[cfg(not(feature = "data-length"))]
            tx: OwnedQueue::new(),
            #[cfg(feature = "data-length")]
            tx: TxQueue::new(),
            #[cfg(not(feature = "data-length"))]
            events: OwnedQueue::new(),
            #[cfg(feature = "data-length")]
            events: EventQueue::new(),
            inflight: None,
            pending_acl: 0,
            version_sent: false,
            #[cfg(feature = "data-length")]
            lengths: LengthState::new(DataLength::MAX),
            #[cfg(feature = "data-length")]
            suggested_length: DataLength::MAX,
        }
    }
    pub(crate) const fn initialized(&self) -> bool {
        !matches!(self.state, RunState::Fresh)
    }
    #[cfg(feature = "external-wake")]
    pub(crate) const fn faulted(&self) -> bool {
        matches!(self.state, RunState::Faulted)
    }
    #[cfg(feature = "external-wake")]
    pub(crate) const fn has_events(&self) -> bool {
        !self.events.is_empty()
    }
    #[cfg(feature = "external-wake")]
    pub(crate) const fn has_acl_room(&self) -> bool {
        self.pending_acl < 4 && !self.tx.is_full()
    }
    pub(crate) fn le_features(&self) -> [u8; 8] {
        let capable = u8::from(self.backend.supports_two_m());
        let dle = self.supports_data_length();
        [
            (capable | u8::from(dle)) << 3 | (u8::from(dle) << 5),
            capable,
            0,
            0,
            0,
            0,
            0,
            0,
        ]
    }
    pub(crate) fn supports_data_length(&self) -> bool {
        cfg!(feature = "data-length")
            && self.backend.supports_data_length()
            && self.backend.data_length_clock().is_some()
    }

    /// Backend access for board scheduling; do not mutate active hardware outside this Controller.
    pub fn backend_mut(&mut self) -> &mut B {
        &mut self.backend
    }
    /// Active connection identity.
    #[must_use]
    pub const fn connection(&self) -> Option<ConnectionId> {
        self.connection
    }
    /// Whether advertising is currently active.
    #[must_use]
    pub const fn is_advertising(&self) -> bool {
        matches!(self.state, RunState::Advertising)
    }
    /// Host packets accepted but not yet acknowledged over the air.
    #[must_use]
    pub const fn pending_acl(&self) -> u8 {
        self.pending_acl
    }
    /// Removes the oldest owned host-facing observation.
    pub fn next_event(&mut self) -> Option<ControllerEvent> {
        self.events.pop()
    }
    /// Resets hardware, state and queues, preserving monotonic transmit tokens.
    /// # Errors
    /// Returns a backend error or generation exhaustion and remains faulted.
    pub fn reset(&mut self) -> Result<(), ControllerError<B::Error>> {
        self.state = RunState::Faulted;
        self.backend.reset().map_err(ControllerError::Backend)?;
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or(ControllerError::Capacity)?;
        #[cfg(feature = "data-length")]
        {
            self.suggested_length = DataLength::MAX;
        }
        self.clear_link();
        self.events.clear();

        self.state = RunState::Ready;
        Ok(())
    }
    fn clear_link(&mut self) {
        self.connection = None;
        self.state = RunState::Ready;
        self.tx.clear();
        self.inflight = None;
        self.pending_acl = 0;
        self.version_sent = false;
        #[cfg(feature = "data-length")]
        {
            self.lengths = LengthState::new(self.suggested_length);
        }
    }
    pub(crate) fn check(&self) -> Result<(), ControllerError<B::Error>> {
        if self.state == RunState::Faulted {
            Err(ControllerError::Faulted)
        } else if self.state == RunState::Fresh {
            Err(ControllerError::State)
        } else {
            Ok(())
        }
    }
    /// Applies a complete advertiser configuration.
    /// # Errors
    /// Rejects active connections or a fault, and latches hardware failures.
    pub fn advertise(
        &mut self,
        advertisement: Advertisement,
    ) -> Result<(), ControllerError<B::Error>> {
        self.check()?;
        if self.connection.is_some() || self.is_advertising() {
            return Err(ControllerError::State);
        }
        if let Err(e) = self.backend.advertise(&advertisement) {
            self.state = RunState::Faulted;
            return Err(ControllerError::Backend(e));
        }
        self.state = RunState::Advertising;
        Ok(())
    }
    /// Stops advertising. It does not silently disconnect an active connection.
    /// # Errors
    /// Rejects connected/faulted states and latches hardware failures.
    pub fn stop_advertising(&mut self) -> Result<(), ControllerError<B::Error>> {
        self.check()?;
        if self.connection.is_some() {
            return Err(ControllerError::State);
        }
        if let Err(e) = self.backend.stop() {
            self.state = RunState::Faulted;
            return Err(ControllerError::Backend(e));
        }
        self.state = RunState::Ready;
        Ok(())
    }
    /// Accepts one host ACL fragment, retaining ownership through FIFO backpressure.
    /// # Errors
    /// Rejects stale handles, control PDUs, faults and exhausted host credits.
    pub fn send_acl(
        &mut self,
        id: ConnectionId,
        frame: LinkFrame,
    ) -> Result<(), ControllerError<B::Error>> {
        self.check()?;
        if self.connection != Some(id) || frame.kind() == FrameKind::Control {
            return Err(ControllerError::State);
        }
        if frame.payload().len() > 27 && !self.supports_data_length() {
            return Err(ControllerError::Protocol);
        }
        if self.pending_acl >= 4 {
            return Err(ControllerError::Capacity);
        }
        #[cfg(feature = "data-length")]
        self.tx
            .push_host(frame)
            .map_err(|_| ControllerError::Capacity)?;
        #[cfg(not(feature = "data-length"))]
        self.tx
            .push(Pending {
                frame,
                host: true,
                terminate: false,
            })
            .map_err(|_| ControllerError::Capacity)?;
        self.pending_acl += 1;
        Ok(())
    }
    /// Queues link termination; completion waits for an actual peer ACK.
    /// # Errors
    /// Rejects stale state. A full FIFO yields its unsent entries to termination.
    pub fn disconnect(
        &mut self,
        id: ConnectionId,
        reason: u8,
    ) -> Result<(), ControllerError<B::Error>> {
        self.check()?;
        if self.connection != Some(id) {
            return Err(ControllerError::State);
        }
        if self.tx.is_full() {
            // The descriptor already in flight retains its token. Unsent ACLs
            // receive no completion credit and are retired with this connection.
            self.tx.clear();
        }
        self.control(&[2, reason], true)
    }
    fn emit(
        &mut self,
        event: ControllerEvent<EventFrame>,
    ) -> Result<(), ControllerError<B::Error>> {
        self.events
            .push(event)
            .map_err(|_| ControllerError::Capacity)
    }
    fn control(&mut self, bytes: &[u8], terminate: bool) -> Result<(), ControllerError<B::Error>> {
        #[cfg(feature = "data-length")]
        {
            self.tx
                .push_control(bytes, terminate)
                .map_err(|_| ControllerError::Capacity)
        }
        #[cfg(not(feature = "data-length"))]
        {
            let frame =
                LinkFrame::new(FrameKind::Control, bytes).map_err(|_| ControllerError::Protocol)?;
            self.tx
                .push(Pending {
                    frame,
                    host: false,
                    terminate,
                })
                .map_err(|_| ControllerError::Capacity)
        }
    }
    fn send_features(
        &mut self,
        opcode: u8,
        peer_first_octet: u8,
    ) -> Result<(), ControllerError<B::Error>> {
        let mut pdu = [0; 9];
        pdu[0] = opcode;
        pdu[1..].copy_from_slice(&self.le_features());
        pdu[1] &= peer_first_octet;
        self.control(&pdu, false)
    }

    #[cfg(feature = "data-length")]
    fn apply_length(&mut self) -> Result<(), ControllerError<B::Error>> {
        let limits = self.lengths.local.effective(self.lengths.peer);
        if limits != self.lengths.effective {
            self.lengths.effective = limits;
            self.backend.data_length_updated(limits);
            let id = self.connection.ok_or(ControllerError::State)?;
            self.emit(ControllerEvent::DataLengthChanged { id, limits })?;
        }
        Ok(())
    }

    #[cfg(feature = "data-length")]
    pub(crate) fn set_tx_length(
        &mut self,
        octets: u16,
        time: u16,
    ) -> Result<(), ControllerError<B::Error>> {
        if matches!(
            self.lengths.procedure,
            Procedure::Waiting(_) | Procedure::Unsupported
        ) {
            return Err(ControllerError::State);
        }
        self.lengths.local.tx_octets = octets;
        self.lengths.local.tx_time = time.min(DataLength::MAX.tx_time);
        self.lengths.procedure = Procedure::Wanted;
        Ok(())
    }

    #[cfg(feature = "data-length")]
    fn receive_length(
        &mut self,
        opcode: u8,
        bytes: &[u8],
    ) -> Result<(), ControllerError<B::Error>> {
        if opcode == 0x15
            && (bytes.len() != 8 || !matches!(self.lengths.procedure, Procedure::Waiting(_)))
        {
            return Ok(());
        }
        let Some(peer) = DataLength::from_peer(bytes) else {
            if opcode == 0x14 {
                return self.control(&[0x11, opcode, 0x1e], false);
            }
            self.lengths.procedure = Procedure::Idle;
            return Ok(());
        };
        if opcode == 0x14 {
            self.control(&self.lengths.local.control(0x15), false)?;
        }
        // A simultaneous remote request does not complete our outstanding request.
        if opcode == 0x15 || !matches!(self.lengths.procedure, Procedure::Waiting(_)) {
            self.lengths.procedure = Procedure::Idle;
        }
        self.lengths.peer = peer;
        self.apply_length()
    }

    #[cfg(feature = "data-length")]
    fn late_length_response(&self, payload: &[u8]) -> bool {
        let Procedure::Waiting(start) = self.lengths.procedure else {
            return false;
        };
        let [opcode, parameter, ..] = payload else {
            return false;
        };
        DataLength::is_response_header(*opcode, *parameter, payload.len())
            && self
                .backend
                .data_length_response_clock()
                .or_else(|| self.backend.data_length_clock())
                .is_some_and(|arrival| Self::length_elapsed(start, arrival) > Self::LENGTH_TIMEOUT)
    }

    fn receive_control(&mut self, payload: &[u8]) -> Result<(), ControllerError<B::Error>> {
        #[cfg(feature = "data-length")]
        if self.late_length_response(payload) {
            return self.end(0x22);
        }
        match payload {
            [0x00, ..] if payload.len() == 12 => self
                .backend
                .connection_update(payload)
                .map_err(ControllerError::Backend),
            [0x01, ..] if payload.len() == 8 => self
                .backend
                .channel_map(payload)
                .map_err(ControllerError::Backend),
            [0x02, reason] => self.end(*reason),
            [
                opcode @ (0x08 | 0x09 | 0x0e),
                first,
                features,
                _,
                _,
                _,
                _,
                _,
                _,
            ] => {
                #[cfg(feature = "data-length")]
                if first & 0x20 == 0 && self.supports_data_length() {
                    self.lengths.procedure = Procedure::Unsupported;
                }
                self.backend.peer_two_m(features & 1 != 0);
                if *opcode == 0x09 {
                    Ok(())
                } else {
                    self.send_features(0x09, *first)
                }
            }
            #[cfg(feature = "data-length")]
            [opcode @ (0x14 | 0x15), bytes @ ..] if self.supports_data_length() => {
                self.receive_length(*opcode, bytes)
            }
            #[cfg(feature = "data-length")]
            [0x07, 0x14] | [0x11, 0x14, _] | [0x0d, _]
                if matches!(self.lengths.procedure, Procedure::Waiting(_)) =>
            {
                self.lengths.procedure = if matches!(payload, [0x07, _] | [0x11, _, 0x1a]) {
                    Procedure::Unsupported
                } else {
                    Procedure::Idle
                };
                Ok(())
            }
            [0x07, 0x16] if self.backend.supports_two_m() => {
                self.backend.phy_rejected();
                self.backend.peer_two_m(false);
                Ok(())
            }
            [0x16, tx, rx] if self.backend.supports_two_m() => {
                if tx.trailing_zeros() >= 3 || rx.trailing_zeros() >= 3 {
                    return self.control(&[0x11, 0x16, 0x1e], false);
                }
                let mask = self.backend.phy_response_mask();
                self.control(&[0x17, mask, mask], false)
            }
            [0x18, central_tx, peripheral_tx, lo, hi] if self.backend.supports_two_m() => {
                if self.backend.schedule_phy(
                    u16::from_le_bytes([*lo, *hi]),
                    *central_tx,
                    *peripheral_tx,
                ) {
                    Ok(())
                } else {
                    self.end(0x28)
                }
            }
            [0x11, 0x16, _] | [0x0d, _] if self.backend.supports_two_m() => {
                self.backend.phy_rejected();
                Ok(())
            }
            [0x0c, _, _, _, _, _] if !self.version_sent => {
                let company = self.version.company.to_le_bytes();
                let sub = self.version.subversion.to_le_bytes();
                self.control(
                    &[
                        0x0c,
                        self.version.number,
                        company[0],
                        company[1],
                        sub[0],
                        sub[1],
                    ],
                    false,
                )?;
                self.version_sent = true;
                Ok(())
            }
            [0x0c, _, _, _, _, _] | [0x07, _] => Ok(()),
            [0x00..=0x02 | 0x07..=0x09 | 0x0c | 0x0e, ..] | [] => Err(ControllerError::Protocol),
            [opcode, ..] => self.control(&[7, *opcode], false),
        }
    }
    fn end(&mut self, reason: u8) -> Result<(), ControllerError<B::Error>> {
        if let Some(id) = self.connection {
            self.backend.stop().map_err(ControllerError::Backend)?;
            self.clear_link();
            self.emit(ControllerEvent::Disconnected { id, reason })?;
        }
        Ok(())
    }
    fn observe(&mut self, event: &LinkEvent) -> Result<(), ControllerError<B::Error>> {
        match *event {
            LinkEvent::Connected(peer, parameters) => {
                if !self.is_advertising() || self.connection.is_some() {
                    return Err(ControllerError::Protocol);
                }
                self.generation = self
                    .generation
                    .checked_add(1)
                    .ok_or(ControllerError::Capacity)?;
                let id = ConnectionId {
                    handle: self.next_handle,
                    generation: self.generation,
                };
                self.next_handle = if self.next_handle == 0x0eff {
                    1
                } else {
                    self.next_handle + 1
                };
                #[cfg(feature = "data-length")]
                if self.supports_data_length() {
                    self.lengths = LengthState::new(self.suggested_length);
                    self.backend.data_length_updated(DataLength::LEGACY);
                }
                self.connection = Some(id);
                self.state = RunState::Ready;
                self.emit(ControllerEvent::Connected {
                    id,
                    peer,
                    parameters,
                })
            }
            LinkEvent::Acknowledged(token) => {
                if let Some(inflight) = self.inflight.filter(|i| i.token == token) {
                    self.inflight = None;
                    if inflight.terminate {
                        return self.end(0x16);
                    }
                    if inflight.host {
                        let id = self.connection.ok_or(ControllerError::Protocol)?;
                        self.pending_acl = self
                            .pending_acl
                            .checked_sub(1)
                            .ok_or(ControllerError::Protocol)?;
                        self.emit(ControllerEvent::Completed { id, packets: 1 })?;
                    }
                }
                Ok(())
            }
            LinkEvent::Received(ref frame) => {
                if let Some(id) = self.connection {
                    if frame.kind() == FrameKind::Control {
                        match self.receive_control(frame.payload()) {
                            Err(ControllerError::Capacity) if self.tx.is_full() => {
                                // We cannot retain this mandatory reply. Stop only this
                                // link; advance reserved a Host event for its disconnect.
                                return self.end(0x16);
                            }
                            result => result?,
                        }
                    } else if !frame.payload().is_empty() {
                        #[cfg(feature = "data-length")]
                        if frame.payload().len() > usize::from(self.lengths.effective.rx_octets) {
                            return Err(ControllerError::Protocol);
                        }
                        #[cfg(feature = "data-length")]
                        self.events
                            .push_acl(id, *frame)
                            .map_err(|_| ControllerError::Capacity)?;
                        #[cfg(not(feature = "data-length"))]
                        self.emit(ControllerEvent::Acl { id, frame: *frame })?;
                    }
                }
                Ok(())
            }
            LinkEvent::Disconnected(reason) => self.end(reason),
            LinkEvent::PhyUpdated(tx, rx) => {
                if let Some(id) = self.connection {
                    self.emit(ControllerEvent::PhyUpdated { id, tx, rx })?;
                }
                Ok(())
            }
            LinkEvent::ParametersUpdated(parameters) => {
                if let Some(id) = self.connection {
                    self.emit(ControllerEvent::ParametersUpdated { id, parameters })?;
                }
                Ok(())
            }
        }
    }
    fn queue_local_control(&mut self) -> Result<(), ControllerError<B::Error>> {
        #[cfg(feature = "data-length")]
        if self.supports_data_length() && matches!(self.lengths.procedure, Procedure::Waiting(_)) {
            return Ok(());
        }
        if self.backend.take_feature_query() {
            return self.send_features(0x0e, 0xff);
        }
        #[cfg(feature = "data-length")]
        if self.supports_data_length() && self.lengths.procedure == Procedure::Wanted {
            return Ok(());
        }
        if let Some(mask) = self.backend.take_phy_request() {
            self.control(&[0x16, mask, mask], false)?;
        }
        Ok(())
    }
    #[cfg(feature = "data-length")]
    fn transmit_next(&mut self, fragment_limit: u8) -> Result<(), ControllerError<B::Error>> {
        let request_clock = (self.backend.supports_data_length()
            && self.lengths.procedure == Procedure::Wanted
            && self.tx.is_empty())
        .then(|| self.backend.data_length_clock())
        .flatten();
        let request;
        let fragment = if request_clock.is_some() {
            request = self.lengths.local.control(0x14);
            TxFragment::control(&request, false, false)
        } else if let Some(fragment) = self.tx.next(fragment_limit) {
            fragment
        } else {
            return Ok(());
        };
        let token = self.token.checked_add(1).ok_or(ControllerError::Capacity)?;
        if self
            .backend
            .transmit_payload(token, fragment.kind, fragment.more, fragment.payload)
            .map_err(ControllerError::Backend)?
            == TxAdmission::Committed
        {
            self.token = token;
            self.inflight = Some(Inflight {
                token,
                host: fragment.completes_host,
                terminate: fragment.terminate,
            });
            if let Some(start) = request_clock {
                self.lengths.procedure = Procedure::Waiting(start);
            } else {
                let consumed =
                    u8::try_from(fragment.payload.len()).map_err(|_| ControllerError::Protocol)?;
                self.tx.committed(fragment.completes_host, consumed);
            }
        }
        Ok(())
    }
    // The native clock wraps at 28 bits and ticks once per 312.5 µs.
    #[cfg(feature = "data-length")]
    const LENGTH_TIMEOUT: u32 = 128_000;

    #[cfg(feature = "data-length")]
    const fn length_elapsed(start: u32, sample: u32) -> u32 {
        sample.wrapping_sub(start) & 0x0fff_ffff
    }

    fn advance(&mut self) -> Result<(), ControllerError<B::Error>> {
        // Leave room for the Host observation, including a local disconnect.
        #[cfg(feature = "data-length")]
        let mut event = LinkEvent::Disconnected(0);
        for _ in 0..8 {
            if self.events.is_full() {
                #[cfg(feature = "data-length")]
                self.backend
                    .service_backpressure()
                    .map_err(ControllerError::Backend)?;
                break;
            }
            #[cfg(feature = "data-length")]
            if !self
                .backend
                .poll_into(&mut event)
                .map_err(ControllerError::Backend)?
            {
                break;
            }
            #[cfg(not(feature = "data-length"))]
            let Some(event) = self.backend.poll().map_err(ControllerError::Backend)? else {
                break;
            };
            self.observe(&event)?;
        }
        #[cfg(feature = "data-length")]
        if let Procedure::Waiting(start) = self.lengths.procedure
            && let Some(now) = self.backend.data_length_clock()
            && Self::length_elapsed(start, now) >= Self::LENGTH_TIMEOUT
            && self
                .backend
                .data_length_response_clock()
                .is_none_or(|arrival| Self::length_elapsed(start, arrival) > Self::LENGTH_TIMEOUT)
        {
            // LL response timeout is 40 s, independent of supervision traffic.
            return self.end(0x22);
        }
        if self.connection.is_some() && !self.tx.is_full() {
            self.queue_local_control()?;
        }
        if self.connection.is_none() || self.inflight.is_some() {
            return Ok(());
        }
        let fragment_limit = self.backend.max_acl_fragment_len();
        #[cfg(feature = "data-length")]
        let fragment_limit =
            fragment_limit.min(u8::try_from(self.lengths.effective.tx_octets).unwrap_or(27));
        #[cfg(feature = "data-length")]
        self.transmit_next(fragment_limit)?;
        #[cfg(not(feature = "data-length"))]
        {
            let has_queued_data = self.tx.len() > 1;
            if let Some(queued) = self.tx.front_mut() {
                let mut pending = *queued;
                let remainder = if queued.host {
                    pending.frame.split_off_acl(fragment_limit)
                } else {
                    None
                };
                pending.host &= remainder.is_none();
                let token = self.token.checked_add(1).ok_or(ControllerError::Capacity)?;
                let frame = pending
                    .frame
                    .with_more_data(has_queued_data || remainder.is_some());
                if self
                    .backend
                    .transmit(token, frame)
                    .map_err(ControllerError::Backend)?
                    == TxAdmission::Committed
                {
                    self.token = token;
                    // Retain the original Host credit on the last fragment. A
                    // Busy result leaves the original packet intact for retry.
                    if let Some(remainder) = remainder {
                        queued.frame = remainder;
                    } else {
                        self.tx.pop();
                    }
                    self.inflight = Some(Inflight {
                        token,
                        host: pending.host,
                        terminate: pending.terminate,
                    });
                }
            }
        }
        Ok(())
    }
    /// Services a bounded number of observations and at most one FIFO commit.
    /// # Errors
    /// Any backend/protocol uncertainty latches until successful reset.
    pub fn service(&mut self) -> Result<(), ControllerError<B::Error>> {
        self.check()?;
        let result = self.advance();
        if result.is_err() {
            self.state = RunState::Faulted;
        }
        result
    }
}
