//! FR8029D bring-up contracts and an optional upstream GATT profile.
#![no_std]
#![forbid(unsafe_code)]

pub mod advertising;
pub mod connection;
#[cfg(feature = "ble-gatt-probe")]
pub mod gatt;
#[cfg(feature = "ble-gatt-probe")]
pub mod gatt_runner;
pub mod radio_trim;
#[cfg(feature = "ble-gatt-probe")]
pub mod wake;

/// Start of the flash application slot used by the boot ROM.
pub const XIP_ENTRY: u32 = 0x1000_2000;
const PACKET_RAM_BASE: u32 = 0x4001_0000;
const PACKET_RAM_SIZE: u32 = 0x4000;

/// Registers captured when the probe enters from the XIP slot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Snapshot {
    /// ARM CPU identification register.
    pub cpuid: u32,
    /// Vector table address before probe startup changes its state.
    pub vtor: u32,
    /// QSPI0 configuration register.
    pub qspi_config: u32,
    /// QSPI0 device size configuration register.
    pub qspi_size: u32,
    /// QSPI0 AHB remap address register.
    pub qspi_remap: u32,
}

/// Diagnostic status written to SRAM for the debugger.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ProbeResult {
    /// Core, vector address and XIP configuration match the probe's expectations.
    Ready = 1,
    /// CPU identification did not match an ARM Cortex-M33 core.
    UnsupportedCore = 2,
    /// The vector table pointed outside the expected application entry.
    UnexpectedVectorTable = 3,
    /// The QSPI0 configuration differed from the expected XIP settings.
    UnexpectedXipConfiguration = 4,
}

impl Snapshot {
    /// Identifies an ARM Cortex-M33 independently of its variant and revision.
    #[must_use]
    pub const fn is_cortex_m33(cpuid: u32) -> bool {
        cpuid & 0xff0f_fff0 == 0x410f_d210
    }

    /// Returns the configured flash command address width in bytes.
    #[must_use]
    pub const fn qspi_address_bytes(self) -> u32 {
        (self.qspi_size & 0xf) + 1
    }

    /// Checks the board snapshot against the observed boot configuration.
    #[must_use]
    pub const fn classify(self) -> ProbeResult {
        const QSPI_DIRECT_ACCESS: u32 = 1 << 7;
        const QSPI_AHB_REMAP: u32 = 1 << 14;

        if !Self::is_cortex_m33(self.cpuid) {
            return ProbeResult::UnsupportedCore;
        }
        if self.vtor != XIP_ENTRY {
            return ProbeResult::UnexpectedVectorTable;
        }
        if self.qspi_config & QSPI_DIRECT_ACCESS == 0
            || self.qspi_config & QSPI_AHB_REMAP != 0
            || self.qspi_remap != 0
            || self.qspi_address_bytes() != 3
        {
            return ProbeResult::UnexpectedXipConfiguration;
        }
        ProbeResult::Ready
    }
}

/// Packet RAM structures whose strides were verified in the ROM layout code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescriptorKind {
    /// Receive descriptor with a 36-byte stride.
    Rx,
    /// Connection control area with a 148-byte stride.
    ConnectionControl,
}

impl DescriptorKind {
    const fn stride(self) -> u32 {
        match self {
            Self::Rx => 36,
            Self::ConnectionControl => 148,
        }
    }
}

/// Failure to fit a descriptor region or index within packet RAM.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DescriptorError {
    /// The descriptor count was zero.
    Empty,
    /// The region's byte offset was not word-aligned.
    Misaligned,
    /// The region did not fit entirely within packet RAM.
    OutOfRange,
    /// The index fell outside the declared descriptor count.
    IndexOutOfRange,
}

/// Checked arithmetic for a contiguous region of packet RAM.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DescriptorRegion {
    kind: DescriptorKind,
    offset: u32,
    count: u32,
}

impl DescriptorRegion {
    /// Validates a region starting at a byte offset from the packet RAM base.
    ///
    /// # Errors
    ///
    /// Returns an error for an empty region, an unaligned start or an out-of-bounds span.
    pub fn new(kind: DescriptorKind, offset: u32, count: u32) -> Result<Self, DescriptorError> {
        if count == 0 {
            return Err(DescriptorError::Empty);
        }
        if !offset.is_multiple_of(4) {
            return Err(DescriptorError::Misaligned);
        }
        let end = count
            .checked_mul(kind.stride())
            .and_then(|size| offset.checked_add(size));
        if end.is_none_or(|end| end > PACKET_RAM_SIZE) {
            return Err(DescriptorError::OutOfRange);
        }
        Ok(Self {
            kind,
            offset,
            count,
        })
    }

    /// Computes the device address of an entry without accessing packet RAM.
    ///
    /// # Errors
    ///
    /// Returns [`DescriptorError::IndexOutOfRange`] when `index` is out of bounds.
    pub fn address(self, index: u32) -> Result<u32, DescriptorError> {
        if index >= self.count {
            return Err(DescriptorError::IndexOutOfRange);
        }
        Ok(PACKET_RAM_BASE + self.offset + index * self.kind.stride())
    }
}
