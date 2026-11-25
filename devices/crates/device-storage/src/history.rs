//! History DATA admission and cancellation barriers. No transport or persistent ACK is implied.
/// History lifecycle failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HistoryError {
    /// Invalid snapshot/stream state or exhausted message IDs.
    InvalidState,
    /// A whole logical message is already in flight.
    Busy,
    /// Stream identifier differs from the active stream.
    Identity,
    /// Cumulative credit has been consumed.
    NoCredit,
    /// Nonmonotonic sequence, grant or unadmitted ACK candidate.
    Sequence,
    /// Dataset epoch changed.
    Epoch,
    /// Filtered reads may never advance global durable watermarks.
    FilteredAck,
}
/// Trusted snapshot metadata from the record store, not from peer assertions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Snapshot {
    /// Nonzero snapshot identity.
    pub id: u32,
    /// Stable dataset epoch.
    pub epoch: u32,
    /// First eligible record sequence (zero with last=zero for an empty snapshot).
    pub first: u64,
    /// Last eligible record sequence.
    pub last: u64,
    /// True for any time/type/parent or other selection filter.
    pub filtered: bool,
}
/// Connection-wide nonreusing DATA message-ID allocation.
/// Share this allocator between every history/raw stream in a connection.
#[derive(Debug)]
pub struct DataIds {
    next: u16,
}
impl Default for DataIds {
    fn default() -> Self {
        Self::new()
    }
}
impl DataIds {
    /// Start a fresh connection. This must never be reset during a live connection.
    #[must_use]
    pub const fn new() -> Self {
        Self { next: 1 }
    }
    /// Reserve a fresh DATA identifier.
    /// # Errors
    /// Reconnect is required after exhausting the connection ID space.
    pub fn reserve(&mut self) -> Result<u16, HistoryError> {
        if self.next == 0 {
            return Err(HistoryError::InvalidState);
        }
        let id = self.next;
        self.next = self.next.checked_add(1).unwrap_or(0);
        Ok(id)
    }
}
/// Logical history stream. One stream must be owned exclusively by a connection's DATA lane.
#[derive(Debug)]
pub struct DataStream {
    snapshot: Snapshot,
    id: u16,
    granted: u32,
    reserved: u32,
    in_flight: Option<(u16, u64)>,
    last_started: u16,
    highest_admitted: u64,
    last_sequence: u64,
    cancelled: bool,
}
impl DataStream {
    /// Start a history stream with zero credit. Message IDs always come from a connection-owned allocator.
    /// # Errors
    /// Rejects zero identities, inconsistent empty ranges or reversed ranges.
    pub fn new(snapshot: Snapshot, id: u16) -> Result<Self, HistoryError> {
        if snapshot.id == 0
            || snapshot.epoch == 0
            || id == 0
            || snapshot.first > snapshot.last
            || (snapshot.first == 0) != (snapshot.last == 0)
        {
            return Err(HistoryError::InvalidState);
        }
        Ok(Self {
            snapshot,
            id,
            granted: 0,
            reserved: 0,
            in_flight: None,
            last_started: 0,
            highest_admitted: 0,
            last_sequence: 0,
            cancelled: false,
        })
    }
    /// Accept a cumulative grant, never an incremental addition.
    /// # Errors
    /// Rejects another stream, decreasing grants or ended streams.
    pub fn grant(&mut self, id: u16, cumulative: u32) -> Result<(), HistoryError> {
        if self.cancelled {
            return Err(HistoryError::InvalidState);
        }
        if id != self.id {
            return Err(HistoryError::Identity);
        }
        if cumulative < self.granted {
            return Err(HistoryError::Sequence);
        }
        self.granted = cumulative;
        Ok(())
    }
    /// Reserve shared connection ID and credit once, before sending the first fragment.
    /// FIFO retry operates on that same message and must not call this again.
    /// # Errors
    /// Rejects busy/ended streams, exhausted credit/IDs and invalid record order.
    pub fn begin_record(&mut self, sequence: u64, ids: &mut DataIds) -> Result<u16, HistoryError> {
        self.check_start(sequence)?;
        let id = ids.reserve()?;
        self.start(id, sequence);
        Ok(id)
    }
    fn check_start(&self, sequence: u64) -> Result<(), HistoryError> {
        if self.cancelled {
            return Err(HistoryError::InvalidState);
        }
        if self.in_flight.is_some() {
            return Err(HistoryError::Busy);
        }
        if sequence < self.snapshot.first
            || sequence > self.snapshot.last
            || sequence <= self.last_sequence
        {
            return Err(HistoryError::Sequence);
        }
        if self.reserved >= self.granted {
            return Err(HistoryError::NoCredit);
        }
        Ok(())
    }
    fn start(&mut self, id: u16, sequence: u64) {
        // reserved < granted <= u32::MAX, so the increment cannot wrap.
        self.reserved += 1;
        self.in_flight = Some((id, sequence));
        self.last_started = id;
        self.last_sequence = sequence;
    }
    /// Mark the last fragment as transport-admitted; this is not phone persistence.
    /// # Errors
    /// Rejects a different or absent in-flight message.
    pub fn finish_record(&mut self, id: u16) -> Result<(), HistoryError> {
        let (expected, sequence) = self.in_flight.ok_or(HistoryError::InvalidState)?;
        if expected != id {
            return Err(HistoryError::Identity);
        }
        self.highest_admitted = sequence;
        self.in_flight = None;
        Ok(())
    }
    /// Stop beginning new records; finish any already-started message before replying.
    /// Returns the final DATA message barrier immediately only when no fragment is in flight.
    /// # Errors
    /// None currently; a Result leaves explicit lifecycle evolution possible.
    pub fn cancel(&mut self) -> Result<Option<u16>, HistoryError> {
        self.cancelled = true;
        Ok(if self.in_flight.is_some() {
            None
        } else {
            Some(self.last_started)
        })
    }
    /// Obtain a cancellation reply barrier after the started message has drained.
    /// # Errors
    /// Returns busy until complete, or invalid state before cancellation.
    pub fn cancel_barrier(&self) -> Result<u16, HistoryError> {
        if !self.cancelled {
            return Err(HistoryError::InvalidState);
        }
        if self.in_flight.is_some() {
            return Err(HistoryError::Busy);
        }
        Ok(self.last_started)
    }
    /// Validate a peer's durable-ACK candidate before passing it to the persistent record store.
    /// The store MUST additionally verify a no-omission prefix and atomically persist its watermark.
    /// This method never commits a watermark and cannot prove phone storage durability.
    /// # Errors
    /// Rejects all filtered ACKs, epoch changes, and unadmitted record sequences.
    pub fn ack_candidate(&self, epoch: u32, upto: u64) -> Result<u64, HistoryError> {
        if self.snapshot.filtered {
            return Err(HistoryError::FilteredAck);
        }
        if epoch != self.snapshot.epoch {
            return Err(HistoryError::Epoch);
        }
        if upto < self.snapshot.first || upto > self.highest_admitted || upto == 0 {
            return Err(HistoryError::Sequence);
        }
        Ok(upto)
    }
}
