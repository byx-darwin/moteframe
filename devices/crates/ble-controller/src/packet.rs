//! Standard legacy link payloads, independent of FIFO layout and MMIO.

/// Legacy link data payload category, excluding sequence-number ownership.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FrameKind {
    /// Continuing L2CAP fragment, or a zero-length empty link PDU.
    Continuation = 1,
    /// First L2CAP fragment containing at least its four-byte header.
    Start = 2,
    /// Nonempty link-control procedure.
    Control = 3,
}

/// A payload is too long, empty control, or a truncated L2CAP start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidFrame;

/// Owned legacy27-byte link payload and two-byte header.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct LinkFrame {
    bytes: [u8; 29],
    len: u8,
    kind: FrameKind,
}
impl core::fmt::Debug for LinkFrame {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LinkFrame")
            .field("len", &self.len)
            .finish_non_exhaustive()
    }
}
impl LinkFrame {
    /// Copies validated on-air payload; sequence numbers remain the backend's concern.
    /// # Errors
    /// Rejects payloads above27 bytes, empty control and starts below4 bytes.
    pub fn new(kind: FrameKind, payload: &[u8]) -> Result<Self, InvalidFrame> {
        if payload.len() > 27
            || (kind == FrameKind::Control && payload.is_empty())
            || (kind == FrameKind::Start && payload.len() < 4)
        {
            return Err(InvalidFrame);
        }
        let len = u8::try_from(payload.len()).map_err(|_| InvalidFrame)?;
        let mut bytes = [0; 29];
        bytes[0] = kind as u8;
        bytes[1] = len;
        bytes[2..2 + payload.len()].copy_from_slice(payload);
        Ok(Self { bytes, len, kind })
    }
    /// Standard payload category.
    #[must_use]
    pub const fn kind(self) -> FrameKind {
        self.kind
    }
    /// Owned payload without link header.
    #[must_use]
    pub fn payload(&self) -> &[u8] {
        &self.bytes[2..2 + usize::from(self.len)]
    }
    /// Header and payload, without CRC/access address or hardware RX metadata.
    #[must_use]
    pub fn as_pdu(&self) -> &[u8] {
        &self.bytes[..2 + usize::from(self.len)]
    }
    /// Stages a truthful pending-data hint without changing other fields.
    #[must_use]
    pub const fn with_more_data(mut self, pending: bool) -> Self {
        self.bytes[0] = (self.bytes[0] & !0x10) | if pending { 0x10 } else { 0 };
        self
    }
}
