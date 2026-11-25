//! Storage-engine record integrity. No chip addresses, erase pages or commit-marker bytes are specified.
use device_protocol::Crc32c;
/// Commit state established by a trusted record engine, not by the peer's payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitState {
    /// Preparation is not visible as valid history.
    Pending,
    /// The storage engine's atomic commit point completed.
    Committed,
}
/// Failures kept distinct from transmission CRC failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordIntegrity {
    /// Storage commit has not completed.
    Uncommitted,
    /// Record structure/identity is invalid.
    Invalid,
    /// `C_store` mismatches; map to `INTEGRITY_FAILURE/STORED_CRC_MISMATCH`.
    Mismatch,
}
/// Logical stored content with format1 WREC domain separation.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct StoredRecord<'a> {
    /// History domain identifier.
    pub domain: u8,
    /// Nonzero persistent dataset epoch.
    pub epoch: u32,
    /// Nonzero persistent domain sequence.
    pub record_id: u64,
    /// Nonzero record type.
    pub record_type: u16,
    /// Nonzero body schema revision.
    pub schema: u16,
    /// UTC milliseconds; zero if timestamp-valid bit0 is clear.
    pub utc_ms: i64,
    /// Bit0 timestamp valid; remaining bits reserved.
    pub flags: u16,
    /// Complete business body, excluding all CRCs and physical storage padding.
    pub payload: &'a [u8],
}
impl core::fmt::Debug for StoredRecord<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("StoredRecord")
            .field("payload_length", &self.payload.len())
            .finish_non_exhaustive()
    }
}
impl StoredRecord<'_> {
    /// Compute `C_store` over WREC, format1 and canonical logical metadata/content.
    /// Commit marker, erase padding, index, wire snapshot/stream and CRC itself are excluded.
    /// # Errors
    /// Rejects malformed identity, timestamp flags or unrepresentable body length.
    pub fn crc(&self) -> Result<u32, RecordIntegrity> {
        if self.epoch == 0
            || self.record_id == 0
            || self.record_type == 0
            || self.schema == 0
            || self.flags & !1 != 0
            || (self.flags & 1 == 0 && self.utc_ms != 0)
        {
            return Err(RecordIntegrity::Invalid);
        }
        let len = u16::try_from(self.payload.len()).map_err(|_| RecordIntegrity::Invalid)?;
        let mut c = Crc32c::new();
        c.update(b"WREC");
        c.update(&1u16.to_le_bytes());
        c.update(&[self.domain]);
        c.update(&self.epoch.to_le_bytes());
        c.update(&self.record_id.to_le_bytes());
        c.update(&self.record_type.to_le_bytes());
        c.update(&self.schema.to_le_bytes());
        c.update(&self.utc_ms.to_le_bytes());
        c.update(&self.flags.to_le_bytes());
        c.update(&len.to_le_bytes());
        c.update(self.payload);
        Ok(c.finish())
    }
    /// Validate trusted commit state and `C_store` before exposing a record to history export.
    /// A separate wire `C_event` must be computed after snapshot/stream encoding.
    /// # Errors
    /// Returns uncommitted, malformed content or storage-integrity mismatch without fabricated zero values.
    pub fn verify(&self, state: CommitState, stored_crc: u32) -> Result<(), RecordIntegrity> {
        if state != CommitState::Committed {
            return Err(RecordIntegrity::Uncommitted);
        }
        if self.crc()? != stored_crc {
            return Err(RecordIntegrity::Mismatch);
        }
        Ok(())
    }
}
