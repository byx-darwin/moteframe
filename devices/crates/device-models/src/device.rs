//! Optional application payloads, candidate V0.6 definitions.
use crate::canonical;
use device_protocol::ProtocolError;
/// `BatteryState` encoded value; Debug intentionally omits field contents.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct BatteryState {
    /// Wire field `level`; lengths and units follow the V0.6 candidate contract.
    pub level: u8,
    /// Wire field `charging`; lengths and units follow the V0.6 candidate contract.
    pub charging: u8,
    /// Wire field `charge_complete`; lengths and units follow the V0.6 candidate contract.
    pub charge_complete: u8,
    /// Wire field `since_charge_s`; lengths and units follow the V0.6 candidate contract.
    pub since_charge_s: u32,
    /// Wire field `valid_mask`; lengths and units follow the V0.6 candidate contract.
    pub valid_mask: u8,
}
impl core::fmt::Debug for BatteryState {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BatteryState")
            .field("encoded_length", &Self::LENGTH)
            .finish_non_exhaustive()
    }
}
impl BatteryState {
    /// Exact encoded length.
    pub const LENGTH: usize = 8;
    /// Decode and validate a complete value.
    /// # Errors
    /// Rejects unknown enum/mask bits, noncanonical absent fields and incorrect lengths.
    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        if bytes.len() != Self::LENGTH {
            return Err(ProtocolError::InvalidLength);
        }
        let result = Self {
            level: bytes[0],
            charging: bytes[1],
            charge_complete: bytes[2],
            since_charge_s: u32::from_le_bytes([bytes[3], bytes[4], bytes[5], bytes[6]]),
            valid_mask: bytes[7],
        };
        result.validate()?;
        Ok(result)
    }
    /// Encode a validated value.
    /// # Errors
    /// Returns invalid fields or insufficient caller capacity.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, ProtocolError> {
        self.validate()?;
        if out.len() < Self::LENGTH {
            return Err(ProtocolError::Capacity);
        }
        out[0] = self.level;
        out[1] = self.charging;
        out[2] = self.charge_complete;
        out[3..7].copy_from_slice(&self.since_charge_s.to_le_bytes());
        out[7] = self.valid_mask;
        Ok(Self::LENGTH)
    }
    /// Validate the value without executing any device behavior.
    /// # Errors
    /// Rejects out-of-contract values.
    pub fn validate(self) -> Result<(), ProtocolError> {
        if self.valid_mask & !15 != 0 {
            return Err(ProtocolError::Reserved);
        }
        if self.level > 100 || self.charging > 1 || self.charge_complete > 1 {
            return Err(ProtocolError::InvalidState);
        }
        if self.valid_mask & 8 != 0 && self.valid_mask & 2 == 0 {
            return Err(ProtocolError::InvalidState);
        }
        if self.valid_mask & 0x0a == 0x0a && self.charging == 1 && self.since_charge_s != 0 {
            return Err(ProtocolError::InvalidState);
        }
        canonical(
            u64::from(self.valid_mask),
            &[
                u64::from(self.level),
                u64::from(self.charging),
                u64::from(self.charge_complete),
                u64::from(self.since_charge_s),
            ],
        )?;
        Ok(())
    }
}
