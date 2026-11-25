//! Read-only SPI NOR plus a separate caller-attested incremental writer.
#![no_std]
#![forbid(unsafe_code)]
use device_core::DelayMs;
use device_core::ReadAt;
use embedded_hal::spi::{Operation, SpiDevice};
/// Opt-in caller-attested asynchronous writer and shared-die access gate.
pub mod writable;
/// Read-only NOR validation or transport failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NorError<E> {
    /// SPI transaction failed; discard output.
    Transport(E),
    /// Blank or unsupported conventional JEDEC identity/density.
    Identity,
    /// Address range is beyond the identified24-bit extent.
    Bounds,
    /// Caller must split the request to at most256 bytes and service other work.
    Chunk,
}
/// Narrow byte reader; no write-enable, programming, erase, status-write or reset API.
/// Capacity uses the conventional JEDEC density exponent, not an independent silicon measurement.
#[derive(Debug)]
pub struct ReadOnlyNor<S> {
    device: S,
    id: [u8; 3],
    capacity: u32,
}
impl<S: SpiDevice> ReadOnlyNor<S> {
    /// Releases power-down, waits at least1ms, and reads conventional JEDEC identity.
    /// # Errors
    /// Rejects blank/type-zero identity and density outside64KiB..16MiB, or SPI errors.
    pub fn open(mut device: S, delay: &mut impl DelayMs) -> Result<Self, NorError<S::Error>> {
        device.write(&[0xab]).map_err(NorError::Transport)?;
        delay.delay_ms(1);
        let mut id = [0; 3];
        device
            .transaction(&mut [Operation::Write(&[0x9f]), Operation::Read(&mut id)])
            .map_err(NorError::Transport)?;
        if matches!(id[0], 0 | 255) || matches!(id[1], 0 | 255) || !(16..=24).contains(&id[2]) {
            return Err(NorError::Identity);
        }
        Ok(Self {
            device,
            capacity: 1_u32 << id[2],
            id,
        })
    }
    /// Raw manufacturer/type/density bytes; do not infer an exact part number from this alone.
    #[must_use]
    pub const fn id(&self) -> [u8; 3] {
        self.id
    }
}
impl<S: SpiDevice> ReadAt for ReadOnlyNor<S> {
    type Error = NorError<S::Error>;
    fn capacity(&self) -> u32 {
        self.capacity
    }
    fn read_exact(&mut self, offset: u32, out: &mut [u8]) -> Result<(), Self::Error> {
        let count = u32::try_from(out.len()).map_err(|_| NorError::Bounds)?;
        if offset
            .checked_add(count)
            .is_none_or(|end| end > self.capacity)
        {
            return Err(NorError::Bounds);
        }
        if out.len() > 256 {
            return Err(NorError::Chunk);
        }
        if out.is_empty() {
            return Ok(());
        }
        let bytes = offset.to_be_bytes();
        let command = [3, bytes[1], bytes[2], bytes[3]];
        self.device
            .transaction(&mut [Operation::Write(&command), Operation::Read(out)])
            .map_err(NorError::Transport)
    }
}
