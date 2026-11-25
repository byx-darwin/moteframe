//! Frozen0022/schema1 record, independent of new2201/schema2 generation.
use crate::Failure;
/// Frozen complete20-byte legacy sport summary; no old phone frame is accepted.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct LegacySportSummary {
    /// Actual elapsed milliseconds.
    pub duration_ms: u32,
    /// Actual steps, never repetitions/oar counts.
    pub steps: u32,
    /// Actual millimetres.
    pub distance_mm: u32,
    /// Actual joules.
    pub energy_j: u32,
    /// Unified sport type0..23.
    pub sport_type: u16,
    /// Bits0duration/1steps/2distance/3energy; other bits zero, absent values zero.
    pub valid_mask: u16,
}
impl core::fmt::Debug for LegacySportSummary {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LegacySportSummary")
            .field("encoded_length", &20)
            .finish_non_exhaustive()
    }
}
impl LegacySportSummary {
    /// Exact payload length, excluding history envelope/CRC.
    pub const LENGTH: usize = 20;
    /// Decode one exact frozen payload.
    /// # Errors
    /// Invalid length, enum, masks or noncanonical absent values.
    pub fn decode(bytes: &[u8]) -> Result<Self, Failure> {
        if bytes.len() != 20 {
            return Err(Failure::Invalid);
        }
        let value = Self {
            duration_ms: read32(bytes, 0),
            steps: read32(bytes, 4),
            distance_mm: read32(bytes, 8),
            energy_j: read32(bytes, 12),
            sport_type: u16::from_le_bytes([bytes[16], bytes[17]]),
            valid_mask: u16::from_le_bytes([bytes[18], bytes[19]]),
        };
        value.validate()?;
        Ok(value)
    }
    /// Encode actual validated old content without converting from schema2.
    /// # Errors
    /// Invalid fields or insufficient capacity.
    pub fn encode(self, out: &mut [u8]) -> Result<usize, Failure> {
        self.validate()?;
        if out.len() < 20 {
            return Err(Failure::Capacity);
        }
        for (index, value) in [
            self.duration_ms,
            self.steps,
            self.distance_mm,
            self.energy_j,
        ]
        .iter()
        .enumerate()
        {
            out[index * 4..index * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        out[16..18].copy_from_slice(&self.sport_type.to_le_bytes());
        out[18..20].copy_from_slice(&self.valid_mask.to_le_bytes());
        Ok(20)
    }
    /// Validate the frozen field/mask contract.
    /// # Errors
    /// Invalid enum/reserved bits/noncanonical absent values.
    pub fn validate(self) -> Result<(), Failure> {
        if self.sport_type > 23 || self.valid_mask & !15 != 0 {
            return Err(Failure::Invalid);
        }
        for (index, value) in [
            self.duration_ms,
            self.steps,
            self.distance_mm,
            self.energy_j,
        ]
        .iter()
        .enumerate()
        {
            if self.valid_mask & (1 << index) == 0 && *value != 0 {
                return Err(Failure::Invalid);
            }
        }
        Ok(())
    }
}
fn read32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}
