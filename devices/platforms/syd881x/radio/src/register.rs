//! Byte register namespace restricted to the chip's five configuration banks.

/// Register banks present in the ROM's configuration executor (`0x0c26..0x0c58`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Bank {
    /// Link layer, base `0x4002_0000`.
    Ll,
    /// Modem, base `0x4003_0000`.
    Modem,
    /// Miscellaneous including the RF bridge, base `0x4000_0000`.
    Misc,
    /// Power on, base `0x5000_1000`.
    PowerOn,
    /// Interface, base `0x5001_0000`.
    Interface,
}

/// One byte register in a restricted chip bank, never an arbitrary RAM address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Register {
    bank: Bank,
    offset: u8,
}
impl Register {
    /// Selects one byte within one of the chip's configuration banks.
    #[must_use]
    pub const fn new(bank: Bank, offset: u8) -> Self {
        Self { bank, offset }
    }
    /// Selected hardware bank.
    #[must_use]
    pub const fn bank(self) -> Bank {
        self.bank
    }
    /// Byte offset within the bank, bounded by construction to 0..=255.
    #[must_use]
    pub const fn offset(self) -> u8 {
        self.offset
    }
}

/// Serialized byte register access supplied by a board's MMIO adapter.
///
/// The adapter must perform each access once, in order, at byte width. It must
/// not enable interrupts or call a vendor/ROM radio handler on the caller's behalf.
pub trait ByteRegisters {
    /// Adapter access error.
    type Error;
    /// Reads one byte.
    /// # Errors
    /// Returns the adapter's access failure.
    fn read8(&mut self, register: Register) -> Result<u8, Self::Error>;
    /// Writes one byte.
    /// # Errors
    /// Returns the adapter's access failure.
    fn write8(&mut self, register: Register, value: u8) -> Result<(), Self::Error>;
}
