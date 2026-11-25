//! Explicit upper-bound heart zones with sample-identity deduplication.
use crate::Error;
/// A frozen per-context threshold snapshot; no implicit maximum-heart-rate formula.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZoneThresholds([u16; 5]);
impl ZoneThresholds {
    /// Require five positive strictly increasing inclusive upper bounds.
    /// # Errors
    /// Zero/equal/descending thresholds are invalid.
    pub fn new(values: [u16; 5]) -> Result<Self, Error> {
        if values[0] == 0 || values.windows(2).any(|w| w[0] >= w[1]) {
            return Err(Error::InvalidField);
        }
        Ok(Self(values))
    }
}
/// Five counters for one live context. Only new actual algorithm samples are counted, never BLE retries.
pub struct ZoneCounter {
    context_id: u32,
    thresholds: ZoneThresholds,
    counts: [u32; 5],
    last_sample: Option<u64>,
}
impl core::fmt::Debug for ZoneCounter {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ZoneCounter")
            .field("context_id", &self.context_id)
            .field("last_sample", &self.last_sample)
            .finish_non_exhaustive()
    }
}
impl ZoneCounter {
    /// Start a nonzero caller-validated live context, taking an immutable threshold snapshot.
    /// # Errors
    /// Context0 is not an implicit latest/independent context.
    pub const fn new(context_id: u32, thresholds: ZoneThresholds) -> Result<Self, Error> {
        if context_id == 0 {
            return Err(Error::InvalidField);
        }
        Ok(Self {
            context_id,
            thresholds,
            counts: [0; 5],
            last_sample: None,
        })
    }
    /// Observe a new nonzero identity. Returns true only when an actual valid value incremented a bin.
    /// None/zero/out-of-five-bounds consumes identity without counting. Repeated identity is ignored;
    /// backwards identity fails. A new sample must not be assigned for retransmission by the caller.
    /// # Errors
    /// Rejects zero/backwards identity and count overflow without mutating counters or sample cursor.
    pub fn observe(&mut self, sample_id: u64, bpm: Option<u16>) -> Result<bool, Error> {
        if sample_id == 0 {
            return Err(Error::InvalidField);
        }
        if let Some(previous) = self.last_sample {
            if sample_id == previous {
                return Ok(false);
            }
            if sample_id < previous {
                return Err(Error::InvalidState);
            }
        }
        let zone = bpm.filter(|v| *v != 0).and_then(|v| {
            self.thresholds
                .0
                .iter()
                .position(|threshold| v <= *threshold)
        });
        if let Some(index) = zone {
            self.counts[index] = self.counts[index].checked_add(1).ok_or(Error::Exhausted)?;
        }
        self.last_sample = Some(sample_id);
        Ok(zone.is_some())
    }
    /// Validated context alias; caller owns live sport/session validation.
    #[must_use]
    pub const fn context_id(&self) -> u32 {
        self.context_id
    }
    /// Frozen inclusive upper bounds for this context.
    #[must_use]
    pub const fn thresholds(&self) -> [u16; 5] {
        self.thresholds.0
    }
    /// Actual per-bin sample counts, not seconds or BLE message counts.
    #[must_use]
    pub const fn counts(&self) -> [u32; 5] {
        self.counts
    }
    /// Encode proposed20.33 business summary:mask:u8,5*(threshold:u16,count:u32)=31B.
    /// # Errors
    /// Short output returns `Capacity` before writing.
    pub fn encode_summary(&self, output: &mut [u8]) -> Result<usize, Error> {
        if output.len() < 31 {
            return Err(Error::Capacity);
        }
        output[0] = 0x1f;
        for (i, (threshold, count)) in self.thresholds.0.iter().zip(self.counts).enumerate() {
            let offset = 1 + i * 6;
            output[offset..offset + 2].copy_from_slice(&threshold.to_le_bytes());
            output[offset + 2..offset + 6].copy_from_slice(&count.to_le_bytes());
        }
        Ok(31)
    }
    /// Encode proposed module-schema2 E82:context:u32 followed by the31B summary, no public status.
    /// # Errors
    /// Short output returns `Capacity` before writing.
    pub fn encode_event(&self, output: &mut [u8]) -> Result<usize, Error> {
        if output.len() < 35 {
            return Err(Error::Capacity);
        }
        output[..4].copy_from_slice(&self.context_id.to_le_bytes());
        self.encode_summary(&mut output[4..])?;
        Ok(35)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_should_not_wrap_or_advance_sample_on_counter_exhaustion() {
        let thresholds =
            ZoneThresholds::new([90, 120, 150, 180, 220]).unwrap_or_else(|e| panic!("{e:?}"));
        let mut zones = ZoneCounter::new(1, thresholds).unwrap_or_else(|e| panic!("{e:?}"));
        zones.counts[0] = u32::MAX;
        assert_eq!(zones.observe(1, Some(60)), Err(Error::Exhausted));
        assert_eq!(zones.counts[0], u32::MAX);
        assert_eq!(zones.last_sample, None);
    }
}
