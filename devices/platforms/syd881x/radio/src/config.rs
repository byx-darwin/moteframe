//! Ordered configuration records from the physical flash, without ROM clock helpers.
use crate::{Bank, ByteRegisters, Radio, RadioError, Register};

/// A malformed or unsupported physical-flash register configuration table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvalidConfiguration {
    /// Missing six-byte header, truncated records or trailing bytes.
    Length,
    /// Table exceeds the physical 4 KiB configuration region.
    TooLarge,
    /// A record type is outside the known ROM dispatch range 0..=5.
    RecordType,
}

/// Validated six-byte header and exactly count ordered three-byte records.
///
/// Validation rejects unknown types before any register side effects; the ROM
/// skips them. No sorting, de-duplication or implicit RF bank selection occurs.
#[derive(Clone, Copy)]
pub struct ConfigurationTable<'a> {
    bytes: &'a [u8],
}
impl core::fmt::Debug for ConfigurationTable<'_> {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("ConfigurationTable")
            .field("len", &self.bytes.len())
            .finish_non_exhaustive()
    }
}
impl<'a> ConfigurationTable<'a> {
    /// Validates a complete table copied from the physical-flash configuration region.
    /// # Errors
    /// Rejects invalid length, sizes above 4096 bytes or unknown record types.
    pub fn new(bytes: &'a [u8]) -> Result<Self, InvalidConfiguration> {
        if bytes.len() > 4096 {
            return Err(InvalidConfiguration::TooLarge);
        }
        if bytes.len() < 6 {
            return Err(InvalidConfiguration::Length);
        }
        let count = usize::from(u16::from_le_bytes([bytes[4], bytes[5]]));
        if bytes.len() != 6 + count * 3 {
            return Err(InvalidConfiguration::Length);
        }
        if bytes[6..].chunks_exact(3).any(|record| record[0] > 5) {
            return Err(InvalidConfiguration::RecordType);
        }
        Ok(Self { bytes })
    }
    /// The six raw header bytes; caller must handle board clock/trim prerequisites.
    #[must_use]
    pub fn header(&self) -> &[u8] {
        &self.bytes[..6]
    }
}
impl<R: ByteRegisters> Radio<R> {
    /// Applies every validated record, in original order, preserving RF bank writes.
    ///
    /// Only the `0x0bfa..0x0c64` record loop is reproduced. No calibration,
    /// header clock helper, delay, interrupt or ROM bookkeeping is implied.
    /// # Errors
    /// Returns the first adapter/RF timeout failure and stops before later records.
    pub fn apply_configuration(
        &mut self,
        table: &ConfigurationTable<'_>,
    ) -> Result<(), RadioError<R::Error>> {
        for record in table.bytes[6..].chunks_exact(3) {
            if record[0] == 3 {
                self.rf_write(record[1], record[2])?;
            } else if let Some(bank) = record_bank(record[0]) {
                self.write(Register::new(bank, record[1]), record[2])?;
            } else {
                return Err(RadioError::InvalidParameters);
            }
        }
        Ok(())
    }
}

const fn record_bank(kind: u8) -> Option<Bank> {
    match kind {
        0 => Some(Bank::Ll),
        1 => Some(Bank::Modem),
        2 => Some(Bank::Misc),
        4 => Some(Bank::PowerOn),
        5 => Some(Bank::Interface),
        _ => None,
    }
}
