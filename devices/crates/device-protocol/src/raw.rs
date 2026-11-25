//! Development-only raw BATCH codec, including its domain-separated event CRC.
use crate::{
    ProtocolError,
    bytes::{Reader, Writer},
    event_crc,
};
/// Complete module70 opcode80 logical body. Format/stride comes from a trusted selected source config.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RawBatch<'a> {
    /// Nonzero connection-local capture identity.
    pub capture_id: u64,
    /// Nonzero connection-nonreused DATA stream.
    pub stream_id: u16,
    /// Capture-local batch sequence, never wrapped by its producer.
    pub batch_seq: u32,
    /// Produced-sample index, including lost samples.
    pub first_sample: u32,
    /// Actual first sample monotonic timestamp, microseconds.
    pub t0_mono_us: u64,
    /// Actual fixed interval in nanoseconds.
    pub period_ns: u32,
    /// Fixed nonzero source-configuration revision.
    pub config_rev: u32,
    /// Cumulative true device drops.
    pub dropped_total: u32,
    /// Complete multi-channel sample count.
    pub sample_count: u16,
    /// bit0 estimated timing,1 sensor overflow,2 gap; remaining bits zero.
    pub quality_flags: u16,
    /// Encoded complete samples, excluded from Debug output.
    pub samples: &'a [u8],
}
impl core::fmt::Debug for RawBatch<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RawBatch")
            .field("sample_count", &self.sample_count)
            .field("samples_length", &self.samples.len())
            .finish_non_exhaustive()
    }
}
impl<'a> RawBatch<'a> {
    /// 42 bytes of metadata plus a 4-byte event CRC, excluding samples.
    pub const OVERHEAD: usize = 46;
    /// Verify event CRC before exposing any bytes, then decode the complete selected source format.
    /// # Errors
    /// Rejects invalid identity/count/stride/timing, capacity, extra bytes or integrity mismatch.
    pub fn decode(bytes: &'a [u8], data_max: usize, stride: usize) -> Result<Self, ProtocolError> {
        if !(128..=512).contains(&data_max)
            || bytes.len() < Self::OVERHEAD
            || bytes.len() > data_max
        {
            return Err(ProtocolError::InvalidLength);
        }
        let (body, crc) = bytes.split_at(bytes.len() - 4);
        let expected = u32::from_le_bytes([crc[0], crc[1], crc[2], crc[3]]);
        if event_crc(0x70, 0x80, body)? != expected {
            return Err(ProtocolError::Integrity);
        }
        let mut r = Reader::new(body);
        let capture_id = read64(&mut r)?;
        let stream_id = r.u16()?;
        let batch_seq = r.u32()?;
        let first_sample = r.u32()?;
        let t0_mono_us = read64(&mut r)?;
        let period_ns = r.u32()?;
        let config_rev = r.u32()?;
        let dropped_total = r.u32()?;
        let sample_count = r.u16()?;
        let quality_flags = r.u16()?;
        let size = usize::from(sample_count)
            .checked_mul(stride)
            .ok_or(ProtocolError::InvalidLength)?;
        let samples = r.bytes(size)?;
        r.finish()?;
        let value = Self {
            capture_id,
            stream_id,
            batch_seq,
            first_sample,
            t0_mono_us,
            period_ns,
            config_rev,
            dropped_total,
            sample_count,
            quality_flags,
            samples,
        };
        value.validate(data_max, stride)?;
        Ok(value)
    }
    /// Encode complete metadata/samples and append `C_event`; ordinary transport fragments add no CRC.
    /// # Errors
    /// Invalid actual source metadata/sample stride, insufficient capacity or negotiated size excess.
    pub fn encode(
        &self,
        out: &mut [u8],
        data_max: usize,
        stride: usize,
    ) -> Result<usize, ProtocolError> {
        self.validate(data_max, stride)?;
        let length = Self::OVERHEAD + self.samples.len();
        if out.len() < length {
            return Err(ProtocolError::Capacity);
        }
        let mut w = Writer::new(&mut out[..length - 4]);
        w.bytes(&self.capture_id.to_le_bytes())?;
        w.u16(self.stream_id)?;
        w.u32(self.batch_seq)?;
        w.u32(self.first_sample)?;
        w.bytes(&self.t0_mono_us.to_le_bytes())?;
        w.u32(self.period_ns)?;
        w.u32(self.config_rev)?;
        w.u32(self.dropped_total)?;
        w.u16(self.sample_count)?;
        w.u16(self.quality_flags)?;
        w.bytes(self.samples)?;
        let crc = event_crc(0x70, 0x80, &out[..length - 4])?;
        out[length - 4..length].copy_from_slice(&crc.to_le_bytes());
        Ok(length)
    }
    fn validate(&self, data_max: usize, stride: usize) -> Result<(), ProtocolError> {
        if !(128..=512).contains(&data_max) || stride == 0 || stride > data_max - Self::OVERHEAD {
            return Err(ProtocolError::InvalidLength);
        }
        if self.capture_id == 0
            || self.stream_id == 0
            || self.config_rev == 0
            || self.sample_count == 0
            || self.period_ns == 0
        {
            return Err(ProtocolError::InvalidId);
        }
        if self.quality_flags & !7 != 0 {
            return Err(ProtocolError::Reserved);
        }
        if self
            .first_sample
            .checked_add(u32::from(self.sample_count) - 1)
            .is_none()
        {
            return Err(ProtocolError::InvalidLength);
        }
        let expected = usize::from(self.sample_count)
            .checked_mul(stride)
            .ok_or(ProtocolError::InvalidLength)?;
        if expected != self.samples.len() || expected > data_max - Self::OVERHEAD {
            return Err(ProtocolError::InvalidLength);
        }
        Ok(())
    }
}
fn read64(r: &mut Reader<'_>) -> Result<u64, ProtocolError> {
    let b = r.bytes(8)?;
    Ok(u64::from_le_bytes([
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
    ]))
}
