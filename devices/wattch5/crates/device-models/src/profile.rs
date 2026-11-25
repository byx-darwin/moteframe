//! Optional application payloads, candidate V0.6 definitions.
use crate::canonical;
use device_protocol::ProtocolError;
/// `UserProfile` encoded value; Debug intentionally omits field contents.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct UserProfile {
    /// Wire field `weight_g`; lengths and units follow the V0.6 candidate contract.
    pub weight_g: u32,
    /// Wire field `height_mm`; lengths and units follow the V0.6 candidate contract.
    pub height_mm: u16,
    /// Wire field `sex`; lengths and units follow the V0.6 candidate contract.
    pub sex: u8,
    /// Wire field `age_years`; lengths and units follow the V0.6 candidate contract.
    pub age_years: u8,
    /// Wire field `walk_stride_mm`; lengths and units follow the V0.6 candidate contract.
    pub walk_stride_mm: u16,
    /// Wire field `run_stride_mm`; lengths and units follow the V0.6 candidate contract.
    pub run_stride_mm: u16,
    /// Wire field `valid_mask`; lengths and units follow the V0.6 candidate contract.
    pub valid_mask: u16,
}
impl core::fmt::Debug for UserProfile {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("UserProfile")
            .field("encoded_length", &Self::LENGTH)
            .finish_non_exhaustive()
    }
}
impl UserProfile {
    /// Exact encoded length.
    pub const LENGTH: usize = 14;
    /// Decode and validate a complete value.
    /// # Errors
    /// Rejects unknown enum/mask bits, noncanonical absent fields and incorrect lengths.
    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        if bytes.len() != Self::LENGTH {
            return Err(ProtocolError::InvalidLength);
        }
        let result = Self {
            weight_g: u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            height_mm: u16::from_le_bytes([bytes[4], bytes[5]]),
            sex: bytes[6],
            age_years: bytes[7],
            walk_stride_mm: u16::from_le_bytes([bytes[8], bytes[9]]),
            run_stride_mm: u16::from_le_bytes([bytes[10], bytes[11]]),
            valid_mask: u16::from_le_bytes([bytes[12], bytes[13]]),
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
        out[0..4].copy_from_slice(&self.weight_g.to_le_bytes());
        out[4..6].copy_from_slice(&self.height_mm.to_le_bytes());
        out[6] = self.sex;
        out[7] = self.age_years;
        out[8..10].copy_from_slice(&self.walk_stride_mm.to_le_bytes());
        out[10..12].copy_from_slice(&self.run_stride_mm.to_le_bytes());
        out[12..14].copy_from_slice(&self.valid_mask.to_le_bytes());
        Ok(Self::LENGTH)
    }
    /// Validate the value without executing any device behavior.
    /// # Errors
    /// Rejects out-of-contract values.
    pub fn validate(self) -> Result<(), ProtocolError> {
        if self.valid_mask & !0x3f != 0 {
            return Err(ProtocolError::Reserved);
        }
        let values = [
            u64::from(self.weight_g),
            u64::from(self.height_mm),
            u64::from(self.sex),
            u64::from(self.age_years),
            u64::from(self.walk_stride_mm),
            u64::from(self.run_stride_mm),
        ];
        canonical(u64::from(self.valid_mask), &values)?;
        if self.sex > 2 || (self.valid_mask & 8 != 0 && self.age_years == 255) {
            return Err(ProtocolError::InvalidState);
        }
        for i in [0, 1, 4, 5] {
            if self.valid_mask & (1 << i) != 0 && values[i] == 0 {
                return Err(ProtocolError::InvalidState);
            }
        }
        Ok(())
    }
}
/// `ActivityGoals` encoded value; Debug intentionally omits field contents.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ActivityGoals {
    /// Wire field `daily_steps`; lengths and units follow the V0.6 candidate contract.
    pub daily_steps: u32,
    /// Wire field `single_steps`; lengths and units follow the V0.6 candidate contract.
    pub single_steps: u32,
    /// Wire field `distance_mm`; lengths and units follow the V0.6 candidate contract.
    pub distance_mm: u32,
    /// Wire field `energy_j`; lengths and units follow the V0.6 candidate contract.
    pub energy_j: u32,
    /// Wire field `valid_mask`; lengths and units follow the V0.6 candidate contract.
    pub valid_mask: u16,
}
impl core::fmt::Debug for ActivityGoals {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ActivityGoals")
            .field("encoded_length", &Self::LENGTH)
            .finish_non_exhaustive()
    }
}
impl ActivityGoals {
    /// Exact encoded length.
    pub const LENGTH: usize = 18;
    /// Decode and validate a complete value.
    /// # Errors
    /// Rejects unknown enum/mask bits, noncanonical absent fields and incorrect lengths.
    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        if bytes.len() != Self::LENGTH {
            return Err(ProtocolError::InvalidLength);
        }
        let result = Self {
            daily_steps: u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            single_steps: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            distance_mm: u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            energy_j: u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
            valid_mask: u16::from_le_bytes([bytes[16], bytes[17]]),
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
        out[0..4].copy_from_slice(&self.daily_steps.to_le_bytes());
        out[4..8].copy_from_slice(&self.single_steps.to_le_bytes());
        out[8..12].copy_from_slice(&self.distance_mm.to_le_bytes());
        out[12..16].copy_from_slice(&self.energy_j.to_le_bytes());
        out[16..18].copy_from_slice(&self.valid_mask.to_le_bytes());
        Ok(Self::LENGTH)
    }
    /// Validate the value without executing any device behavior.
    /// # Errors
    /// Rejects out-of-contract values.
    pub fn validate(self) -> Result<(), ProtocolError> {
        if self.valid_mask & !0xf != 0 {
            return Err(ProtocolError::Reserved);
        }
        canonical(
            u64::from(self.valid_mask),
            &[
                u64::from(self.daily_steps),
                u64::from(self.single_steps),
                u64::from(self.distance_mm),
                u64::from(self.energy_j),
            ],
        )?;
        Ok(())
    }
}
