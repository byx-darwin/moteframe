//! Unencrypted legacy data-channel FIFO construction; no hidden ROM queue.
use crate::{Bank, ByteRegisters, Radio, RadioError, Register};

/// A payload cannot fit the supported legacy data-channel PDU.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidDataPdu;

/// Complete owned legacy1M data PDU with at most27 payload bytes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DataPdu {
    bytes: [u8; 29],
    len: u8,
}
impl core::fmt::Debug for DataPdu {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DataPdu")
            .field("len", &self.len)
            .finish_non_exhaustive()
    }
}
impl DataPdu {
    /// Encodes a raw legacy fragment without adding a second L2CAP header.
    /// # Errors
    /// Rejects reserved LLIDs, payloads above27 bytes, empty controls and
    /// truncated first-fragment headers. SN/NESN stay under hardware ownership.
    pub fn fragment(llid: u8, payload: &[u8]) -> Result<Self, InvalidDataPdu> {
        if !(1..=3).contains(&llid)
            || payload.len() > 27
            || (llid == 2 && payload.len() < 4)
            || (llid == 3 && payload.is_empty())
        {
            return Err(InvalidDataPdu);
        }
        let length = u8::try_from(payload.len()).map_err(|_| InvalidDataPdu)?;
        let mut bytes = [0; 29];
        bytes[0] = llid;
        bytes[1] = length;
        bytes[2..2 + payload.len()].copy_from_slice(payload);
        Ok(Self {
            bytes,
            len: length + 2,
        })
    }
    /// Encodes an LL control payload with LLID3 and no RX metadata flags.
    /// # Errors
    /// Rejects empty payloads and payloads longer than27 bytes.
    pub fn control(payload: &[u8]) -> Result<Self, InvalidDataPdu> {
        if payload.is_empty() || payload.len() > 27 {
            return Err(InvalidDataPdu);
        }
        let length = u8::try_from(payload.len()).map_err(|_| InvalidDataPdu)?;
        let mut pdu = Self {
            bytes: [0; 29],
            len: length + 2,
        };
        pdu.bytes[0] = 3;
        pdu.bytes[1] = length;
        pdu.bytes[2..2 + payload.len()].copy_from_slice(payload);
        Ok(pdu)
    }
    /// Encodes a complete, unfragmented basic L2CAP packet with LLID2.
    /// # Errors
    /// Rejects zero channel IDs and SDUs longer than23 bytes (legacy27 minus4).
    pub fn l2cap(channel: u16, payload: &[u8]) -> Result<Self, InvalidDataPdu> {
        if channel == 0 || payload.len() > 23 {
            return Err(InvalidDataPdu);
        }
        let length = u8::try_from(payload.len()).map_err(|_| InvalidDataPdu)?;
        let mut pdu = Self {
            bytes: [0; 29],
            len: length + 6,
        };
        pdu.bytes[0] = 2;
        pdu.bytes[1] = length + 4;
        pdu.bytes[2] = length;
        let [lo, hi] = channel.to_le_bytes();
        pdu.bytes[4] = lo;
        pdu.bytes[5] = hi;
        pdu.bytes[6..6 + payload.len()].copy_from_slice(payload);
        Ok(pdu)
    }
    /// Sets the MD hint while retaining LLID, length and payload.
    ///
    /// Set this only when another owned PDU is waiting for transmission. Once
    /// committed, the hardware owns this header through any link retransmission.
    #[must_use]
    pub const fn with_more_data(mut self, pending: bool) -> Self {
        self.bytes[0] = (self.bytes[0] & !0x10) | if pending { 0x10 } else { 0 };
        self
    }
    /// Two-byte TX header followed by complete payload; no CRC/access address.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.bytes[..usize::from(self.len)]
    }
}
impl<R: ByteRegisters> Radio<R> {
    /// Queues one data PDU via FIFO0, then commits exactly once.
    ///
    /// SN/NESN and retransmission behavior must be validated separately. This
    /// method does not reinterpret RX metadata or claim remote delivery.
    /// # Errors
    /// Returns `FifoFull` without writes or adapter errors with possible partial
    /// FIFO state; after a partial adapter failure the caller must reset.
    pub fn queue_data(&mut self, pdu: &DataPdu) -> Result<(), RadioError<R::Error>> {
        if self.read(Register::new(Bank::Ll, 3))? & 1 != 0 {
            return Err(RadioError::FifoFull);
        }
        for &byte in pdu.as_slice() {
            self.write(Register::new(Bank::Ll, 0xc0), byte)?;
        }
        self.write(Register::new(Bank::Ll, 0xc8), 1)
    }
}
