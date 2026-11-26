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

#[cfg(feature = "data-length")]
const MAX_ACL: usize = 251;
#[cfg(not(feature = "data-length"))]
const MAX_ACL: usize = 27;

/// An owned link-layer payload and its two-byte header.
#[derive(Clone, Copy)]
pub struct LinkFrame {
    bytes: [u8; MAX_ACL + 2],
    len: u8,
    kind: FrameKind,
}
impl PartialEq for LinkFrame {
    fn eq(&self, other: &Self) -> bool {
        self.as_pdu() == other.as_pdu()
    }
}
impl Eq for LinkFrame {}
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
    /// Rejects oversized payloads, empty control PDUs and truncated starts.
    pub fn new(kind: FrameKind, payload: &[u8]) -> Result<Self, InvalidFrame> {
        let len = u8::try_from(payload.len()).map_err(|_| InvalidFrame)?;
        let mut frame = Self::empty(kind, len)?;
        frame.bytes[2..2 + payload.len()].copy_from_slice(payload);
        Ok(frame)
    }
    fn empty(kind: FrameKind, len: u8) -> Result<Self, InvalidFrame> {
        Self::validate(kind, len)?;
        let mut frame = Self::EMPTY;
        frame.set_header(kind, len);
        Ok(frame)
    }
    /// Empty link-layer continuation, also usable as caller-owned receive storage.
    pub const EMPTY: Self = {
        let mut bytes = [0; MAX_ACL + 2];
        bytes[0] = FrameKind::Continuation as u8;
        Self {
            bytes,
            len: 0,
            kind: FrameKind::Continuation,
        }
    };
    fn set_header(&mut self, kind: FrameKind, len: u8) {
        self.bytes[0] = kind as u8;
        self.bytes[1] = len;
        self.len = len;
        self.kind = kind;
    }
    fn validate(kind: FrameKind, len: u8) -> Result<(), InvalidFrame> {
        if usize::from(len) > MAX_ACL
            || (kind == FrameKind::Control && !(1..=27).contains(&len))
            || (kind == FrameKind::Start && len < 4)
        {
            return Err(InvalidFrame);
        }
        Ok(())
    }
    /// Fills a reusable frame directly from packet RAM.
    /// # Errors
    /// Rejects invalid lengths before touching the frame or reading packet RAM.
    #[cfg(feature = "data-length")]
    pub fn read_payload(
        &mut self,
        kind: FrameKind,
        len: u8,
        mut read: impl FnMut(usize) -> u8,
    ) -> Result<(), InvalidFrame> {
        Self::validate(kind, len)?;
        self.set_header(kind, len);
        for (offset, byte) in self.bytes[2..2 + usize::from(len)].iter_mut().enumerate() {
            *byte = read(offset);
        }
        Ok(())
    }
    /// Copies directly from a byte-addressed packet RAM slot into its new owner.
    /// # Errors
    /// Rejects invalid lengths before reading any packet bytes.
    #[cfg(feature = "data-length")]
    pub fn from_reader(
        kind: FrameKind,
        len: u8,
        read: impl FnMut(usize) -> u8,
    ) -> Result<Self, InvalidFrame> {
        let mut frame = Self::EMPTY;
        frame.read_payload(kind, len, read)?;
        Ok(frame)
    }
    /// Standard payload category.
    #[must_use]
    pub const fn kind(&self) -> FrameKind {
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
    /// Retains the first fragment and returns any remaining ACL payload.
    #[cfg(not(feature = "data-length"))]
    pub(crate) fn split_off_acl(&mut self, limit: u8) -> Option<Self> {
        let limit = limit.max(4);
        if self.len <= limit {
            return None;
        }
        let tail = Self::new(
            FrameKind::Continuation,
            &self.payload()[usize::from(limit)..],
        )
        .ok()?;
        self.len = limit;
        self.bytes[1] = limit;
        Some(tail)
    }
    /// Stages a truthful pending-data hint without changing other fields.
    #[must_use]
    pub const fn with_more_data(mut self, pending: bool) -> Self {
        self.bytes[0] = (self.bytes[0] & !0x10) | if pending { 0x10 } else { 0 };
        self
    }
}
