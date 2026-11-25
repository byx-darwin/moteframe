//! Explicit fixed-layout motion records. Semantic source/algorithm availability is a backend responsibility.
use crate::MotionError;
#[cfg(feature = "sleep")]
/// `SleepInterval` schema body, excluding the common history envelope/CRC.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SleepInterval {
    /// Encoded contract field `duration_ms`; units follow the versioned motion schema.
    pub duration_ms: u32,
    /// Encoded contract field `stage`; units follow the versioned motion schema.
    pub stage: u8,
    /// Encoded contract field `quality`; units follow the versioned motion schema.
    pub quality: u8,
    /// Encoded contract field `flags`; units follow the versioned motion schema.
    pub flags: u8,
}
#[cfg(feature = "sleep")]
impl core::fmt::Debug for SleepInterval {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SleepInterval")
            .field("encoded_length", &Self::LENGTH)
            .finish_non_exhaustive()
    }
}
#[cfg(feature = "sleep")]
impl SleepInterval {
    /// Exact body length.
    pub const LENGTH: usize = 7;
    /// Decode a complete typed record.
    /// # Errors
    /// Rejects length, reserved fields, impossible enum/mask combinations and noncanonical absent values.
    pub fn decode(bytes: &[u8]) -> Result<Self, MotionError> {
        if bytes.len() != Self::LENGTH {
            return Err(MotionError::Invalid);
        }
        let value = Self {
            duration_ms: u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            stage: bytes[4],
            quality: bytes[5],
            flags: bytes[6],
        };
        value.validate()?;
        Ok(value)
    }
    /// Encode a validated record, without asserting a real algorithm produced it.
    /// # Errors
    /// Rejects invalid fields or insufficient output capacity.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, MotionError> {
        self.validate()?;
        if out.len() < Self::LENGTH {
            return Err(MotionError::Capacity);
        }
        out[0..4].copy_from_slice(&self.duration_ms.to_le_bytes());
        out[4] = self.stage;
        out[5] = self.quality;
        out[6] = self.flags;
        Ok(Self::LENGTH)
    }
    /// Check enum, reserved-bit and absent-field invariants.
    /// # Errors
    /// Rejects invalid wire semantics, without clinical interpretation.
    pub fn validate(self) -> Result<(), MotionError> {
        if self.stage > 5 || (self.quality > 100 && self.quality != 255) || self.flags & !1 != 0 {
            return Err(MotionError::Invalid);
        }
        Ok(())
    }
}
#[cfg(feature = "sleep")]
/// `SleepProvisional` schema body, excluding the common history envelope/CRC.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SleepProvisional {
    /// Encoded contract field `duration_ms`; units follow the versioned motion schema.
    pub duration_ms: u32,
    /// Encoded contract field `stage`; units follow the versioned motion schema.
    pub stage: u8,
    /// Encoded contract field `quality`; units follow the versioned motion schema.
    pub quality: u8,
    /// Encoded contract field `flags`; units follow the versioned motion schema.
    pub flags: u8,
    /// Encoded contract field `provisional`; units follow the versioned motion schema.
    pub provisional: u8,
}
#[cfg(feature = "sleep")]
impl core::fmt::Debug for SleepProvisional {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SleepProvisional")
            .field("encoded_length", &Self::LENGTH)
            .finish_non_exhaustive()
    }
}
#[cfg(feature = "sleep")]
impl SleepProvisional {
    /// Exact body length.
    pub const LENGTH: usize = 8;
    /// Decode a complete typed record.
    /// # Errors
    /// Rejects length, reserved fields, impossible enum/mask combinations and noncanonical absent values.
    pub fn decode(bytes: &[u8]) -> Result<Self, MotionError> {
        if bytes.len() != Self::LENGTH {
            return Err(MotionError::Invalid);
        }
        let value = Self {
            duration_ms: u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            stage: bytes[4],
            quality: bytes[5],
            flags: bytes[6],
            provisional: bytes[7],
        };
        value.validate()?;
        Ok(value)
    }
    /// Encode a validated record, without asserting a real algorithm produced it.
    /// # Errors
    /// Rejects invalid fields or insufficient output capacity.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, MotionError> {
        self.validate()?;
        if out.len() < Self::LENGTH {
            return Err(MotionError::Capacity);
        }
        out[0..4].copy_from_slice(&self.duration_ms.to_le_bytes());
        out[4] = self.stage;
        out[5] = self.quality;
        out[6] = self.flags;
        out[7] = self.provisional;
        Ok(Self::LENGTH)
    }
    /// Check enum, reserved-bit and absent-field invariants.
    /// # Errors
    /// Rejects invalid wire semantics, without clinical interpretation.
    pub fn validate(self) -> Result<(), MotionError> {
        if self.stage > 5 || (self.quality > 100 && self.quality != 255) || self.flags & !1 != 0 {
            return Err(MotionError::Invalid);
        }
        if self.provisional != 1 {
            return Err(MotionError::Invalid);
        }
        Ok(())
    }
}
#[cfg(feature = "sleep")]
/// `SleepDetection` schema body, excluding the common history envelope/CRC.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SleepDetection {
    /// Encoded contract field `state`; units follow the versioned motion schema.
    pub state: u8,
    /// Encoded contract field `wear`; units follow the versioned motion schema.
    pub wear: u8,
    /// Encoded contract field `active_ms`; units follow the versioned motion schema.
    pub active_ms: u32,
    /// Encoded contract field `sleep_ms`; units follow the versioned motion schema.
    pub sleep_ms: u32,
    /// Encoded contract field `deep_ms`; units follow the versioned motion schema.
    pub deep_ms: u32,
    /// Encoded contract field `light_ms`; units follow the versioned motion schema.
    pub light_ms: u32,
}
#[cfg(feature = "sleep")]
impl core::fmt::Debug for SleepDetection {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SleepDetection")
            .field("encoded_length", &Self::LENGTH)
            .finish_non_exhaustive()
    }
}
#[cfg(feature = "sleep")]
impl SleepDetection {
    /// Exact body length.
    pub const LENGTH: usize = 18;
    /// Decode a complete typed record.
    /// # Errors
    /// Rejects length, reserved fields, impossible enum/mask combinations and noncanonical absent values.
    pub fn decode(bytes: &[u8]) -> Result<Self, MotionError> {
        if bytes.len() != Self::LENGTH {
            return Err(MotionError::Invalid);
        }
        let value = Self {
            state: bytes[0],
            wear: bytes[1],
            active_ms: u32::from_le_bytes([bytes[2], bytes[3], bytes[4], bytes[5]]),
            sleep_ms: u32::from_le_bytes([bytes[6], bytes[7], bytes[8], bytes[9]]),
            deep_ms: u32::from_le_bytes([bytes[10], bytes[11], bytes[12], bytes[13]]),
            light_ms: u32::from_le_bytes([bytes[14], bytes[15], bytes[16], bytes[17]]),
        };
        value.validate()?;
        Ok(value)
    }
    /// Encode a validated record, without asserting a real algorithm produced it.
    /// # Errors
    /// Rejects invalid fields or insufficient output capacity.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, MotionError> {
        self.validate()?;
        if out.len() < Self::LENGTH {
            return Err(MotionError::Capacity);
        }
        out[0] = self.state;
        out[1] = self.wear;
        out[2..6].copy_from_slice(&self.active_ms.to_le_bytes());
        out[6..10].copy_from_slice(&self.sleep_ms.to_le_bytes());
        out[10..14].copy_from_slice(&self.deep_ms.to_le_bytes());
        out[14..18].copy_from_slice(&self.light_ms.to_le_bytes());
        Ok(Self::LENGTH)
    }
    /// Check enum, reserved-bit and absent-field invariants.
    /// # Errors
    /// Rejects invalid wire semantics, without clinical interpretation.
    pub fn validate(self) -> Result<(), MotionError> {
        if self.state > 5
            || self.wear > 2
            || self
                .deep_ms
                .checked_add(self.light_ms)
                .ok_or(MotionError::Capacity)?
                > self.sleep_ms
        {
            return Err(MotionError::Invalid);
        }
        Ok(())
    }
}
#[cfg(feature = "activity")]
/// `ActivityRecord` schema body, excluding the common history envelope/CRC.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ActivityRecord {
    /// Encoded contract field `kind`; units follow the versioned motion schema.
    pub kind: u8,
    /// Encoded contract field `start_utc_ms`; units follow the versioned motion schema.
    pub start_utc_ms: i64,
    /// Encoded contract field `end_utc_ms`; units follow the versioned motion schema.
    pub end_utc_ms: i64,
    /// Encoded contract field `steps`; units follow the versioned motion schema.
    pub steps: u32,
    /// Encoded contract field `distance_mm`; units follow the versioned motion schema.
    pub distance_mm: u32,
    /// Encoded contract field `energy_j`; units follow the versioned motion schema.
    pub energy_j: u32,
    /// Encoded contract field `valid_mask`; units follow the versioned motion schema.
    pub valid_mask: u16,
}
#[cfg(feature = "activity")]
impl core::fmt::Debug for ActivityRecord {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ActivityRecord")
            .field("encoded_length", &Self::LENGTH)
            .finish_non_exhaustive()
    }
}
#[cfg(feature = "activity")]
impl ActivityRecord {
    /// Exact body length.
    pub const LENGTH: usize = 31;
    /// Decode a complete typed record.
    /// # Errors
    /// Rejects length, reserved fields, impossible enum/mask combinations and noncanonical absent values.
    pub fn decode(bytes: &[u8]) -> Result<Self, MotionError> {
        if bytes.len() != Self::LENGTH {
            return Err(MotionError::Invalid);
        }
        let value = Self {
            kind: bytes[0],
            start_utc_ms: i64::from_le_bytes([
                bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7], bytes[8],
            ]),
            end_utc_ms: i64::from_le_bytes([
                bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
                bytes[16],
            ]),
            steps: u32::from_le_bytes([bytes[17], bytes[18], bytes[19], bytes[20]]),
            distance_mm: u32::from_le_bytes([bytes[21], bytes[22], bytes[23], bytes[24]]),
            energy_j: u32::from_le_bytes([bytes[25], bytes[26], bytes[27], bytes[28]]),
            valid_mask: u16::from_le_bytes([bytes[29], bytes[30]]),
        };
        value.validate()?;
        Ok(value)
    }
    /// Encode a validated record, without asserting a real algorithm produced it.
    /// # Errors
    /// Rejects invalid fields or insufficient output capacity.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, MotionError> {
        self.validate()?;
        if out.len() < Self::LENGTH {
            return Err(MotionError::Capacity);
        }
        out[0] = self.kind;
        out[1..9].copy_from_slice(&self.start_utc_ms.to_le_bytes());
        out[9..17].copy_from_slice(&self.end_utc_ms.to_le_bytes());
        out[17..21].copy_from_slice(&self.steps.to_le_bytes());
        out[21..25].copy_from_slice(&self.distance_mm.to_le_bytes());
        out[25..29].copy_from_slice(&self.energy_j.to_le_bytes());
        out[29..31].copy_from_slice(&self.valid_mask.to_le_bytes());
        Ok(Self::LENGTH)
    }
    /// Check enum, reserved-bit and absent-field invariants.
    /// # Errors
    /// Rejects invalid wire semantics, without clinical interpretation.
    pub fn validate(self) -> Result<(), MotionError> {
        if !(1..=4).contains(&self.kind) || self.valid_mask & !31 != 0 {
            return Err(MotionError::Invalid);
        }
        canonical(
            self.valid_mask,
            &[
                self.start_utc_ms != 0,
                self.end_utc_ms != 0,
                self.steps != 0,
                self.distance_mm != 0,
                self.energy_j != 0,
            ],
        )?;
        Ok(())
    }
}
#[cfg(feature = "wear")]
/// `WearInterval` schema body, excluding the common history envelope/CRC.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct WearInterval {
    /// Encoded contract field `start_utc_ms`; units follow the versioned motion schema.
    pub start_utc_ms: i64,
    /// Encoded contract field `end_utc_ms`; units follow the versioned motion schema.
    pub end_utc_ms: i64,
    /// Encoded contract field `state`; units follow the versioned motion schema.
    pub state: u8,
    /// Encoded contract field `valid_mask`; units follow the versioned motion schema.
    pub valid_mask: u8,
    /// Encoded contract field `provisional`; units follow the versioned motion schema.
    pub provisional: u8,
}
#[cfg(feature = "wear")]
impl core::fmt::Debug for WearInterval {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("WearInterval")
            .field("encoded_length", &Self::LENGTH)
            .finish_non_exhaustive()
    }
}
#[cfg(feature = "wear")]
impl WearInterval {
    /// Exact body length.
    pub const LENGTH: usize = 19;
    /// Decode a complete typed record.
    /// # Errors
    /// Rejects length, reserved fields, impossible enum/mask combinations and noncanonical absent values.
    pub fn decode(bytes: &[u8]) -> Result<Self, MotionError> {
        if bytes.len() != Self::LENGTH {
            return Err(MotionError::Invalid);
        }
        let value = Self {
            start_utc_ms: i64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]),
            end_utc_ms: i64::from_le_bytes([
                bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14],
                bytes[15],
            ]),
            state: bytes[16],
            valid_mask: bytes[17],
            provisional: bytes[18],
        };
        value.validate()?;
        Ok(value)
    }
    /// Encode a validated record, without asserting a real algorithm produced it.
    /// # Errors
    /// Rejects invalid fields or insufficient output capacity.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, MotionError> {
        self.validate()?;
        if out.len() < Self::LENGTH {
            return Err(MotionError::Capacity);
        }
        out[0..8].copy_from_slice(&self.start_utc_ms.to_le_bytes());
        out[8..16].copy_from_slice(&self.end_utc_ms.to_le_bytes());
        out[16] = self.state;
        out[17] = self.valid_mask;
        out[18] = self.provisional;
        Ok(Self::LENGTH)
    }
    /// Check enum, reserved-bit and absent-field invariants.
    /// # Errors
    /// Rejects invalid wire semantics, without clinical interpretation.
    pub fn validate(self) -> Result<(), MotionError> {
        if self.state > 2 || self.valid_mask & !7 != 0 || self.provisional > 1 {
            return Err(MotionError::Invalid);
        }
        canonical(
            u16::from(self.valid_mask),
            &[
                self.start_utc_ms != 0,
                self.end_utc_ms != 0,
                self.state != 0,
            ],
        )?;
        if self.provisional == 1 && (self.end_utc_ms != 0 || self.valid_mask & 2 != 0) {
            return Err(MotionError::Invalid);
        }
        Ok(())
    }
}
#[cfg(feature = "wear")]
/// `PostureInterval` schema body, excluding the common history envelope/CRC.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PostureInterval {
    /// Encoded contract field `start_utc_ms`; units follow the versioned motion schema.
    pub start_utc_ms: i64,
    /// Encoded contract field `end_utc_ms`; units follow the versioned motion schema.
    pub end_utc_ms: i64,
    /// Encoded contract field `posture`; units follow the versioned motion schema.
    pub posture: u8,
    /// Encoded contract field `valid_mask`; units follow the versioned motion schema.
    pub valid_mask: u8,
    /// Encoded contract field `provisional`; units follow the versioned motion schema.
    pub provisional: u8,
}
#[cfg(feature = "wear")]
impl core::fmt::Debug for PostureInterval {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PostureInterval")
            .field("encoded_length", &Self::LENGTH)
            .finish_non_exhaustive()
    }
}
#[cfg(feature = "wear")]
impl PostureInterval {
    /// Exact body length.
    pub const LENGTH: usize = 19;
    /// Decode a complete typed record.
    /// # Errors
    /// Rejects length, reserved fields, impossible enum/mask combinations and noncanonical absent values.
    pub fn decode(bytes: &[u8]) -> Result<Self, MotionError> {
        if bytes.len() != Self::LENGTH {
            return Err(MotionError::Invalid);
        }
        let value = Self {
            start_utc_ms: i64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]),
            end_utc_ms: i64::from_le_bytes([
                bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14],
                bytes[15],
            ]),
            posture: bytes[16],
            valid_mask: bytes[17],
            provisional: bytes[18],
        };
        value.validate()?;
        Ok(value)
    }
    /// Encode a validated record, without asserting a real algorithm produced it.
    /// # Errors
    /// Rejects invalid fields or insufficient output capacity.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, MotionError> {
        self.validate()?;
        if out.len() < Self::LENGTH {
            return Err(MotionError::Capacity);
        }
        out[0..8].copy_from_slice(&self.start_utc_ms.to_le_bytes());
        out[8..16].copy_from_slice(&self.end_utc_ms.to_le_bytes());
        out[16] = self.posture;
        out[17] = self.valid_mask;
        out[18] = self.provisional;
        Ok(Self::LENGTH)
    }
    /// Check enum, reserved-bit and absent-field invariants.
    /// # Errors
    /// Rejects invalid wire semantics, without clinical interpretation.
    pub fn validate(self) -> Result<(), MotionError> {
        if !(self.posture <= 1 || self.posture == 255)
            || self.valid_mask & !7 != 0
            || self.provisional > 1
        {
            return Err(MotionError::Invalid);
        }
        canonical(
            u16::from(self.valid_mask),
            &[
                self.start_utc_ms != 0,
                self.end_utc_ms != 0,
                self.posture != 0,
            ],
        )?;
        if self.provisional == 1 && (self.end_utc_ms != 0 || self.valid_mask & 2 != 0) {
            return Err(MotionError::Invalid);
        }
        Ok(())
    }
}
#[cfg(feature = "sport")]
/// `SportSummary` schema body, excluding the common history envelope/CRC.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SportSummary {
    /// Encoded contract field `session`; units follow the versioned motion schema.
    pub session: u64,
    /// Encoded contract field `start_utc_ms`; units follow the versioned motion schema.
    pub start_utc_ms: i64,
    /// Encoded contract field `end_utc_ms`; units follow the versioned motion schema.
    pub end_utc_ms: i64,
    /// Encoded contract field `duration_ms`; units follow the versioned motion schema.
    pub duration_ms: u32,
    /// Encoded contract field `sport_type`; units follow the versioned motion schema.
    pub sport_type: u16,
    /// Encoded contract field `environment`; units follow the versioned motion schema.
    pub environment: u8,
    /// Encoded contract field `count_kind`; units follow the versioned motion schema.
    pub count_kind: u8,
    /// Encoded contract field `count`; units follow the versioned motion schema.
    pub count: u32,
    /// Encoded contract field `distance_mm`; units follow the versioned motion schema.
    pub distance_mm: u32,
    /// Encoded contract field `energy_j`; units follow the versioned motion schema.
    pub energy_j: u32,
    /// Encoded contract field `valid_mask`; units follow the versioned motion schema.
    pub valid_mask: u16,
}
#[cfg(feature = "sport")]
impl core::fmt::Debug for SportSummary {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SportSummary")
            .field("encoded_length", &Self::LENGTH)
            .finish_non_exhaustive()
    }
}
#[cfg(feature = "sport")]
impl SportSummary {
    /// Exact body length.
    pub const LENGTH: usize = 46;
    /// Decode a complete typed record.
    /// # Errors
    /// Rejects length, reserved fields, impossible enum/mask combinations and noncanonical absent values.
    pub fn decode(bytes: &[u8]) -> Result<Self, MotionError> {
        if bytes.len() != Self::LENGTH {
            return Err(MotionError::Invalid);
        }
        let value = Self {
            session: u64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]),
            start_utc_ms: i64::from_le_bytes([
                bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14],
                bytes[15],
            ]),
            end_utc_ms: i64::from_le_bytes([
                bytes[16], bytes[17], bytes[18], bytes[19], bytes[20], bytes[21], bytes[22],
                bytes[23],
            ]),
            duration_ms: u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]),
            sport_type: u16::from_le_bytes([bytes[28], bytes[29]]),
            environment: bytes[30],
            count_kind: bytes[31],
            count: u32::from_le_bytes([bytes[32], bytes[33], bytes[34], bytes[35]]),
            distance_mm: u32::from_le_bytes([bytes[36], bytes[37], bytes[38], bytes[39]]),
            energy_j: u32::from_le_bytes([bytes[40], bytes[41], bytes[42], bytes[43]]),
            valid_mask: u16::from_le_bytes([bytes[44], bytes[45]]),
        };
        value.validate()?;
        Ok(value)
    }
    /// Encode a validated record, without asserting a real algorithm produced it.
    /// # Errors
    /// Rejects invalid fields or insufficient output capacity.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, MotionError> {
        self.validate()?;
        if out.len() < Self::LENGTH {
            return Err(MotionError::Capacity);
        }
        out[0..8].copy_from_slice(&self.session.to_le_bytes());
        out[8..16].copy_from_slice(&self.start_utc_ms.to_le_bytes());
        out[16..24].copy_from_slice(&self.end_utc_ms.to_le_bytes());
        out[24..28].copy_from_slice(&self.duration_ms.to_le_bytes());
        out[28..30].copy_from_slice(&self.sport_type.to_le_bytes());
        out[30] = self.environment;
        out[31] = self.count_kind;
        out[32..36].copy_from_slice(&self.count.to_le_bytes());
        out[36..40].copy_from_slice(&self.distance_mm.to_le_bytes());
        out[40..44].copy_from_slice(&self.energy_j.to_le_bytes());
        out[44..46].copy_from_slice(&self.valid_mask.to_le_bytes());
        Ok(Self::LENGTH)
    }
    /// Check enum, reserved-bit and absent-field invariants.
    /// # Errors
    /// Rejects invalid wire semantics, without clinical interpretation.
    pub fn validate(self) -> Result<(), MotionError> {
        if self.session == 0
            || self.sport_type > 23
            || self.environment > 2
            || self.count_kind > 3
            || self.valid_mask & !63 != 0
            || (self.count_kind == 0 && self.valid_mask & 8 != 0)
        {
            return Err(MotionError::Invalid);
        }
        canonical(
            self.valid_mask,
            &[
                self.start_utc_ms != 0,
                self.end_utc_ms != 0,
                self.duration_ms != 0,
                self.count != 0,
                self.distance_mm != 0,
                self.energy_j != 0,
            ],
        )?;
        Ok(())
    }
}
#[cfg(feature = "sport")]
/// `SportRealtime` schema body, excluding the common history envelope/CRC.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SportRealtime {
    /// Encoded contract field `session`; units follow the versioned motion schema.
    pub session: u64,
    /// Encoded contract field `time_offset_ms`; units follow the versioned motion schema.
    pub time_offset_ms: u32,
    /// Encoded contract field `valid_mask`; units follow the versioned motion schema.
    pub valid_mask: u16,
    /// Encoded contract field `bpm`; units follow the versioned motion schema.
    pub bpm: u16,
    /// Encoded contract field `cadence_per_min`; units follow the versioned motion schema.
    pub cadence_per_min: u16,
    /// Encoded contract field `steps`; units follow the versioned motion schema.
    pub steps: u32,
    /// Encoded contract field `rri_us`; units follow the versioned motion schema.
    pub rri_us: u32,
}
#[cfg(feature = "sport")]
impl core::fmt::Debug for SportRealtime {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SportRealtime")
            .field("encoded_length", &Self::LENGTH)
            .finish_non_exhaustive()
    }
}
#[cfg(feature = "sport")]
impl SportRealtime {
    /// Exact body length.
    pub const LENGTH: usize = 26;
    /// Decode a complete typed record.
    /// # Errors
    /// Rejects length, reserved fields, impossible enum/mask combinations and noncanonical absent values.
    pub fn decode(bytes: &[u8]) -> Result<Self, MotionError> {
        if bytes.len() != Self::LENGTH {
            return Err(MotionError::Invalid);
        }
        let value = Self {
            session: u64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]),
            time_offset_ms: u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            valid_mask: u16::from_le_bytes([bytes[12], bytes[13]]),
            bpm: u16::from_le_bytes([bytes[14], bytes[15]]),
            cadence_per_min: u16::from_le_bytes([bytes[16], bytes[17]]),
            steps: u32::from_le_bytes([bytes[18], bytes[19], bytes[20], bytes[21]]),
            rri_us: u32::from_le_bytes([bytes[22], bytes[23], bytes[24], bytes[25]]),
        };
        value.validate()?;
        Ok(value)
    }
    /// Encode a validated record, without asserting a real algorithm produced it.
    /// # Errors
    /// Rejects invalid fields or insufficient output capacity.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, MotionError> {
        self.validate()?;
        if out.len() < Self::LENGTH {
            return Err(MotionError::Capacity);
        }
        out[0..8].copy_from_slice(&self.session.to_le_bytes());
        out[8..12].copy_from_slice(&self.time_offset_ms.to_le_bytes());
        out[12..14].copy_from_slice(&self.valid_mask.to_le_bytes());
        out[14..16].copy_from_slice(&self.bpm.to_le_bytes());
        out[16..18].copy_from_slice(&self.cadence_per_min.to_le_bytes());
        out[18..22].copy_from_slice(&self.steps.to_le_bytes());
        out[22..26].copy_from_slice(&self.rri_us.to_le_bytes());
        Ok(Self::LENGTH)
    }
    /// Check enum, reserved-bit and absent-field invariants.
    /// # Errors
    /// Rejects invalid wire semantics, without clinical interpretation.
    pub fn validate(self) -> Result<(), MotionError> {
        if self.session == 0
            || self.valid_mask & !15 != 0
            || (self.valid_mask & 1 != 0 && self.bpm == 0)
            || (self.valid_mask & 8 != 0 && self.rri_us == 0)
        {
            return Err(MotionError::Invalid);
        }
        canonical(
            self.valid_mask,
            &[
                self.bpm != 0,
                self.cadence_per_min != 0,
                self.steps != 0,
                self.rri_us != 0,
            ],
        )?;
        Ok(())
    }
}
#[cfg(feature = "sport")]
/// `SportHeartSample` schema body, excluding the common history envelope/CRC.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SportHeartSample {
    /// Encoded contract field `session`; units follow the versioned motion schema.
    pub session: u64,
    /// Encoded contract field `index`; units follow the versioned motion schema.
    pub index: u32,
    /// Encoded contract field `time_offset_ms`; units follow the versioned motion schema.
    pub time_offset_ms: u32,
    /// Encoded contract field `bpm`; units follow the versioned motion schema.
    pub bpm: u16,
}
#[cfg(feature = "sport")]
impl core::fmt::Debug for SportHeartSample {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SportHeartSample")
            .field("encoded_length", &Self::LENGTH)
            .finish_non_exhaustive()
    }
}
#[cfg(feature = "sport")]
impl SportHeartSample {
    /// Exact body length.
    pub const LENGTH: usize = 18;
    /// Decode a complete typed record.
    /// # Errors
    /// Rejects length, reserved fields, impossible enum/mask combinations and noncanonical absent values.
    pub fn decode(bytes: &[u8]) -> Result<Self, MotionError> {
        if bytes.len() != Self::LENGTH {
            return Err(MotionError::Invalid);
        }
        let value = Self {
            session: u64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]),
            index: u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            time_offset_ms: u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
            bpm: u16::from_le_bytes([bytes[16], bytes[17]]),
        };
        value.validate()?;
        Ok(value)
    }
    /// Encode a validated record, without asserting a real algorithm produced it.
    /// # Errors
    /// Rejects invalid fields or insufficient output capacity.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, MotionError> {
        self.validate()?;
        if out.len() < Self::LENGTH {
            return Err(MotionError::Capacity);
        }
        out[0..8].copy_from_slice(&self.session.to_le_bytes());
        out[8..12].copy_from_slice(&self.index.to_le_bytes());
        out[12..16].copy_from_slice(&self.time_offset_ms.to_le_bytes());
        out[16..18].copy_from_slice(&self.bpm.to_le_bytes());
        Ok(Self::LENGTH)
    }
    /// Check enum, reserved-bit and absent-field invariants.
    /// # Errors
    /// Rejects invalid wire semantics, without clinical interpretation.
    pub fn validate(self) -> Result<(), MotionError> {
        if self.session == 0 || self.bpm == 0 {
            return Err(MotionError::Invalid);
        }
        Ok(())
    }
}
#[cfg(feature = "sport")]
/// `SportCadenceSample` schema body, excluding the common history envelope/CRC.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SportCadenceSample {
    /// Encoded contract field `session`; units follow the versioned motion schema.
    pub session: u64,
    /// Encoded contract field `index`; units follow the versioned motion schema.
    pub index: u32,
    /// Encoded contract field `time_offset_ms`; units follow the versioned motion schema.
    pub time_offset_ms: u32,
    /// Encoded contract field `cadence_per_min`; units follow the versioned motion schema.
    pub cadence_per_min: u16,
}
#[cfg(feature = "sport")]
impl core::fmt::Debug for SportCadenceSample {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SportCadenceSample")
            .field("encoded_length", &Self::LENGTH)
            .finish_non_exhaustive()
    }
}
#[cfg(feature = "sport")]
impl SportCadenceSample {
    /// Exact body length.
    pub const LENGTH: usize = 18;
    /// Decode a complete typed record.
    /// # Errors
    /// Rejects length, reserved fields, impossible enum/mask combinations and noncanonical absent values.
    pub fn decode(bytes: &[u8]) -> Result<Self, MotionError> {
        if bytes.len() != Self::LENGTH {
            return Err(MotionError::Invalid);
        }
        let value = Self {
            session: u64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]),
            index: u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            time_offset_ms: u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
            cadence_per_min: u16::from_le_bytes([bytes[16], bytes[17]]),
        };
        value.validate()?;
        Ok(value)
    }
    /// Encode a validated record, without asserting a real algorithm produced it.
    /// # Errors
    /// Rejects invalid fields or insufficient output capacity.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, MotionError> {
        self.validate()?;
        if out.len() < Self::LENGTH {
            return Err(MotionError::Capacity);
        }
        out[0..8].copy_from_slice(&self.session.to_le_bytes());
        out[8..12].copy_from_slice(&self.index.to_le_bytes());
        out[12..16].copy_from_slice(&self.time_offset_ms.to_le_bytes());
        out[16..18].copy_from_slice(&self.cadence_per_min.to_le_bytes());
        Ok(Self::LENGTH)
    }
    /// Check enum, reserved-bit and absent-field invariants.
    /// # Errors
    /// Rejects invalid wire semantics, without clinical interpretation.
    pub fn validate(self) -> Result<(), MotionError> {
        if self.session == 0 {
            return Err(MotionError::Invalid);
        }
        Ok(())
    }
}
#[cfg(feature = "sport")]
/// `SportRriSample` schema body, excluding the common history envelope/CRC.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SportRriSample {
    /// Encoded contract field `session`; units follow the versioned motion schema.
    pub session: u64,
    /// Encoded contract field `index`; units follow the versioned motion schema.
    pub index: u32,
    /// Encoded contract field `time_offset_ms`; units follow the versioned motion schema.
    pub time_offset_ms: u32,
    /// Encoded contract field `rri_us`; units follow the versioned motion schema.
    pub rri_us: u32,
}
#[cfg(feature = "sport")]
impl core::fmt::Debug for SportRriSample {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SportRriSample")
            .field("encoded_length", &Self::LENGTH)
            .finish_non_exhaustive()
    }
}
#[cfg(feature = "sport")]
impl SportRriSample {
    /// Exact body length.
    pub const LENGTH: usize = 20;
    /// Decode a complete typed record.
    /// # Errors
    /// Rejects length, reserved fields, impossible enum/mask combinations and noncanonical absent values.
    pub fn decode(bytes: &[u8]) -> Result<Self, MotionError> {
        if bytes.len() != Self::LENGTH {
            return Err(MotionError::Invalid);
        }
        let value = Self {
            session: u64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]),
            index: u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            time_offset_ms: u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
            rri_us: u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]),
        };
        value.validate()?;
        Ok(value)
    }
    /// Encode a validated record, without asserting a real algorithm produced it.
    /// # Errors
    /// Rejects invalid fields or insufficient output capacity.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, MotionError> {
        self.validate()?;
        if out.len() < Self::LENGTH {
            return Err(MotionError::Capacity);
        }
        out[0..8].copy_from_slice(&self.session.to_le_bytes());
        out[8..12].copy_from_slice(&self.index.to_le_bytes());
        out[12..16].copy_from_slice(&self.time_offset_ms.to_le_bytes());
        out[16..20].copy_from_slice(&self.rri_us.to_le_bytes());
        Ok(Self::LENGTH)
    }
    /// Check enum, reserved-bit and absent-field invariants.
    /// # Errors
    /// Rejects invalid wire semantics, without clinical interpretation.
    pub fn validate(self) -> Result<(), MotionError> {
        if self.session == 0 || self.rri_us == 0 {
            return Err(MotionError::Invalid);
        }
        Ok(())
    }
}
#[cfg(feature = "sport")]
/// `SportRespirationSample` schema body, excluding the common history envelope/CRC.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SportRespirationSample {
    /// Encoded contract field `session`; units follow the versioned motion schema.
    pub session: u64,
    /// Encoded contract field `index`; units follow the versioned motion schema.
    pub index: u32,
    /// Encoded contract field `time_offset_ms`; units follow the versioned motion schema.
    pub time_offset_ms: u32,
    /// Encoded contract field `respiration_x100`; units follow the versioned motion schema.
    pub respiration_x100: u16,
}
#[cfg(feature = "sport")]
impl core::fmt::Debug for SportRespirationSample {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SportRespirationSample")
            .field("encoded_length", &Self::LENGTH)
            .finish_non_exhaustive()
    }
}
#[cfg(feature = "sport")]
impl SportRespirationSample {
    /// Exact body length.
    pub const LENGTH: usize = 18;
    /// Decode a complete typed record.
    /// # Errors
    /// Rejects length, reserved fields, impossible enum/mask combinations and noncanonical absent values.
    pub fn decode(bytes: &[u8]) -> Result<Self, MotionError> {
        if bytes.len() != Self::LENGTH {
            return Err(MotionError::Invalid);
        }
        let value = Self {
            session: u64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]),
            index: u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            time_offset_ms: u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
            respiration_x100: u16::from_le_bytes([bytes[16], bytes[17]]),
        };
        value.validate()?;
        Ok(value)
    }
    /// Encode a validated record, without asserting a real algorithm produced it.
    /// # Errors
    /// Rejects invalid fields or insufficient output capacity.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, MotionError> {
        self.validate()?;
        if out.len() < Self::LENGTH {
            return Err(MotionError::Capacity);
        }
        out[0..8].copy_from_slice(&self.session.to_le_bytes());
        out[8..12].copy_from_slice(&self.index.to_le_bytes());
        out[12..16].copy_from_slice(&self.time_offset_ms.to_le_bytes());
        out[16..18].copy_from_slice(&self.respiration_x100.to_le_bytes());
        Ok(Self::LENGTH)
    }
    /// Check enum, reserved-bit and absent-field invariants.
    /// # Errors
    /// Rejects invalid wire semantics, without clinical interpretation.
    pub fn validate(self) -> Result<(), MotionError> {
        if self.session == 0 {
            return Err(MotionError::Invalid);
        }
        Ok(())
    }
}
#[cfg(feature = "sport")]
/// `SportPaceSample` schema body, excluding the common history envelope/CRC.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SportPaceSample {
    /// Encoded contract field `session`; units follow the versioned motion schema.
    pub session: u64,
    /// Encoded contract field `index`; units follow the versioned motion schema.
    pub index: u32,
    /// Encoded contract field `time_offset_ms`; units follow the versioned motion schema.
    pub time_offset_ms: u32,
    /// Encoded contract field `pace_ms_per_km`; units follow the versioned motion schema.
    pub pace_ms_per_km: u32,
}
#[cfg(feature = "sport")]
impl core::fmt::Debug for SportPaceSample {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SportPaceSample")
            .field("encoded_length", &Self::LENGTH)
            .finish_non_exhaustive()
    }
}
#[cfg(feature = "sport")]
impl SportPaceSample {
    /// Exact body length.
    pub const LENGTH: usize = 20;
    /// Decode a complete typed record.
    /// # Errors
    /// Rejects length, reserved fields, impossible enum/mask combinations and noncanonical absent values.
    pub fn decode(bytes: &[u8]) -> Result<Self, MotionError> {
        if bytes.len() != Self::LENGTH {
            return Err(MotionError::Invalid);
        }
        let value = Self {
            session: u64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]),
            index: u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            time_offset_ms: u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
            pace_ms_per_km: u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]),
        };
        value.validate()?;
        Ok(value)
    }
    /// Encode a validated record, without asserting a real algorithm produced it.
    /// # Errors
    /// Rejects invalid fields or insufficient output capacity.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, MotionError> {
        self.validate()?;
        if out.len() < Self::LENGTH {
            return Err(MotionError::Capacity);
        }
        out[0..8].copy_from_slice(&self.session.to_le_bytes());
        out[8..12].copy_from_slice(&self.index.to_le_bytes());
        out[12..16].copy_from_slice(&self.time_offset_ms.to_le_bytes());
        out[16..20].copy_from_slice(&self.pace_ms_per_km.to_le_bytes());
        Ok(Self::LENGTH)
    }
    /// Check enum, reserved-bit and absent-field invariants.
    /// # Errors
    /// Rejects invalid wire semantics, without clinical interpretation.
    pub fn validate(self) -> Result<(), MotionError> {
        if self.session == 0 {
            return Err(MotionError::Invalid);
        }
        Ok(())
    }
}
#[cfg(feature = "sport")]
/// `SportDistanceSample` schema body, excluding the common history envelope/CRC.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SportDistanceSample {
    /// Encoded contract field `session`; units follow the versioned motion schema.
    pub session: u64,
    /// Encoded contract field `index`; units follow the versioned motion schema.
    pub index: u32,
    /// Encoded contract field `time_offset_ms`; units follow the versioned motion schema.
    pub time_offset_ms: u32,
    /// Encoded contract field `distance_mm`; units follow the versioned motion schema.
    pub distance_mm: u32,
}
#[cfg(feature = "sport")]
impl core::fmt::Debug for SportDistanceSample {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SportDistanceSample")
            .field("encoded_length", &Self::LENGTH)
            .finish_non_exhaustive()
    }
}
#[cfg(feature = "sport")]
impl SportDistanceSample {
    /// Exact body length.
    pub const LENGTH: usize = 20;
    /// Decode a complete typed record.
    /// # Errors
    /// Rejects length, reserved fields, impossible enum/mask combinations and noncanonical absent values.
    pub fn decode(bytes: &[u8]) -> Result<Self, MotionError> {
        if bytes.len() != Self::LENGTH {
            return Err(MotionError::Invalid);
        }
        let value = Self {
            session: u64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]),
            index: u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            time_offset_ms: u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
            distance_mm: u32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]),
        };
        value.validate()?;
        Ok(value)
    }
    /// Encode a validated record, without asserting a real algorithm produced it.
    /// # Errors
    /// Rejects invalid fields or insufficient output capacity.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, MotionError> {
        self.validate()?;
        if out.len() < Self::LENGTH {
            return Err(MotionError::Capacity);
        }
        out[0..8].copy_from_slice(&self.session.to_le_bytes());
        out[8..12].copy_from_slice(&self.index.to_le_bytes());
        out[12..16].copy_from_slice(&self.time_offset_ms.to_le_bytes());
        out[16..20].copy_from_slice(&self.distance_mm.to_le_bytes());
        Ok(Self::LENGTH)
    }
    /// Check enum, reserved-bit and absent-field invariants.
    /// # Errors
    /// Rejects invalid wire semantics, without clinical interpretation.
    pub fn validate(self) -> Result<(), MotionError> {
        if self.session == 0 {
            return Err(MotionError::Invalid);
        }
        Ok(())
    }
}
#[cfg(feature = "sport")]
/// `SportGpsSample` schema body, excluding the common history envelope/CRC.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SportGpsSample {
    /// Encoded contract field `session`; units follow the versioned motion schema.
    pub session: u64,
    /// Encoded contract field `index`; units follow the versioned motion schema.
    pub index: u32,
    /// Encoded contract field `time_offset_ms`; units follow the versioned motion schema.
    pub time_offset_ms: u32,
    /// Encoded contract field `lat_e7`; units follow the versioned motion schema.
    pub lat_e7: i32,
    /// Encoded contract field `lon_e7`; units follow the versioned motion schema.
    pub lon_e7: i32,
    /// Encoded contract field `valid`; units follow the versioned motion schema.
    pub valid: u8,
    /// Encoded contract field `point_kind`; units follow the versioned motion schema.
    pub point_kind: u8,
    /// Encoded contract field `steps`; units follow the versioned motion schema.
    pub steps: u32,
    /// Encoded contract field `valid_mask`; units follow the versioned motion schema.
    pub valid_mask: u8,
}
#[cfg(feature = "sport")]
impl core::fmt::Debug for SportGpsSample {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SportGpsSample")
            .field("encoded_length", &Self::LENGTH)
            .finish_non_exhaustive()
    }
}
#[cfg(feature = "sport")]
impl SportGpsSample {
    /// Exact body length.
    pub const LENGTH: usize = 31;
    /// Decode a complete typed record.
    /// # Errors
    /// Rejects length, reserved fields, impossible enum/mask combinations and noncanonical absent values.
    pub fn decode(bytes: &[u8]) -> Result<Self, MotionError> {
        if bytes.len() != Self::LENGTH {
            return Err(MotionError::Invalid);
        }
        let value = Self {
            session: u64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]),
            index: u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            time_offset_ms: u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
            lat_e7: i32::from_le_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]),
            lon_e7: i32::from_le_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]),
            valid: bytes[24],
            point_kind: bytes[25],
            steps: u32::from_le_bytes([bytes[26], bytes[27], bytes[28], bytes[29]]),
            valid_mask: bytes[30],
        };
        value.validate()?;
        Ok(value)
    }
    /// Encode a validated record, without asserting a real algorithm produced it.
    /// # Errors
    /// Rejects invalid fields or insufficient output capacity.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, MotionError> {
        self.validate()?;
        if out.len() < Self::LENGTH {
            return Err(MotionError::Capacity);
        }
        out[0..8].copy_from_slice(&self.session.to_le_bytes());
        out[8..12].copy_from_slice(&self.index.to_le_bytes());
        out[12..16].copy_from_slice(&self.time_offset_ms.to_le_bytes());
        out[16..20].copy_from_slice(&self.lat_e7.to_le_bytes());
        out[20..24].copy_from_slice(&self.lon_e7.to_le_bytes());
        out[24] = self.valid;
        out[25] = self.point_kind;
        out[26..30].copy_from_slice(&self.steps.to_le_bytes());
        out[30] = self.valid_mask;
        Ok(Self::LENGTH)
    }
    /// Check enum, reserved-bit and absent-field invariants.
    /// # Errors
    /// Rejects invalid wire semantics, without clinical interpretation.
    pub fn validate(self) -> Result<(), MotionError> {
        if self.session == 0
            || !(1..=5).contains(&self.point_kind)
            || self.valid_mask & !3 != 0
            || self.valid != (self.valid_mask & 1)
        {
            return Err(MotionError::Invalid);
        }
        if self.valid == 1
            && (!(-900_000_000..=900_000_000).contains(&self.lat_e7)
                || !(-1_800_000_000..=1_800_000_000).contains(&self.lon_e7))
        {
            return Err(MotionError::Invalid);
        }
        canonical(
            u16::from(self.valid_mask),
            &[self.lat_e7 != 0 || self.lon_e7 != 0, self.steps != 0],
        )?;
        Ok(())
    }
}
#[cfg(any(feature = "sport", feature = "activity", feature = "wear"))]
fn canonical(mask: u16, nonzero: &[bool]) -> Result<(), MotionError> {
    for (i, value) in nonzero.iter().enumerate() {
        if mask & (1 << i) == 0 && *value {
            return Err(MotionError::Invalid);
        }
    }
    Ok(())
}
