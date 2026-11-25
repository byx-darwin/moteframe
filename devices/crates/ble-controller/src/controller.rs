//! Bounded portable link state and acknowledgement accounting.
use crate::{
    Advertisement, ConnectionParameters, FrameKind, LinkBackend, LinkEvent, LinkFrame, OwnedQueue,
    Peer, TxAdmission,
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
pub enum ControllerEvent {
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
        frame: LinkFrame,
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
struct Pending {
    frame: LinkFrame,
    host: bool,
    terminate: bool,
}
#[derive(Clone, Copy, Debug)]
struct Inflight {
    token: u32,
    pending: Pending,
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
    tx: OwnedQueue<Pending, 8>,
    events: OwnedQueue<ControllerEvent, 16>,
    inflight: Option<Inflight>,
    pending_acl: u8,
    version_sent: bool,
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
            tx: OwnedQueue::new(),
            events: OwnedQueue::new(),
            inflight: None,
            pending_acl: 0,
            version_sent: false,
        }
    }
    pub(crate) const fn initialized(&self) -> bool {
        !matches!(self.state, RunState::Fresh)
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
        if self.pending_acl >= 4 {
            return Err(ControllerError::Capacity);
        }
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
    /// Rejects stale state or insufficient queue capacity.
    pub fn disconnect(
        &mut self,
        id: ConnectionId,
        reason: u8,
    ) -> Result<(), ControllerError<B::Error>> {
        self.check()?;
        if self.connection != Some(id) {
            return Err(ControllerError::State);
        }
        self.control(&[2, reason], true)
    }
    fn emit(&mut self, event: ControllerEvent) -> Result<(), ControllerError<B::Error>> {
        self.events
            .push(event)
            .map_err(|_| ControllerError::Capacity)
    }
    fn control(&mut self, bytes: &[u8], terminate: bool) -> Result<(), ControllerError<B::Error>> {
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
    fn receive_control(&mut self, payload: &[u8]) -> Result<(), ControllerError<B::Error>> {
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
            [0x08 | 0x0e, _, _, _, _, _, _, _, _] => {
                self.control(&[9, 0, 0, 0, 0, 0, 0, 0, 0], false)
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
            [0x0c, _, _, _, _, _] | [0x07, _] | [0x09, _, _, _, _, _, _, _, _] => Ok(()),
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
    fn observe(&mut self, event: LinkEvent) -> Result<(), ControllerError<B::Error>> {
        match event {
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
                    if inflight.pending.terminate {
                        return self.end(0x16);
                    }
                    if inflight.pending.host {
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
            LinkEvent::Received(frame) => {
                if let Some(id) = self.connection {
                    if frame.kind() == FrameKind::Control {
                        self.receive_control(frame.payload())?;
                    } else if !frame.payload().is_empty() {
                        self.emit(ControllerEvent::Acl { id, frame })?;
                    }
                }
                Ok(())
            }
            LinkEvent::Disconnected(reason) => self.end(reason),
            LinkEvent::ParametersUpdated(parameters) => {
                if let Some(id) = self.connection {
                    self.emit(ControllerEvent::ParametersUpdated { id, parameters })?;
                }
                Ok(())
            }
        }
    }
    fn advance(&mut self) -> Result<(), ControllerError<B::Error>> {
        // Keep room for an observation; TX saturation faults rather than hiding ACKs.
        for _ in 0..8 {
            if self.events.is_full() {
                break;
            }
            let Some(event) = self.backend.poll().map_err(ControllerError::Backend)? else {
                break;
            };
            self.observe(event)?;
        }
        if self.connection.is_some()
            && self.inflight.is_none()
            && let Some(pending) = self.tx.front().copied()
        {
            let token = self.token.checked_add(1).ok_or(ControllerError::Capacity)?;
            let frame = pending.frame.with_more_data(self.tx.len() > 1);
            if self
                .backend
                .transmit(token, frame)
                .map_err(ControllerError::Backend)?
                == TxAdmission::Committed
            {
                self.token = token;
                self.tx.pop();
                self.inflight = Some(Inflight { token, pending });
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
