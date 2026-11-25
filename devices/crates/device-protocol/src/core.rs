//! Core payload codecs; these types neither authenticate nor register modules.
use crate::ProtocolError;
/// HELLO request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HelloRequest {
    /// Wire field `min_major`.
    pub min_major: u8,
    /// Wire field `max_major`.
    pub max_major: u8,
    /// Wire field `rx_control_max`.
    pub rx_control_max: u16,
    /// Wire field `rx_data_max`.
    pub rx_data_max: u16,
    /// Wire field `flags`.
    pub flags: u16,
}
impl HelloRequest {
    /// Exact encoded length.
    pub const LENGTH: usize = 8;
    /// Decode an exact fixed-layout payload.
    /// # Errors
    /// Rejects extra/missing bytes and invalid contract fields.
    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        if bytes.len() != Self::LENGTH {
            return Err(ProtocolError::InvalidLength);
        }
        let value = Self {
            min_major: bytes[0],
            max_major: bytes[1],
            rx_control_max: u16::from_le_bytes([bytes[2], bytes[3]]),
            rx_data_max: u16::from_le_bytes([bytes[4], bytes[5]]),
            flags: u16::from_le_bytes([bytes[6], bytes[7]]),
        };
        value.validate()?;
        Ok(value)
    }
    /// Encode validated fields into caller storage.
    /// # Errors
    /// Rejects invalid fields or insufficient output capacity.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, ProtocolError> {
        self.validate()?;
        if out.len() < Self::LENGTH {
            return Err(ProtocolError::Capacity);
        }
        out[0] = self.min_major;
        out[1] = self.max_major;
        out[2..4].copy_from_slice(&self.rx_control_max.to_le_bytes());
        out[4..6].copy_from_slice(&self.rx_data_max.to_le_bytes());
        out[6..8].copy_from_slice(&self.flags.to_le_bytes());
        Ok(Self::LENGTH)
    }
    fn validate(self) -> Result<(), ProtocolError> {
        if self.min_major > 1
            || self.max_major < 1
            || self.min_major > self.max_major
            || self.flags != 3
        {
            return Err(ProtocolError::InvalidVersion);
        }
        if !(128..=512).contains(&self.rx_control_max)
            || (self.rx_data_max != 0 && !(128..=512).contains(&self.rx_data_max))
        {
            return Err(ProtocolError::InvalidLength);
        }
        Ok(())
    }
}
/// HELLO success data, excluding common status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HelloResponse {
    /// Wire field `major`.
    pub major: u8,
    /// Wire field `minor`.
    pub minor: u8,
    /// Wire field `session`.
    pub session: u32,
    /// Wire field `control_max`.
    pub control_max: u16,
    /// Wire field `data_max`.
    pub data_max: u16,
    /// Wire field `inflight`.
    pub inflight: u8,
    /// Wire field `profile`.
    pub profile: u8,
    /// Wire field `access`.
    pub access: u8,
    /// Wire field `reserved`.
    pub reserved: u8,
    /// Wire field `capability_rev`.
    pub capability_rev: u32,
}
impl HelloResponse {
    /// Exact encoded length.
    pub const LENGTH: usize = 18;
    /// Decode an exact fixed-layout payload.
    /// # Errors
    /// Rejects extra/missing bytes and invalid contract fields.
    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        if bytes.len() != Self::LENGTH {
            return Err(ProtocolError::InvalidLength);
        }
        let value = Self {
            major: bytes[0],
            minor: bytes[1],
            session: u32::from_le_bytes([bytes[2], bytes[3], bytes[4], bytes[5]]),
            control_max: u16::from_le_bytes([bytes[6], bytes[7]]),
            data_max: u16::from_le_bytes([bytes[8], bytes[9]]),
            inflight: bytes[10],
            profile: bytes[11],
            access: bytes[12],
            reserved: bytes[13],
            capability_rev: u32::from_le_bytes([bytes[14], bytes[15], bytes[16], bytes[17]]),
        };
        value.validate()?;
        Ok(value)
    }
    /// Encode validated fields into caller storage.
    /// # Errors
    /// Rejects invalid fields or insufficient output capacity.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, ProtocolError> {
        self.validate()?;
        if out.len() < Self::LENGTH {
            return Err(ProtocolError::Capacity);
        }
        out[0] = self.major;
        out[1] = self.minor;
        out[2..6].copy_from_slice(&self.session.to_le_bytes());
        out[6..8].copy_from_slice(&self.control_max.to_le_bytes());
        out[8..10].copy_from_slice(&self.data_max.to_le_bytes());
        out[10] = self.inflight;
        out[11] = self.profile;
        out[12] = self.access;
        out[13] = self.reserved;
        out[14..18].copy_from_slice(&self.capability_rev.to_le_bytes());
        Ok(Self::LENGTH)
    }
    fn validate(self) -> Result<(), ProtocolError> {
        if self.major != 1 || self.minor != 2 {
            return Err(ProtocolError::InvalidVersion);
        }
        if self.session == 0 || self.capability_rev == 0 {
            return Err(ProtocolError::InvalidId);
        }
        if self.reserved != 0 {
            return Err(ProtocolError::Reserved);
        }
        if self.inflight != 1 || self.profile > 2 || self.access > 2 {
            return Err(ProtocolError::InvalidState);
        }
        if !(128..=512).contains(&self.control_max)
            || (self.data_max != 0 && !(128..=512).contains(&self.data_max))
        {
            return Err(ProtocolError::InvalidLength);
        }
        Ok(())
    }
}
/// One MODULES capability entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModuleEntry {
    /// Wire field `module`.
    pub module: u8,
    /// Wire field `major`.
    pub major: u8,
    /// Wire field `minor`.
    pub minor: u8,
    /// Wire field `availability`.
    pub availability: u8,
    /// Wire field `features`.
    pub features: u32,
    /// Wire field `input_max`.
    pub input_max: u16,
    /// Wire field `output_max`.
    pub output_max: u16,
}
impl ModuleEntry {
    /// Exact encoded length.
    pub const LENGTH: usize = 12;
    /// Decode an exact fixed-layout payload.
    /// # Errors
    /// Rejects extra/missing bytes and invalid contract fields.
    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        if bytes.len() != Self::LENGTH {
            return Err(ProtocolError::InvalidLength);
        }
        let value = Self {
            module: bytes[0],
            major: bytes[1],
            minor: bytes[2],
            availability: bytes[3],
            features: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
            input_max: u16::from_le_bytes([bytes[8], bytes[9]]),
            output_max: u16::from_le_bytes([bytes[10], bytes[11]]),
        };
        value.validate()?;
        Ok(value)
    }
    /// Encode validated fields into caller storage.
    /// # Errors
    /// Rejects invalid fields or insufficient output capacity.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, ProtocolError> {
        self.validate()?;
        if out.len() < Self::LENGTH {
            return Err(ProtocolError::Capacity);
        }
        out[0] = self.module;
        out[1] = self.major;
        out[2] = self.minor;
        out[3] = self.availability;
        out[4..8].copy_from_slice(&self.features.to_le_bytes());
        out[8..10].copy_from_slice(&self.input_max.to_le_bytes());
        out[10..12].copy_from_slice(&self.output_max.to_le_bytes());
        Ok(Self::LENGTH)
    }
    fn validate(self) -> Result<(), ProtocolError> {
        if self.features != 0 {
            return Err(ProtocolError::Reserved);
        }
        if self.availability > 2 {
            return Err(ProtocolError::InvalidState);
        }
        if self.input_max > 512 || self.output_max > 512 {
            return Err(ProtocolError::InvalidLength);
        }
        Ok(())
    }
}
/// Fixed wire 1.2 limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtocolLimits {
    /// Wire field `rx_frame_max`.
    pub rx_frame_max: u16,
    /// Wire field `tx_frame_max`.
    pub tx_frame_max: u16,
    /// Wire field `control_max`.
    pub control_max: u16,
    /// Wire field `data_max`.
    pub data_max: u16,
    /// Wire field `idle_ms`.
    pub idle_ms: u16,
    /// Wire field `fragment_ms`.
    pub fragment_ms: u16,
    /// Wire field `response_ms`.
    pub response_ms: u16,
}
impl ProtocolLimits {
    /// Exact encoded length.
    pub const LENGTH: usize = 14;
    /// Decode an exact fixed-layout payload.
    /// # Errors
    /// Rejects extra/missing bytes and invalid contract fields.
    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        if bytes.len() != Self::LENGTH {
            return Err(ProtocolError::InvalidLength);
        }
        let value = Self {
            rx_frame_max: u16::from_le_bytes([bytes[0], bytes[1]]),
            tx_frame_max: u16::from_le_bytes([bytes[2], bytes[3]]),
            control_max: u16::from_le_bytes([bytes[4], bytes[5]]),
            data_max: u16::from_le_bytes([bytes[6], bytes[7]]),
            idle_ms: u16::from_le_bytes([bytes[8], bytes[9]]),
            fragment_ms: u16::from_le_bytes([bytes[10], bytes[11]]),
            response_ms: u16::from_le_bytes([bytes[12], bytes[13]]),
        };
        value.validate()?;
        Ok(value)
    }
    /// Encode validated fields into caller storage.
    /// # Errors
    /// Rejects invalid fields or insufficient output capacity.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, ProtocolError> {
        self.validate()?;
        if out.len() < Self::LENGTH {
            return Err(ProtocolError::Capacity);
        }
        out[0..2].copy_from_slice(&self.rx_frame_max.to_le_bytes());
        out[2..4].copy_from_slice(&self.tx_frame_max.to_le_bytes());
        out[4..6].copy_from_slice(&self.control_max.to_le_bytes());
        out[6..8].copy_from_slice(&self.data_max.to_le_bytes());
        out[8..10].copy_from_slice(&self.idle_ms.to_le_bytes());
        out[10..12].copy_from_slice(&self.fragment_ms.to_le_bytes());
        out[12..14].copy_from_slice(&self.response_ms.to_le_bytes());
        Ok(Self::LENGTH)
    }
    fn validate(self) -> Result<(), ProtocolError> {
        if !(20..=522).contains(&self.rx_frame_max)
            || !(20..=522).contains(&self.tx_frame_max)
            || !(128..=512).contains(&self.control_max)
            || (self.data_max != 0 && !(128..=512).contains(&self.data_max))
        {
            return Err(ProtocolError::InvalidLength);
        }
        if self.idle_ms != 2000 || self.fragment_ms != 100 || self.response_ms != 10000 {
            return Err(ProtocolError::InvalidState);
        }
        Ok(())
    }
}
