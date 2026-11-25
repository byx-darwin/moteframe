//! Borrowed, explicitly little-endian frame codec.
use crate::ProtocolError;

/// Number of bytes in every transport header.
pub const HEADER_LENGTH: usize = 10;
/// Maximum logical message supported by wire 1.2.
pub const LOGICAL_MAX: usize = 512;

/// Wire message kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Kind {
    /// Peer command.
    Request = 0,
    /// Command reply, including the common status prefix.
    Response = 1,
    /// Asynchronous control or data event.
    Event = 2,
}

/// Identity and fragment coordinates. Payload length is derived from the GATT value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    /// Message kind.
    pub kind: Kind,
    /// Module identifier.
    pub module: u8,
    /// Module opcode.
    pub opcode: u8,
    /// Nonzero message identifier.
    pub message_id: u16,
    /// Offset within the logical body.
    pub offset: u16,
    /// Full logical body length.
    pub total_length: u16,
}

/// A frame borrowing its payload; Debug deliberately omits its contents.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Frame<'a> {
    /// Fragment header.
    pub header: Header,
    /// Fragment bytes.
    pub payload: &'a [u8],
}
impl core::fmt::Debug for Frame<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Frame")
            .field("header", &self.header)
            .field("payload_length", &self.payload.len())
            .finish()
    }
}

impl<'a> Frame<'a> {
    /// Decode one complete GATT value, enforcing negotiated maxima.
    /// # Errors
    /// Returns a structural error for malformed or unsupported frames.
    pub fn decode(
        bytes: &'a [u8],
        frame_max: usize,
        logical_max: usize,
    ) -> Result<Self, ProtocolError> {
        validate_limits(frame_max, logical_max)?;
        if bytes.len() < HEADER_LENGTH {
            return Err(ProtocolError::Truncated);
        }
        if bytes.len() > frame_max {
            return Err(ProtocolError::InvalidLength);
        }
        if bytes[0] >> 4 != 1 {
            return Err(ProtocolError::InvalidVersion);
        }
        let kind = match bytes[0] & 15 {
            0 => Kind::Request,
            1 => Kind::Response,
            2 => Kind::Event,
            _ => return Err(ProtocolError::InvalidKind),
        };
        if bytes[3] != 0 {
            return Err(ProtocolError::Reserved);
        }
        let frame = Self {
            header: Header {
                kind,
                module: bytes[1],
                opcode: bytes[2],
                message_id: u16::from_le_bytes([bytes[4], bytes[5]]),
                offset: u16::from_le_bytes([bytes[6], bytes[7]]),
                total_length: u16::from_le_bytes([bytes[8], bytes[9]]),
            },
            payload: &bytes[HEADER_LENGTH..],
        };
        frame.validate(frame_max, logical_max)?;
        Ok(frame)
    }
    /// Encode header and payload into caller storage. Negotiated limits are checked by decode/transport.
    /// # Errors
    /// Returns invalid coordinates or insufficient output capacity.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, ProtocolError> {
        self.validate_body(LOGICAL_MAX)?;
        let len = HEADER_LENGTH
            .checked_add(self.payload.len())
            .ok_or(ProtocolError::InvalidLength)?;
        if out.len() < len {
            return Err(ProtocolError::Capacity);
        }
        out[0] = 0x10 | self.header.kind as u8;
        out[1] = self.header.module;
        out[2] = self.header.opcode;
        out[3] = 0;
        out[4..6].copy_from_slice(&self.header.message_id.to_le_bytes());
        out[6..8].copy_from_slice(&self.header.offset.to_le_bytes());
        out[8..10].copy_from_slice(&self.header.total_length.to_le_bytes());
        out[10..len].copy_from_slice(self.payload);
        Ok(len)
    }
    pub(crate) fn validate(
        &self,
        frame_max: usize,
        logical_max: usize,
    ) -> Result<(), ProtocolError> {
        validate_limits(frame_max, logical_max)?;
        self.validate_body(logical_max)?;
        let capacity = frame_max - HEADER_LENGTH;
        if self.payload.len() > capacity {
            return Err(ProtocolError::InvalidLength);
        }
        let end = usize::from(self.header.offset) + self.payload.len();
        if end < usize::from(self.header.total_length) && self.payload.len() != capacity {
            return Err(ProtocolError::InvalidLength);
        }
        Ok(())
    }
    fn validate_body(&self, logical_max: usize) -> Result<(), ProtocolError> {
        if self.header.message_id == 0 {
            return Err(ProtocolError::InvalidId);
        }
        let total = usize::from(self.header.total_length);
        let offset = usize::from(self.header.offset);
        if total > logical_max || total > LOGICAL_MAX {
            return Err(ProtocolError::InvalidLength);
        }
        let end = offset
            .checked_add(self.payload.len())
            .ok_or(ProtocolError::InvalidLength)?;
        if end > total || (total == 0 && offset != 0) || (total != 0 && self.payload.is_empty()) {
            return Err(ProtocolError::InvalidLength);
        }
        Ok(())
    }
}

pub(crate) fn validate_limits(frame_max: usize, logical_max: usize) -> Result<(), ProtocolError> {
    if !(20..=LOGICAL_MAX + HEADER_LENGTH).contains(&frame_max)
        || logical_max == 0
        || logical_max > LOGICAL_MAX
    {
        return Err(ProtocolError::InvalidLength);
    }
    Ok(())
}
