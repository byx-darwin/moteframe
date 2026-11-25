//! Raw service integration with the same connection-owned lane used by history.
use device_maintenance::{
    Error,
    raw::{Batch, DataLane},
};
use device_protocol::{DataKind, DataLease, LaneError, ProtocolError, RawBatch, SharedDataLane};
/// Raw adapter retains only a lease; all identifiers/exclusivity come from the shared owner.
#[derive(Debug)]
pub struct RawLaneAdapter<'a> {
    owner: &'a SharedDataLane,
    lease: Option<DataLease>,
    closed: bool,
}
impl<'a> RawLaneAdapter<'a> {
    /// Borrow the one application executor's shared owner; this does not acquire or allocate IDs.
    #[must_use]
    pub const fn new(owner: &'a SharedDataLane) -> Self {
        Self {
            owner,
            lease: None,
            closed: false,
        }
    }
    /// End an actual application/physical connection, including revoked CCCDs.
    /// Never use this to bypass normal STOP drain. Every producer must discard old queued work.
    /// # Errors
    /// Reentrant owner access returns Busy; it never silently permits reuse or falsely claims release.
    pub fn end_session(&mut self) -> Result<(), Error> {
        self.closed = true;
        self.owner
            .with_mut(|o| {
                o.disconnect();
                Ok(())
            })
            .map_err(map_lane)?;
        self.lease = None;
        Ok(())
    }
    fn lease(&self, stream: u16) -> Result<DataLease, Error> {
        if self.closed {
            return Err(Error::Closed);
        }
        self.lease
            .filter(|lease| lease.stream_id() == stream && lease.kind() == DataKind::Raw)
            .ok_or(Error::Identity)
    }
}
impl DataLane for RawLaneAdapter<'_> {
    fn available(&self) -> bool {
        !self.closed && self.lease.is_none() && self.owner.available().unwrap_or(false)
    }
    fn acquire(&mut self) -> Result<u16, Error> {
        if self.closed {
            return Err(Error::Closed);
        }
        if self.lease.is_some() {
            return Err(Error::Busy);
        }
        let lease = self
            .owner
            .with_mut(|o| o.acquire(DataKind::Raw))
            .map_err(map_lane)?;
        self.lease = Some(lease);
        Ok(lease.stream_id())
    }
    fn owns(&self, stream: u16) -> bool {
        let Ok(lease) = self.lease(stream) else {
            return false;
        };
        self.owner.with_mut(|o| Ok(o.owns(lease))).unwrap_or(false)
    }
    fn reserve_message(&mut self, stream: u16) -> Result<u16, Error> {
        let lease = self.lease(stream)?;
        self.owner
            .with_mut(|o| o.begin_message(lease))
            .map_err(map_lane)
    }
    fn finish_message(&mut self, stream: u16, message: u16) -> Result<(), Error> {
        let lease = self.lease(stream)?;
        self.owner
            .with_mut(|o| o.finish_message(lease, message))
            .map_err(map_lane)
    }
    fn release(&mut self, stream: u16) -> Result<(), Error> {
        let lease = self.lease(stream)?;
        self.owner
            .with_mut(|o| o.release(lease))
            .map_err(map_lane)?;
        self.lease = None;
        Ok(())
    }
}
const fn map_lane(error: LaneError) -> Error {
    match error {
        LaneError::Busy => Error::Busy,
        LaneError::Identity => Error::Identity,
        LaneError::Cancelled | LaneError::InvalidState => Error::InvalidState,
        LaneError::Exhausted => Error::Capacity,
        LaneError::Closed => Error::Closed,
    }
}
/// Encode an actual retained `RawEngine` batch with the one shared protocol codec.
/// The callback finishing the `RawEngine` batch must wait until every encoded fragment was admitted.
/// # Errors
/// Invalid source descriptor/stride, CRC encoding capacity or negotiated size rejection. No truncation.
pub fn encode_raw_batch(
    batch: &Batch<'_>,
    out: &mut [u8],
    data_max: usize,
    stride: usize,
) -> Result<usize, ProtocolError> {
    let wire = RawBatch {
        capture_id: batch.capture_id,
        stream_id: batch.stream_id,
        batch_seq: batch.batch_seq,
        first_sample: batch.first_sample,
        t0_mono_us: batch.t0_mono_us,
        period_ns: batch.period_ns,
        config_rev: batch.config_rev,
        dropped_total: batch.dropped_total,
        sample_count: batch.sample_count,
        quality_flags: batch.quality_flags,
        samples: batch.samples,
    };
    wire.encode(out, data_max, stride)
}
