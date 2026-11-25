//! Two-phase fragment transmission with transport backpressure.
use crate::frame::validate_limits;
use crate::{Frame, HEADER_LENGTH, Header, LOGICAL_MAX, ProtocolError};

/// A borrowed message cursor. Only `advance` acknowledges actual queue admission.
#[derive(Clone)]
pub struct Fragmenter<'a> {
    header: Header,
    body: &'a [u8],
    capacity: usize,
    offset: usize,
    complete: bool,
}
impl core::fmt::Debug for Fragmenter<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Fragmenter")
            .field("header", &self.header)
            .field("body_length", &self.body.len())
            .field("offset", &self.offset)
            .field("complete", &self.complete)
            .finish()
    }
}
impl<'a> Fragmenter<'a> {
    /// Start a logical message at offset zero using a fixed negotiated frame size.
    /// # Errors
    /// Rejects invalid identity, length, start offset or frame maximum.
    pub fn new(header: Header, body: &'a [u8], frame_max: usize) -> Result<Self, ProtocolError> {
        validate_limits(frame_max, LOGICAL_MAX)?;
        if header.offset != 0 || usize::from(header.total_length) != body.len() {
            return Err(ProtocolError::InvalidLength);
        }
        Frame {
            header,
            payload: body,
        }
        .encode(&mut [0; LOGICAL_MAX + HEADER_LENGTH])?;
        Ok(Self {
            header,
            body,
            capacity: frame_max - HEADER_LENGTH,
            offset: 0,
            complete: false,
        })
    }
    /// Encode the current fragment without consuming it. Retry freely on a full FIFO.
    /// # Errors
    /// Returns invalid state after completion or insufficient caller buffer capacity.
    pub fn write_next(&self, out: &mut [u8]) -> Result<usize, ProtocolError> {
        if self.complete {
            return Err(ProtocolError::InvalidState);
        }
        let end = (self.offset + self.capacity).min(self.body.len());
        let mut header = self.header;
        header.offset = u16::try_from(self.offset).map_err(|_| ProtocolError::InvalidLength)?;
        Frame {
            header,
            payload: &self.body[self.offset..end],
        }
        .encode(out)
    }
    /// Acknowledge that the transport admitted the current fragment exactly once.
    /// # Errors
    /// Returns invalid state if the message has already completed.
    pub fn advance(&mut self) -> Result<(), ProtocolError> {
        if self.complete {
            return Err(ProtocolError::InvalidState);
        }
        self.offset = (self.offset + self.capacity).min(self.body.len());
        self.complete = self.offset == self.body.len();
        Ok(())
    }
    /// Whether every fragment, including an empty message's sole header, was admitted.
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        self.complete
    }
}
