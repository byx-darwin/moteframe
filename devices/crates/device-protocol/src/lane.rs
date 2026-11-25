//! Connection-owned DATA exclusivity and drain barriers, independent of BLE queues.
use core::num::NonZeroU64;
/// Kind of exclusive DATA stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataKind {
    /// Persistent record snapshot export.
    History,
    /// Development-only raw capture.
    Raw,
}
/// Failure of exclusive logical DATA admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneError {
    /// Another stream or logical message owns the lane.
    Busy,
    /// Lease or message identity does not match.
    Identity,
    /// A cancelled stream cannot begin new messages.
    Cancelled,
    /// Cancellation barrier requested before cancellation.
    InvalidState,
    /// Identifier space exhausted; reconnect instead of wrapping.
    Exhausted,
    /// Physical connection ended; this owner cannot reopen.
    Closed,
}
/// A lease created only by its connection's lane owner. Copying does not create another owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DataLease {
    connection_identity: NonZeroU64,
    stream_id: u16,
    kind: DataKind,
}
impl DataLease {
    /// Nonzero, connection-nonreused stream identifier.
    #[must_use]
    pub const fn stream_id(self) -> u16 {
        self.stream_id
    }
    /// The admitted kind, used to keep raw/history authorization separate.
    #[must_use]
    pub const fn kind(self) -> DataKind {
        self.kind
    }
}
#[derive(Debug)]
struct Active {
    lease: DataLease,
    in_flight: Option<u16>,
    last_started: u16,
    cancelling: bool,
}
/// Exactly one instance must be owned by each connection runtime and shared by all DATA producers.
/// Stream and message IDs are never reset during a physical connection.
#[derive(Debug)]
pub struct ConnectionDataLane {
    connection_identity: NonZeroU64,
    active: Option<Active>,
    next_stream: u16,
    next_message: u16,
    closed: bool,
}
impl ConnectionDataLane {
    /// Create a fresh physical connection's DATA owner.
    /// The runtime must supply a nonreusing identity while any old connection work can exist.
    /// All history/raw producers for this connection must borrow this same owner.
    #[must_use]
    pub const fn new(connection_identity: NonZeroU64) -> Self {
        Self {
            connection_identity,
            active: None,
            next_stream: 1,
            next_message: 1,
            closed: false,
        }
    }
    /// Whether no stream owns the open lane and its identifier space remains available.
    #[must_use]
    pub const fn available(&self) -> bool {
        !self.closed && self.active.is_none() && self.next_stream != 0 && self.next_message != 0
    }
    /// Obtain the exclusive history/raw lease.
    /// # Errors
    /// Returns busy, closed or exhausted, never reuses a previous stream ID.
    pub fn acquire(&mut self, kind: DataKind) -> Result<DataLease, LaneError> {
        if self.closed {
            return Err(LaneError::Closed);
        }
        if self.active.is_some() {
            return Err(LaneError::Busy);
        }
        if self.next_stream == 0 || self.next_message == 0 {
            return Err(LaneError::Exhausted);
        }
        let lease = DataLease {
            connection_identity: self.connection_identity,
            stream_id: self.next_stream,
            kind,
        };
        self.next_stream = self.next_stream.checked_add(1).unwrap_or(0);
        self.active = Some(Active {
            lease,
            in_flight: None,
            last_started: 0,
            cancelling: false,
        });
        Ok(lease)
    }
    /// Whether this exact lease currently owns the lane.
    #[must_use]
    pub fn owns(&self, lease: DataLease) -> bool {
        !self.closed && self.active.as_ref().is_some_and(|a| a.lease == lease)
    }
    /// Reserve one logical DATA identifier before its first fragment; FIFO retry keeps the same identifier.
    /// Credit must already be reserved by the service. Do not call this again on fragment retries.
    /// # Errors
    /// Rejects nonowners, cancelled/closed lane, an in-flight message or exhausted identifiers.
    pub fn begin_message(&mut self, lease: DataLease) -> Result<u16, LaneError> {
        let next = self.next_message;
        let active = self.active_mut(lease)?;
        if active.cancelling {
            return Err(LaneError::Cancelled);
        }
        if active.in_flight.is_some() {
            return Err(LaneError::Busy);
        }
        if next == 0 {
            return Err(LaneError::Exhausted);
        }
        active.in_flight = Some(next);
        active.last_started = next;
        self.next_message = next.checked_add(1).unwrap_or(0);
        Ok(next)
    }
    /// Confirm all fragments of the currently reserved logical message were transport-admitted.
    /// This does not prove peer receipt or durable storage.
    /// # Errors
    /// Returns identity failure for an absent/different message; the real in-flight state is preserved.
    pub fn finish_message(&mut self, lease: DataLease, message_id: u16) -> Result<(), LaneError> {
        let active = self.active_mut(lease)?;
        if active.in_flight != Some(message_id) {
            return Err(LaneError::Identity);
        }
        active.in_flight = None;
        Ok(())
    }
    /// Stop new admissions. Already-started fragments must drain before a control reply.
    /// Some(0) means no DATA message ever started; None means wait for `finish_message`.
    /// # Errors
    /// Rejects a stale lease or closed connection.
    pub fn cancel(&mut self, lease: DataLease) -> Result<Option<u16>, LaneError> {
        let active = self.active_mut(lease)?;
        active.cancelling = true;
        Ok(if active.in_flight.is_some() {
            None
        } else {
            Some(active.last_started)
        })
    }
    /// Read the final DATA barrier after cancellation and complete drain.
    /// # Errors
    /// Rejects wrong lease, uncancelled stream or unfinished logical message.
    pub fn cancel_barrier(&self, lease: DataLease) -> Result<u16, LaneError> {
        let active = self.active_ref(lease)?;
        if !active.cancelling {
            return Err(LaneError::InvalidState);
        }
        if active.in_flight.is_some() {
            return Err(LaneError::Busy);
        }
        Ok(active.last_started)
    }
    /// Release an ended stream only after its logical message drained.
    /// # Errors
    /// Rejects a stale lease, closed connection or in-flight message.
    pub fn release(&mut self, lease: DataLease) -> Result<(), LaneError> {
        if self.active_ref(lease)?.in_flight.is_some() {
            return Err(LaneError::Busy);
        }
        self.active = None;
        Ok(())
    }
    /// Physically end a connection, discarding work and permanently closing this owner.
    pub fn disconnect(&mut self) {
        self.active = None;
        self.closed = true;
    }
    fn active_mut(&mut self, lease: DataLease) -> Result<&mut Active, LaneError> {
        if self.closed {
            return Err(LaneError::Closed);
        }
        self.active
            .as_mut()
            .filter(|a| a.lease == lease)
            .ok_or(LaneError::Identity)
    }
    fn active_ref(&self, lease: DataLease) -> Result<&Active, LaneError> {
        if self.closed {
            return Err(LaneError::Closed);
        }
        self.active
            .as_ref()
            .filter(|a| a.lease == lease)
            .ok_or(LaneError::Identity)
    }
}

