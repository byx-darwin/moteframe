//! Version-2 factory RF trim from the Puya P25Q80 security region.

const CHECKSUM_OFFSET: usize = 0x9c;
/// Size of the checked trim record, including the stored CRC.
pub const TRIM_BYTES: usize = CHECKSUM_OFFSET + 4;
const RF_VALID_FLAGS: u32 = 0b111;

/// Per-unit RF calibration required by the cold-start probe.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RadioTrim {
    /// TX VCO gain selector.
    pub kvco: u8,
    /// Split RF synthesizer trim for the two analog configuration bytes.
    pub synthesizer_trim: u8,
    /// RX analog bias trim.
    pub rx_bias_trim: u8,
    /// RX DC offset for each gain slot.
    pub rx_dc: [u8; 32],
    /// Measured TX modulation scale.
    pub tx_modulation_scale: u16,
    /// Measured TX modulation DAC current.
    pub tx_modulation_current: u8,
    /// Calibrated PA control steps.
    pub tx_power_steps: [u8; 16],
}

/// Factory data cannot be used for the supported RF mode.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TrimError {
    /// The stored CRC did not match the payload.
    Checksum,
    /// The record belongs to a different factory layout.
    Version,
    /// TX or RX measurements are missing or outside the supported range.
    UnsupportedCalibration,
}

impl RadioTrim {
    /// Gain selector for both three-bit VCO modulation fields.
    #[must_use]
    pub const fn tx_kvco_fields(&self) -> u8 {
        (self.kvco << 4) | self.kvco
    }

    /// Expand the v2 reference measurement into the FR8029D modem's
    /// eight (1M, 2M) scale entries. Both banks use unsigned Q1.15 ratios.
    pub fn tx_modulation_bands(&self) -> impl Iterator<Item = (u16, u16)> {
        const RATIOS: [(u16, u16); 8] = [
            (0x44fb, 0x87e0),
            (0x4288, 0x830d),
            (0x41f5, 0x81eb),
            (0x40fb, 0x8000),
            (0x40e8, 0x7fda),
            (0x401c, 0x7e46),
            (0x3f3c, 0x7c8d),
            (0x3dc2, 0x79a6),
        ];
        let measured = u32::from(self.tx_modulation_scale);
        RATIOS.into_iter().map(move |(one_m, two_m)| {
            let scale =
                |ratio| u16::try_from((measured * u32::from(ratio)) >> 15).unwrap_or(u16::MAX);
            (scale(one_m), scale(two_m))
        })
    }

    /// Verifies and extracts the version-2 RF measurements.
    ///
    /// # Errors
    /// Returns an error for a corrupt record or an unsupported calibration.
    pub fn parse(bytes: &[u8; TRIM_BYTES]) -> Result<Self, TrimError> {
        let mut crc = u32::MAX;
        for &byte in &bytes[..CHECKSUM_OFFSET] {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = (crc >> 1) ^ (0xedb8_8320 & 0_u32.wrapping_sub(crc & 1));
            }
        }
        let stored = u32::from_le_bytes([
            bytes[CHECKSUM_OFFSET],
            bytes[CHECKSUM_OFFSET + 1],
            bytes[CHECKSUM_OFFSET + 2],
            bytes[CHECKSUM_OFFSET + 3],
        ]);
        if !crc != stored {
            return Err(TrimError::Checksum);
        }
        if bytes[0x86] != 2 {
            return Err(TrimError::Version);
        }

        let flags = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let kvco = bytes[0x2f];
        let tx_modulation_scale = u16::from_le_bytes([bytes[0x5c], bytes[0x5d]]);
        let tx_modulation_current = bytes[0x6c];
        let mut rx_dc = [0; 32];
        rx_dc.copy_from_slice(&bytes[0x3c..0x5c]);
        let mut tx_power_steps = [0; 16];
        tx_power_steps.copy_from_slice(&bytes[0x74..0x84]);

        if flags & RF_VALID_FLAGS != RF_VALID_FLAGS
            || kvco > 7
            || !(1..=0x0fff).contains(&tx_modulation_scale)
            || tx_modulation_current == 0
            || tx_modulation_current == u8::MAX
            || bytes[0x5e..0x6c].iter().any(|&byte| byte != 0)
            || bytes[0x6d..0x74].iter().any(|&byte| byte != 0)
            || tx_power_steps.iter().any(|&step| step > 31)
        {
            return Err(TrimError::UnsupportedCalibration);
        }

        Ok(Self {
            kvco,
            synthesizer_trim: bytes[0x39],
            rx_bias_trim: bytes[0x3a],
            rx_dc,
            tx_modulation_scale,
            tx_modulation_current,
            tx_power_steps,
        })
    }
}
