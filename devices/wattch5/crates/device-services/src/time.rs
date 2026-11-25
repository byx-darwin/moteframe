//! Numerical UTC/monotonic mapping. Physical clock calibration is supplied by the adapter.
use device_protocol::ProtocolError;
/// Time conversion failures, never a fabricated timestamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeError {
    /// Caller-provided monotonic clock moved backwards.
    NonMonotonic,
    /// UTC, drift budget or uncertainty cannot be represented.
    Overflow,
}
/// Module01 time-mapping payload, exactly21 bytes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct UtcMapping {
    /// UTC milliseconds; zero when `valid` is false.
    pub utc_ms: i64,
    /// Adapter-provided monotonic microseconds, unaffected by UTC sync.
    pub mono_us: u64,
    /// Conservative current uncertainty in milliseconds.
    pub uncertainty_ms: u32,
    /// Whether UTC is available. False is never interpreted as the Unix epoch.
    pub valid: bool,
}
impl core::fmt::Debug for UtcMapping {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("UtcMapping")
            .field("encoded_length", &21)
            .field("valid", &self.valid)
            .finish_non_exhaustive()
    }
}
impl UtcMapping {
    /// Decode exact little-endian mapping data, excluding the common status prefix.
    /// # Errors
    /// Rejects length, invalid boolean or a noncanonical unavailable timestamp.
    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        if bytes.len() != 21 {
            return Err(ProtocolError::InvalidLength);
        }
        let utc_ms = i64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]);
        let mono_us = u64::from_le_bytes([
            bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
        ]);
        let uncertainty_ms = u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
        if bytes[20] > 1 || (bytes[20] == 0 && utc_ms != 0) {
            return Err(ProtocolError::InvalidState);
        }
        Ok(Self {
            utc_ms,
            mono_us,
            uncertainty_ms,
            valid: bytes[20] == 1,
        })
    }
    /// Encode a mapping without touching an RTC or changing its validity.
    /// # Errors
    /// Rejects absent UTC with nonzero value, or insufficient output capacity.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, ProtocolError> {
        if !self.valid && self.utc_ms != 0 {
            return Err(ProtocolError::InvalidState);
        }
        if out.len() < 21 {
            return Err(ProtocolError::Capacity);
        }
        out[..8].copy_from_slice(&self.utc_ms.to_le_bytes());
        out[8..16].copy_from_slice(&self.mono_us.to_le_bytes());
        out[16..20].copy_from_slice(&self.uncertainty_ms.to_le_bytes());
        out[20] = u8::from(self.valid);
        Ok(21)
    }
}
/// UTC mapping against an external monotonic source. No internal clock or oscillator claim.
pub struct SoftwareClock {
    anchor: Option<(i64, u64, u32)>,
    last: Option<u64>,
    max_drift_ppm: u32,
}
impl core::fmt::Debug for SoftwareClock {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SoftwareClock")
            .field("synchronized", &self.anchor.is_some())
            .finish_non_exhaustive()
    }
}
impl SoftwareClock {
    /// Configure a conservative monotonic-source drift bound, supplied by the physical adapter.
    /// Zero is appropriate only for an exact externally supplied/test clock, not an uncalibrated oscillator.
    #[must_use]
    pub const fn new(max_drift_ppm: u32) -> Self {
        Self {
            anchor: None,
            last: None,
            max_drift_ppm,
        }
    }
    /// Set UTC from an independently authorized time source; monotonic time is never reset.
    /// # Errors
    /// Rejects backwards monotonic input.
    pub fn synchronize(
        &mut self,
        utc_ms: i64,
        uncertainty_ms: u32,
        mono_us: u64,
    ) -> Result<(), TimeError> {
        self.observe(mono_us)?;
        self.anchor = Some((utc_ms, mono_us, uncertainty_ms));
        Ok(())
    }
    /// Map current monotonic time with accumulated drift and submillisecond rounding uncertainty.
    /// # Errors
    /// Rejects backwards input or any unrepresentable timestamp/uncertainty; never wraps.
    pub fn read(&mut self, mono_us: u64) -> Result<UtcMapping, TimeError> {
        self.observe(mono_us)?;
        let Some((utc, anchor, base_uncertainty)) = self.anchor else {
            return Ok(UtcMapping {
                utc_ms: 0,
                mono_us,
                uncertainty_ms: 0,
                valid: false,
            });
        };
        let elapsed = mono_us.checked_sub(anchor).ok_or(TimeError::NonMonotonic)?;
        let delta = i64::try_from(elapsed / 1000).map_err(|_| TimeError::Overflow)?;
        let utc_ms = utc.checked_add(delta).ok_or(TimeError::Overflow)?;
        let drift = elapsed
            .checked_mul(u64::from(self.max_drift_ppm))
            .ok_or(TimeError::Overflow)?
            .div_ceil(1_000_000_000);
        let uncertainty_ms = base_uncertainty
            .checked_add(u32::try_from(drift).map_err(|_| TimeError::Overflow)?)
            .and_then(|v| v.checked_add(u32::from(!elapsed.is_multiple_of(1000))))
            .ok_or(TimeError::Overflow)?;
        Ok(UtcMapping {
            utc_ms,
            mono_us,
            uncertainty_ms,
            valid: true,
        })
    }
    /// Mark UTC unavailable without rewinding the monotonic source.
    pub fn invalidate(&mut self) {
        self.anchor = None;
    }
    fn observe(&mut self, now: u64) -> Result<(), TimeError> {
        if self.last.is_some_and(|last| now < last) {
            return Err(TimeError::NonMonotonic);
        }
        self.last = Some(now);
        Ok(())
    }
}
