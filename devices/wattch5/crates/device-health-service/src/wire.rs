use crate::Error;
/// Validated five-zone summary. Counts are actual samples, never seconds.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ZoneSummary {
    pub(crate) thresholds: [u16; 5],
    pub(crate) counts: [u32; 5],
}
impl core::fmt::Debug for ZoneSummary {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("ZoneSummary { encoded_length: 31 }")
    }
}
impl ZoneSummary {
    /// # Errors
    /// Rejects zero or nonincreasing thresholds.
    pub fn new(thresholds: [u16; 5], counts: [u32; 5]) -> Result<Self, Error> {
        if thresholds[0] == 0 || thresholds.windows(2).any(|w| w[0] >= w[1]) {
            return Err(Error::Invalid);
        }
        Ok(Self { thresholds, counts })
    }
    /// # Errors
    /// Rejects the wrong exact length, validity mask or threshold table.
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() != 31 || bytes[0] != 31 {
            return Err(Error::Invalid);
        }
        let mut t = [0; 5];
        let mut c = [0; 5];
        for i in 0..5 {
            let p = 1 + i * 6;
            t[i] = u16::from_le_bytes([bytes[p], bytes[p + 1]]);
            c[i] = u32::from_le_bytes([bytes[p + 2], bytes[p + 3], bytes[p + 4], bytes[p + 5]]);
        }
        Self::new(t, c)
    }
    /// # Errors
    /// Returns Capacity before writing when caller storage is too short.
    pub fn encode(self, out: &mut [u8]) -> Result<usize, Error> {
        if out.len() < 31 {
            return Err(Error::Capacity);
        }
        out[0] = 31;
        for i in 0..5 {
            let p = 1 + i * 6;
            out[p..p + 2].copy_from_slice(&self.thresholds[i].to_le_bytes());
            out[p + 2..p + 6].copy_from_slice(&self.counts[i].to_le_bytes());
        }
        Ok(31)
    }
}
/// Opaque admission receipt for an immutable prepared event. Only final real FIFO admission may
/// confirm it. It is connection-bound and cannot be constructed from peer bytes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct EventToken {
    pub(crate) connection: u64,
    pub(crate) module: u8,
    pub(crate) opcode: u8,
    pub(crate) job: u32,
    pub(crate) sample: u64,
}
impl EventToken {
    #[must_use]
    pub const fn module(self) -> u8 {
        self.module
    }
    #[must_use]
    pub const fn opcode(self) -> u8 {
        self.opcode
    }
}

impl core::fmt::Debug for EventToken {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("EventToken")
            .field("module", &self.module)
            .field("opcode", &self.opcode)
            .finish_non_exhaustive()
    }
}
/// Frozen module01 state/reason body. These states are independent of health job states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModuleRunStatus {
    pub state: u8,
    pub reason: u16,
}
impl ModuleRunStatus {
    /// # Errors
    /// Rejects states outside unavailable0/ready1/acquiring2/fault3 and incorrect exact length.
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() != 3 {
            return Err(Error::Invalid);
        }
        let value = Self {
            state: bytes[0],
            reason: u16::from_le_bytes([bytes[1], bytes[2]]),
        };
        if value.state > 3 {
            return Err(Error::Invalid);
        }
        Ok(value)
    }
    /// # Errors
    /// Rejects invalid states or short storage before writing.
    pub fn encode(self, out: &mut [u8]) -> Result<usize, Error> {
        if self.state > 3 {
            return Err(Error::Invalid);
        }
        if out.len() < 3 {
            return Err(Error::Capacity);
        }
        out[0] = self.state;
        out[1..3].copy_from_slice(&self.reason.to_le_bytes());
        Ok(3)
    }
}
/// Actual cuff pressure and cuff battery, not watch battery or a PPG estimate.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CuffPressure {
    pub job: u32,
    pub pressure_centi_mmhg: u32,
    pub battery: Option<u8>,
}
impl core::fmt::Debug for CuffPressure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("CuffPressure { encoded_length: 9 }")
    }
}
impl CuffPressure {
    /// # Errors
    /// Rejects zero job, battery outside0..100/255, or incorrect exact length.
    pub fn decode(b: &[u8]) -> Result<Self, Error> {
        if b.len() != 9 {
            return Err(Error::Invalid);
        }
        let v = Self {
            job: u32::from_le_bytes([b[0], b[1], b[2], b[3]]),
            pressure_centi_mmhg: u32::from_le_bytes([b[4], b[5], b[6], b[7]]),
            battery: if b[8] == 255 { None } else { Some(b[8]) },
        };
        let mut check = [0; 9];
        v.encode(&mut check)?;
        Ok(v)
    }
    /// # Errors
    /// Rejects invalid identity/battery and insufficient capacity before writing.
    pub fn encode(self, b: &mut [u8]) -> Result<usize, Error> {
        if self.job == 0 || self.battery.is_some_and(|v| v > 100) {
            return Err(Error::Invalid);
        }
        if b.len() < 9 {
            return Err(Error::Capacity);
        }
        b[..4].copy_from_slice(&self.job.to_le_bytes());
        b[4..8].copy_from_slice(&self.pressure_centi_mmhg.to_le_bytes());
        b[8] = self.battery.unwrap_or(255);
        Ok(9)
    }
}
