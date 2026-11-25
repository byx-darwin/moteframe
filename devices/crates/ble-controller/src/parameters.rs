//! Bluetooth units and owned legacy advertising configuration.

/// A value violates the Bluetooth legacy parameter bounds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidParameters;

/// Connection timing in 1.25 ms interval and 10 ms supervision units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConnectionParameters {
    interval: u16,
    latency: u16,
    timeout: u16,
}
impl ConnectionParameters {
    /// Validates bounds and the strict supervision timeout inequality.
    /// # Errors
    /// Rejects out-of-range timing or an insufficient supervision timeout.
    pub fn new(interval: u16, latency: u16, timeout: u16) -> Result<Self, InvalidParameters> {
        if !(6..=3200).contains(&interval)
            || latency > 499
            || !(10..=3200).contains(&timeout)
            || u32::from(timeout) * 4 <= u32::from(interval) * (1 + u32::from(latency))
        {
            return Err(InvalidParameters);
        }
        Ok(Self {
            interval,
            latency,
            timeout,
        })
    }
    /// Connection interval, in 1.25 ms units.
    #[must_use]
    pub const fn interval(self) -> u16 {
        self.interval
    }
    /// Number of connection events a peripheral may skip.
    #[must_use]
    pub const fn latency(self) -> u16 {
        self.latency
    }
    /// Supervision timeout, in 10 ms units.
    #[must_use]
    pub const fn timeout(self) -> u16 {
        self.timeout
    }
}

/// Validated connectable advertising interval range and primary channel mask.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdvertisingParameters {
    min: u16,
    max: u16,
    channels: u8,
}
impl AdvertisingParameters {
    /// Validates 0.625 ms interval units and channel bits 37, 38 and 39.
    /// # Errors
    /// Rejects intervals outside 32..=16384, reversed ranges or invalid masks.
    pub fn new(min: u16, max: u16, channels: u8) -> Result<Self, InvalidParameters> {
        if min < 32 || max > 16384 || min > max || channels == 0 || channels & !7 != 0 {
            return Err(InvalidParameters);
        }
        Ok(Self { min, max, channels })
    }
    /// Minimum interval, in 0.625 ms units.
    #[must_use]
    pub const fn min_interval(self) -> u16 {
        self.min
    }
    /// Maximum interval, in 0.625 ms units.
    #[must_use]
    pub const fn max_interval(self) -> u16 {
        self.max
    }
    /// Primary advertising channel mask.
    #[must_use]
    pub const fn channels(self) -> u8 {
        self.channels
    }
}

/// Static random address in Bluetooth little-endian byte order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RandomAddress([u8; 6]);
impl RandomAddress {
    /// Checks the static address tag and nonconstant random part.
    /// # Errors
    /// Rejects other address kinds and all-zero/all-one 46-bit random parts.
    pub fn new(bytes: [u8; 6]) -> Result<Self, InvalidParameters> {
        let high = bytes[5] & 0x3f;
        if bytes[5] & 0xc0 != 0xc0
            || (high == 0 && bytes[..5].iter().all(|&b| b == 0))
            || (high == 0x3f && bytes[..5].iter().all(|&b| b == 0xff))
        {
            return Err(InvalidParameters);
        }
        Ok(Self(bytes))
    }
    /// Six address bytes in Bluetooth byte order.
    #[must_use]
    pub const fn bytes(self) -> [u8; 6] {
        self.0
    }
}

/// Owned advertising or scan response data; Debug omits the contents.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct AdvertisingData {
    bytes: [u8; 31],
    len: u8,
}
impl core::fmt::Debug for AdvertisingData {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("AdvertisingData")
            .field("len", &self.len)
            .finish_non_exhaustive()
    }
}
impl AdvertisingData {
    /// Copies at most 31 bytes, including an empty payload.
    /// # Errors
    /// Rejects data exceeding the legacy advertising limit.
    pub fn new(data: &[u8]) -> Result<Self, InvalidParameters> {
        let len = u8::try_from(data.len()).map_err(|_| InvalidParameters)?;
        if len > 31 {
            return Err(InvalidParameters);
        }
        let mut bytes = [0; 31];
        bytes[..data.len()].copy_from_slice(data);
        Ok(Self { bytes, len })
    }
    /// Data without trailing storage bytes.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.bytes[..usize::from(self.len)]
    }
}
