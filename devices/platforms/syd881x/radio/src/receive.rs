//! Bounded RX FIFO ownership from pinned ROM0x612a..0x61c4.
use crate::{Bank, ByteRegisters, Radio, RadioError, Register};

/// Owned RX FIFO bytes with hardware metadata retained separately from length.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ReceivedPacket {
    bytes: [u8; 40],
    payload_len: u8,
}

impl core::fmt::Debug for ReceivedPacket {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("ReceivedPacket")
            .field("payload_len", &self.payload_len)
            .field("data_channel", &self.is_data_channel())
            .finish_non_exhaustive()
    }
}

impl ReceivedPacket {
    /// Two original hardware FIFO header bytes, including type/MIC metadata.
    /// These bytes must not be retransmitted as a raw air header.
    #[must_use]
    pub const fn fifo_header(&self) -> [u8; 2] {
        [self.bytes[0], self.bytes[1]]
    }

    /// Complete owned payload; internal type/MIC flags are not part of length.
    #[must_use]
    pub fn payload(&self) -> &[u8] {
        &self.bytes[2..2 + usize::from(self.payload_len)]
    }

    /// Hardware byte1 bit7 distinguishes data-channel from advertising PDUs.
    #[must_use]
    pub const fn is_data_channel(&self) -> bool {
        self.bytes[1] & 0x80 != 0
    }

    /// Uninterpreted MIC status bit, defined only for data-channel FIFO entries.
    /// It is not a CRC-valid indicator; unencrypted packets need no MIC gate.
    #[must_use]
    pub const fn mic_status_bit(&self) -> Option<bool> {
        if self.is_data_channel() {
            Some(self.bytes[1] & 0x40 != 0)
        } else {
            None
        }
    }
}

impl<R: ByteRegisters> Radio<R> {
    /// Pops one complete bounded FIFO entry and releases it exactly once.
    ///
    /// Returns `None` only when `RX_EMPTY` was observed before popping any bytes.
    /// A malformed declared length is released without reading its payload.
    /// # Errors
    /// Returns adapter failures or `InvalidReceiveLength`. On an adapter failure
    /// after popping starts, the FIFO cursor may be partial: caller must stop
    /// and reset the experiment, not retry/release an unknown partial entry.
    pub fn receive(&mut self) -> Result<Option<ReceivedPacket>, RadioError<R::Error>> {
        let fifo = Register::new(Bank::Ll, 0xc4);
        let release = Register::new(Bank::Ll, 0xc9);
        if self.read(Register::new(Bank::Ll, 3))? & 8 != 0 {
            return Ok(None);
        }
        let first = self.read(fifo)?;
        let second = self.read(fifo)?;
        let length = second & if second & 0x80 != 0 { 0x1f } else { 0x3f };
        if length >= 39 {
            self.write(release, 1)?;
            return Err(RadioError::InvalidReceiveLength { length });
        }
        let mut packet = ReceivedPacket {
            bytes: [0; 40],
            payload_len: length,
        };
        packet.bytes[0] = first;
        packet.bytes[1] = second;
        for index in 0..usize::from(length) {
            packet.bytes[2 + index] = self.read(fifo)?;
        }
        self.write(release, 1)?;
        Ok(Some(packet))
    }
}
