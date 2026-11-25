//! Fixed-capacity, fault-latching receive lifecycle.
use crate::frame::validate_limits;
use crate::{Frame, HEADER_LENGTH, Header, ProtocolError};
/// Result of admitting a fragment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    /// More contiguous fragments are required.
    Pending,
    /// The logical message is available until reset.
    Complete,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Idle,
    Receiving,
    Complete,
    Faulted,
}
/// One logical message buffer. Any failure requires session cleanup and explicit reset.
pub struct Reassembler<const N: usize> {
    bytes: [u8; N],
    header: Option<Header>,
    used: usize,
    frame_max: usize,
    logical_max: usize,
    state: State,
    started: u64,
    last: u64,
    observed: u64,
    budget: u64,
}
impl<const N: usize> Reassembler<N> {
    /// Allocate static capacity and set fixed negotiated limits.
    /// # Errors
    /// Returns invalid length if limits cannot fit this receiver.
    pub fn new(frame_max: usize, logical_max: usize) -> Result<Self, ProtocolError> {
        validate_limits(frame_max, logical_max)?;
        if logical_max > N {
            return Err(ProtocolError::Capacity);
        }
        Ok(Self {
            bytes: [0; N],
            header: None,
            used: 0,
            frame_max,
            logical_max,
            state: State::Idle,
            started: 0,
            last: 0,
            observed: 0,
            budget: 0,
        })
    }
    /// Admit a decoded fragment at a caller-provided monotonic millisecond time.
    /// # Errors
    /// Rejects malformed, interleaved, noncontiguous or late fragments and latches failure.
    pub fn push(&mut self, frame: Frame<'_>, now_ms: u64) -> Result<Progress, ProtocolError> {
        let result = self.push_inner(frame, now_ms);
        if result.is_err() {
            self.state = State::Faulted;
        }
        result
    }
    fn push_inner(&mut self, frame: Frame<'_>, now: u64) -> Result<Progress, ProtocolError> {
        if matches!(self.state, State::Complete | State::Faulted) {
            return Err(ProtocolError::InvalidState);
        }
        frame.validate(self.frame_max, self.logical_max)?;
        if self.state == State::Idle {
            if frame.header.offset != 0 {
                return Err(ProtocolError::Sequence);
            }
            self.header = Some(frame.header);
            self.started = now;
            let fragments = usize::from(frame.header.total_length)
                .div_ceil(self.frame_max - HEADER_LENGTH)
                .max(1);
            self.budget = 2000
                + u64::try_from(fragments - 1).map_err(|_| ProtocolError::InvalidLength)? * 100;
        } else {
            let identity = self.header.ok_or(ProtocolError::InvalidState)?;
            let mut incoming = frame.header;
            incoming.offset = 0;
            if incoming != identity {
                return Err(ProtocolError::Identity);
            }
            self.check_time(now)?;
        }
        if usize::from(frame.header.offset) != self.used {
            return Err(ProtocolError::Sequence);
        }
        let end = self
            .used
            .checked_add(frame.payload.len())
            .ok_or(ProtocolError::Capacity)?;
        if end > N {
            return Err(ProtocolError::Capacity);
        }
        self.bytes[self.used..end].copy_from_slice(frame.payload);
        self.used = end;
        self.last = now;
        self.observed = now;
        if end == usize::from(frame.header.total_length) {
            self.state = State::Complete;
            Ok(Progress::Complete)
        } else {
            self.state = State::Receiving;
            Ok(Progress::Pending)
        }
    }
    /// Advance receive deadlines even when the peer sends no further fragments.
    /// # Errors
    /// A silent timeout, backwards time or prior fault latches the receiver until session reset.
    pub fn advance(&mut self, now_ms: u64) -> Result<(), ProtocolError> {
        if self.state == State::Faulted {
            return Err(ProtocolError::InvalidState);
        }
        if self.state != State::Receiving {
            return Ok(());
        }
        let result = self.check_time(now_ms);
        if result.is_err() {
            self.state = State::Faulted;
        } else {
            self.observed = now_ms;
        }
        result
    }
    fn check_time(&self, now: u64) -> Result<(), ProtocolError> {
        if now < self.observed {
            return Err(ProtocolError::InvalidState);
        }
        let idle = now
            .checked_sub(self.last)
            .ok_or(ProtocolError::InvalidState)?;
        let elapsed = now
            .checked_sub(self.started)
            .ok_or(ProtocolError::InvalidState)?;
        if idle > 2000 || elapsed > self.budget {
            return Err(ProtocolError::Timeout);
        }
        Ok(())
    }
    /// Borrow a complete logical message (not a single transport frame).
    /// # Errors
    /// Returns invalid state until completion or after a receive failure.
    pub fn message(&self) -> Result<Frame<'_>, ProtocolError> {
        if self.state != State::Complete {
            return Err(ProtocolError::InvalidState);
        }
        Ok(Frame {
            header: self.header.ok_or(ProtocolError::InvalidState)?,
            payload: &self.bytes[..self.used],
        })
    }
    /// Discard the buffer only after the caller has ended/discarded old session work.
    pub fn reset(&mut self) {
        self.bytes.fill(0);
        self.header = None;
        self.used = 0;
        self.state = State::Idle;
        self.started = 0;
        self.last = 0;
        self.observed = 0;
        self.budget = 0;
    }
}

impl<const N: usize> core::fmt::Debug for Reassembler<N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Reassembler")
            .field("state", &self.state)
            .field("header", &self.header)
            .field("used", &self.used)
            .field("capacity", &N)
            .finish_non_exhaustive()
    }
}
