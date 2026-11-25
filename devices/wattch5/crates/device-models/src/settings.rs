//! Optional application payloads, candidate V0.6 definitions.
use device_protocol::ProtocolError;
/// Units encoded value; Debug intentionally omits field contents.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Units {
    /// Wire field `distance_unit`; lengths and units follow the V0.6 candidate contract.
    pub distance_unit: u8,
    /// Wire field `temp_unit`; lengths and units follow the V0.6 candidate contract.
    pub temp_unit: u8,
}
impl core::fmt::Debug for Units {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Units")
            .field("encoded_length", &Self::LENGTH)
            .finish_non_exhaustive()
    }
}
impl Units {
    /// Exact encoded length.
    pub const LENGTH: usize = 2;
    /// Decode and validate a complete value.
    /// # Errors
    /// Rejects unknown enum/mask bits, noncanonical absent fields and incorrect lengths.
    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        if bytes.len() != Self::LENGTH {
            return Err(ProtocolError::InvalidLength);
        }
        let result = Self {
            distance_unit: bytes[0],
            temp_unit: bytes[1],
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
        out[0] = self.distance_unit;
        out[1] = self.temp_unit;
        Ok(Self::LENGTH)
    }
    /// Validate the value without executing any device behavior.
    /// # Errors
    /// Rejects out-of-contract values.
    pub fn validate(self) -> Result<(), ProtocolError> {
        if self.distance_unit > 1 || self.temp_unit > 1 {
            return Err(ProtocolError::InvalidState);
        }
        Ok(())
    }
}
/// `HourFormat` encoded value; Debug intentionally omits field contents.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct HourFormat {
    /// Wire field `hour_format`; lengths and units follow the V0.6 candidate contract.
    pub hour_format: u8,
}
impl core::fmt::Debug for HourFormat {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HourFormat")
            .field("encoded_length", &Self::LENGTH)
            .finish_non_exhaustive()
    }
}
impl HourFormat {
    /// Exact encoded length.
    pub const LENGTH: usize = 1;
    /// Decode and validate a complete value.
    /// # Errors
    /// Rejects unknown enum/mask bits, noncanonical absent fields and incorrect lengths.
    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        if bytes.len() != Self::LENGTH {
            return Err(ProtocolError::InvalidLength);
        }
        let result = Self {
            hour_format: bytes[0],
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
        out[0] = self.hour_format;
        Ok(Self::LENGTH)
    }
    /// Validate the value without executing any device behavior.
    /// # Errors
    /// Rejects out-of-contract values.
    pub fn validate(self) -> Result<(), ProtocolError> {
        if self.hour_format > 1 {
            return Err(ProtocolError::InvalidState);
        }
        Ok(())
    }
}
/// `TimeWindow` encoded value; Debug intentionally omits field contents.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TimeWindow {
    /// Wire field `enabled`; lengths and units follow the V0.6 candidate contract.
    pub enabled: u8,
    /// Wire field `weekday_mask`; lengths and units follow the V0.6 candidate contract.
    pub weekday_mask: u8,
    /// Wire field `start_min`; lengths and units follow the V0.6 candidate contract.
    pub start_min: u16,
    /// Wire field `end_min`; lengths and units follow the V0.6 candidate contract.
    pub end_min: u16,
}
impl core::fmt::Debug for TimeWindow {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TimeWindow")
            .field("encoded_length", &Self::LENGTH)
            .finish_non_exhaustive()
    }
}
impl TimeWindow {
    /// Exact encoded length.
    pub const LENGTH: usize = 6;
    /// Decode and validate a complete value.
    /// # Errors
    /// Rejects unknown enum/mask bits, noncanonical absent fields and incorrect lengths.
    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        if bytes.len() != Self::LENGTH {
            return Err(ProtocolError::InvalidLength);
        }
        let result = Self {
            enabled: bytes[0],
            weekday_mask: bytes[1],
            start_min: u16::from_le_bytes([bytes[2], bytes[3]]),
            end_min: u16::from_le_bytes([bytes[4], bytes[5]]),
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
        out[0] = self.enabled;
        out[1] = self.weekday_mask;
        out[2..4].copy_from_slice(&self.start_min.to_le_bytes());
        out[4..6].copy_from_slice(&self.end_min.to_le_bytes());
        Ok(Self::LENGTH)
    }
    /// Validate the value without executing any device behavior.
    /// # Errors
    /// Rejects out-of-contract values.
    pub fn validate(self) -> Result<(), ProtocolError> {
        if self.enabled > 1 || self.weekday_mask & 0x80 != 0 {
            return Err(ProtocolError::InvalidState);
        }
        if self.start_min >= 1440 || self.end_min >= 1440 {
            return Err(ProtocolError::InvalidLength);
        }
        Ok(())
    }
}
