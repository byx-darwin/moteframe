//! Advertising register path. Channel and timing semantics need hardware validation.
use crate::{
    AdvertisingPdu, Bank, ByteRegisters, Radio, RadioError, Register, StaticRandomAddress,
};

/// Raw ROM advertising parameters, not a guarantee of Bluetooth timing or frequency.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdvertisingParameters {
    rounds: u8,
    interval: u16,
    channel_selection: u8,
}
impl AdvertisingParameters {
    /// Validates finite rounds and a nonempty three-bit channel selection.
    ///
    /// The mapping of selection bits to frequency and the interval unit remain
    /// candidates. This API deliberately does not label a selection bit as 37/38/39.
    /// # Errors
    /// Returns `InvalidParameters` for rounds outside 1..=127, zero interval,
    /// or channel selection outside 1..=7.
    pub fn new(
        rounds: u8,
        interval: u16,
        channel_selection: u8,
    ) -> Result<Self, RadioError<core::convert::Infallible>> {
        if rounds == 0 || rounds > 127 || interval == 0 || !(1..=7).contains(&channel_selection) {
            return Err(RadioError::InvalidParameters);
        }
        Ok(Self {
            rounds,
            interval,
            channel_selection,
        })
    }
}

/// Uninterpreted observed hardware bytes. These are not an air-validation result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Status {
    /// LL+1, read as in ROM `0x6294`.
    pub interrupt_status: u8,
    /// LL+3, FIFO state; bit zero is full in ROM `0x60e0..0x60e8`.
    pub fifo_status: u8,
    /// LL+0x10, advertising control observed without clearing any bit.
    pub advertising_control: u8,
}
impl<R: ByteRegisters> Radio<R> {
    /// Writes address/access address and the ROM `0x565e` parameters; filter disabled.
    ///
    /// Does not initialize CRC, whitening, power, clock or trim. MAC/access-address
    /// byte writes follow ROM's byte-copy path; aligned ROM copies may use words.
    /// # Errors
    /// Returns adapter errors; previous writes remain visible on failure.
    pub fn configure_advertising(
        &mut self,
        parameters: AdvertisingParameters,
        address: StaticRandomAddress,
    ) -> Result<(), RadioError<R::Error>> {
        for (offset, &byte) in (0x80..=0x85).zip(address.0.iter()) {
            self.write(ll(offset), byte)?;
        }
        for (offset, byte) in (0x88..=0x8b).zip([0xd6, 0xbe, 0x89, 0x8e]) {
            self.write(ll(offset), byte)?;
        }
        self.write(ll(0x15), parameters.channel_selection)?;
        let previous_rounds = self.read(ll(0x11))?;
        self.write(ll(0x11), (previous_rounds & 0x80) | parameters.rounds)?;
        let [low, high] = parameters.interval.to_le_bytes();
        self.write(ll(0x12), low)?;
        self.write(ll(0x13), high)?;
        let control = self.read(ll(0x10))?;
        self.write(ll(0x10), control | 2)?;
        self.write(ll(0x16), 0)
    }
    /// Queues the complete PDU through FIFO0 and commits once, after all bytes.
    /// # Errors
    /// Returns `FifoFull` with no writes or an adapter error. On a mid-packet
    /// adapter error no commit occurs; the caller must recover partial FIFO state.
    pub fn queue_advertisement(
        &mut self,
        pdu: &AdvertisingPdu,
    ) -> Result<(), RadioError<R::Error>> {
        if !matches!(pdu.as_slice()[0] & 15, 0 | 2) {
            return Err(RadioError::InvalidParameters);
        }
        self.queue_pdu(pdu, 0xc0)
    }

    /// Preloads a complete `SCAN_RSP` through FIFO1 as ROM0x402c/0x6104.
    /// # Errors
    /// Rejects a different PDU type before MMIO, or a full FIFO without writes.
    /// Adapter errors may leave a partial FIFO requiring reset recovery.
    pub fn queue_scan_response(
        &mut self,
        pdu: &AdvertisingPdu,
    ) -> Result<(), RadioError<R::Error>> {
        if pdu.as_slice()[0] & 15 != 4 {
            return Err(RadioError::InvalidParameters);
        }
        self.queue_pdu(pdu, 0xc1)
    }

    fn queue_pdu(&mut self, pdu: &AdvertisingPdu, offset: u8) -> Result<(), RadioError<R::Error>> {
        if self.read(ll(3))? & 1 != 0 {
            return Err(RadioError::FifoFull);
        }
        for &byte in pdu.as_slice() {
            self.write(ll(offset), byte)?;
        }
        self.write(ll(0xc8), 1)
    }
    /// Sets LL+0x10 bit zero as in ROM `0x56a6..0x56aa`.
    ///
    /// Does not touch ROM bookkeeping RAM and does not imply TX occurred.
    /// # Errors
    /// Returns adapter access failures.
    pub fn start_advertising(&mut self) -> Result<(), RadioError<R::Error>> {
        let control = self.read(ll(0x10))?;
        self.write(ll(0x10), control | 1)
    }
    /// Samples raw interrupt/FIFO/advertising state without acknowledging it.
    /// # Errors
    /// Returns adapter access failures; this is a sequential, non-atomic snapshot.
    pub fn poll_status(&mut self) -> Result<Status, RadioError<R::Error>> {
        Ok(Status {
            interrupt_status: self.read(ll(1))?,
            fifo_status: self.read(ll(3))?,
            advertising_control: self.read(ll(0x10))?,
        })
    }
    /// Polls at most `snapshots` times for any of the requested raw interrupt bits.
    /// # Errors
    /// Returns `InvalidParameters` for a zero mask/budget, `StatusTimeout` on
    /// exhaustion, or adapter failures. An observed bit is not proof of TX.
    pub fn wait_interrupt(
        &mut self,
        mask: u8,
        snapshots: u16,
    ) -> Result<Status, RadioError<R::Error>> {
        if mask == 0 || snapshots == 0 {
            return Err(RadioError::InvalidParameters);
        }
        for _ in 0..snapshots {
            let status = self.poll_status()?;
            if status.interrupt_status & mask != 0 {
                return Ok(status);
            }
        }
        Err(RadioError::StatusTimeout)
    }
    /// Reads LL+1 and writes that same byte back, matching ROM `0x6294..0x6296`.
    ///
    /// W1C hardware semantics remain unverified. Returns the observed byte.
    /// # Errors
    /// Returns adapter read/write failures.
    pub fn acknowledge_interrupt(&mut self) -> Result<u8, RadioError<R::Error>> {
        let observed = self.read(ll(1))?;
        self.write(ll(1), observed)?;
        Ok(observed)
    }
}

const fn ll(offset: u8) -> Register {
    Register::new(Bank::Ll, offset)
}