/// Shared single-executor DATA owner for independently borrowed history/raw adapters.
/// Uses checked `RefCell` borrowing, not a heap or a panic-capable borrow. It is deliberately !Sync;
/// ISR access must be serialized by a separately audited board-level adapter.
#[derive(Debug)]
pub struct SharedDataLane {
    owner: core::cell::RefCell<ConnectionDataLane>,
}
impl SharedDataLane {
    /// Create one shared owner for a fresh nonreusing connection identity.
    #[must_use]
    pub const fn new(connection_identity: NonZeroU64) -> Self {
        Self {
            owner: core::cell::RefCell::new(ConnectionDataLane::new(connection_identity)),
        }
    }
    /// Run a short lane operation on the application executor.
    /// # Errors
    /// Reentrant borrowing returns Busy instead of panicking; lane failures propagate explicitly.
    pub fn with_mut<R>(
        &self,
        operation: impl FnOnce(&mut ConnectionDataLane) -> Result<R, LaneError>,
    ) -> Result<R, LaneError> {
        let mut owner = self.owner.try_borrow_mut().map_err(|_| LaneError::Busy)?;
        operation(&mut owner)
    }
    /// Inspect availability without hiding a reentrant borrow error.
    /// # Errors
    /// Returns Busy while another operation holds the owner.
    pub fn available(&self) -> Result<bool, LaneError> {
        Ok(self
            .owner
            .try_borrow()
            .map_err(|_| LaneError::Busy)?
            .available())
    }
}
