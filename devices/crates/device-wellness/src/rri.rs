//! Concrete2C01/2C02 schema1. Relative monotonic time is not a fabricated UTC timestamp.
use crate::{Error, boolean, exact};
/// RRI session summary. Stable identity is supplied by the product's persistent allocator.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RriSummary {
    /// Nonzero stable64-bit measurement ID.
    pub measurement: u64,
    /// Actual monotonic start-to-end duration,not a sum of intervals.
    pub duration_ms: u32,
    /// Actual valid interval count,excluding invalid/baseline candidates.
    pub valid_count: u32,
}
redacted!(RriSummary, "RriSummary", 16);
impl RriSummary {
    /// Exact body size,not a LATEST wrapper.
    pub const LENGTH: usize = 16;
    /// Decode one exact2C01 body.
    /// # Errors
    /// Extra/missing bytes or ID0 fail.
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        exact(bytes, 16)?;
        let mut id = [0; 8];
        id.copy_from_slice(&bytes[..8]);
        let value = Self {
            measurement: u64::from_le_bytes(id),
            duration_ms: u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            valid_count: u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]),
        };
        value.validate()?;
        Ok(value)
    }
    /// Validate representation,without claiming any algorithm produced these counters.
    /// # Errors
    /// Measurement0 is invalid.
    pub const fn validate(self) -> Result<(), Error> {
        if self.measurement == 0 {
            Err(Error::InvalidField)
        } else {
            Ok(())
        }
    }
    /// Encode exact16B little-endian.
    /// # Errors
    /// Invalid identity/short output fails before writing.
    pub fn encode(self, out: &mut [u8]) -> Result<usize, Error> {
        self.validate()?;
        if out.len() < 16 {
            return Err(Error::Capacity);
        }
        out[..8].copy_from_slice(&self.measurement.to_le_bytes());
        out[8..12].copy_from_slice(&self.duration_ms.to_le_bytes());
        out[12..16].copy_from_slice(&self.valid_count.to_le_bytes());
        Ok(16)
    }
}
/// One candidate heartbeat interval;2C.E80 is exactly this25B body,not a LATEST envelope.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RriEvent {
    /// Parent measurement ID,not a truncated sport alias.
    pub measurement: u64,
    /// Candidate index,starts0; invalid candidates still consume an index.
    pub index: u32,
    /// Monotonic microseconds from measurement start to this candidate.
    pub cumulative_us: u64,
    /// Actual positive interval from previous valid heartbeat,or absent0/valid0.
    pub interval_us: Option<u32>,
}
redacted!(RriEvent, "RriEvent", 25);
impl RriEvent {
    /// Exact record/event body size.
    pub const LENGTH: usize = 25;
    /// Decode the exact relative-time event with strict validity/canonical absence.
    /// # Errors
    /// Zero identity,positive-validity contradiction,bad boolean or length fails.
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        exact(bytes, 25)?;
        let mut id = [0; 8];
        id.copy_from_slice(&bytes[..8]);
        let mut cumulative = [0; 8];
        cumulative.copy_from_slice(&bytes[12..20]);
        let interval = u32::from_le_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
        let valid = boolean(bytes[24])?;
        if !valid && interval != 0 {
            return Err(Error::InvalidField);
        }
        let result = Self {
            measurement: u64::from_le_bytes(id),
            index: u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]),
            cumulative_us: u64::from_le_bytes(cumulative),
            interval_us: if valid { Some(interval) } else { None },
        };
        result.validate()?;
        Ok(result)
    }
    /// Validate scalar fields. Session/index/delta relationships require a live tracker.
    /// # Errors
    /// ID0 or present interval0 is invalid.
    pub const fn validate(self) -> Result<(), Error> {
        if self.measurement == 0 || matches!(self.interval_us, Some(0)) {
            Err(Error::InvalidField)
        } else {
            Ok(())
        }
    }
    /// Encode exactly25B without source/UTC/status additions absent from the candidate layout.
    /// # Errors
    /// Invalid scalar fields or short output fails before writing.
    pub fn encode(self, out: &mut [u8]) -> Result<usize, Error> {
        self.validate()?;
        if out.len() < 25 {
            return Err(Error::Capacity);
        }
        out[..8].copy_from_slice(&self.measurement.to_le_bytes());
        out[8..12].copy_from_slice(&self.index.to_le_bytes());
        out[12..20].copy_from_slice(&self.cumulative_us.to_le_bytes());
        out[20..24].copy_from_slice(&self.interval_us.unwrap_or(0).to_le_bytes());
        out[24] = u8::from(self.interval_us.is_some());
        Ok(25)
    }
}
/// Actual backend knowledge,not another wire flag. A rejected candidate cannot establish a heartbeat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BeatClass {
    /// No trustworthy heartbeat at this candidate.
    Rejected,
    /// First actual trustworthy heartbeat after start/resume; no prior interval yet.
    Baseline,
    /// Actual trustworthy beat with a measured interval from the previous trustworthy beat.
    Interval,
}
/// Actual candidate plus trusted quality classification. Classification is not appended to25B wire.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RriObservation {
    /// Exact candidate event.
    pub event: RriEvent,
    /// Actual algorithm classification; never inferred from bpm or zero bytes.
    pub class: BeatClass,
}
redacted!(RriObservation, "RriObservation", 25);
#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Running,
    Paused,
    Finished,
}
/// One statically bounded measurement validator with one retained event awaiting admission.
/// It is not a sensor driver,persistent ID allocator or proof that physical sampling stopped.
pub struct RriTracker {
    measurement: u64,
    parent_sport: Option<u64>,
    started: u64,
    last_clock: u64,
    state: State,
    next_index: Option<u32>,
    anchor: Option<u64>,
    last: Option<RriObservation>,
    pending: bool,
    last_confirmed: Option<u32>,
    valid_count: u32,
    finished: Option<RriSummary>,
}
redacted!(RriTracker, "RriTracker", 0);
impl RriTracker {
    /// Create only after real backend acceptance. Parent sport remains full64-bit metadata.
    /// # Errors
    /// Measurement0 or Some(parent0) fails. Stable no-reuse measurement allocation belongs to product.
    pub fn new(
        measurement: u64,
        parent_sport: Option<u64>,
        start_mono_us: u64,
    ) -> Result<Self, Error> {
        if measurement == 0 || parent_sport == Some(0) {
            return Err(Error::InvalidField);
        }
        Ok(Self {
            measurement,
            parent_sport,
            started: start_mono_us,
            last_clock: start_mono_us,
            state: State::Running,
            next_index: Some(0),
            anchor: None,
            last: None,
            pending: false,
            last_confirmed: None,
            valid_count: 0,
            finished: None,
        })
    }
    /// Exact parent identity,never truncated into a32-bit alias.
    #[must_use]
    pub const fn parent_sport(&self) -> Option<u64> {
        self.parent_sport
    }
    /// Last actual candidate remains pending until admission is confirmed.
    #[must_use]
    pub fn pending(&self) -> Option<RriEvent> {
        if self.pending {
            self.last.map(|value| value.event)
        } else {
            None
        }
    }
    fn elapsed(&self, now: u64) -> Result<u64, Error> {
        if now < self.last_clock {
            return Err(Error::Time);
        }
        let elapsed = now.checked_sub(self.started).ok_or(Error::Time)?;
        u32::try_from(elapsed / 1000).map_err(|_| Error::Exhausted)?;
        Ok(elapsed)
    }
    /// Accept one actual immutable candidate. Exact retry returns false and never recounts.
    /// # Errors
    /// Rejects paused/finished work,future/backwards time,wrong IDs/index/delta,invalid baseline,
    /// count/index exhaustion or overwriting an unadmitted actual candidate. No value is synthesized.
    pub fn accept(&mut self, value: RriObservation, now: u64) -> Result<bool, Error> {
        if self.state != State::Running {
            return Err(Error::InvalidState);
        }
        value.event.validate()?;
        if value.event.measurement != self.measurement {
            return Err(Error::Sequence);
        }
        let elapsed = self.elapsed(now)?;
        if let Some(last) = self.last {
            if value == last {
                self.last_clock = now;
                return Ok(false);
            }
            if value.event.index == last.event.index {
                return Err(Error::Sequence);
            }
        }
        if self.pending {
            return Err(Error::Busy);
        }
        if Some(value.event.index) != self.next_index {
            return Err(if self.next_index.is_none() {
                Error::Exhausted
            } else {
                Error::Sequence
            });
        }
        if value.event.cumulative_us > elapsed
            || self
                .last
                .is_some_and(|last| value.event.cumulative_us <= last.event.cumulative_us)
        {
            return Err(Error::Time);
        }
        let mut anchor = self.anchor;
        let mut count = self.valid_count;
        match value.class {
            BeatClass::Rejected => {
                if value.event.interval_us.is_some() {
                    return Err(Error::InvalidField);
                }
            }
            BeatClass::Baseline => {
                if value.event.interval_us.is_some() || anchor.is_some() {
                    return Err(Error::InvalidField);
                }
                anchor = Some(value.event.cumulative_us);
            }
            BeatClass::Interval => {
                let previous = anchor.ok_or(Error::InvalidField)?;
                let delta = value
                    .event
                    .cumulative_us
                    .checked_sub(previous)
                    .ok_or(Error::Time)?;
                let expected = u32::try_from(delta).map_err(|_| Error::Exhausted)?;
                if value.event.interval_us != Some(expected) || expected == 0 {
                    return Err(Error::InvalidField);
                }
                count = count.checked_add(1).ok_or(Error::Exhausted)?;
                anchor = Some(value.event.cumulative_us);
            }
        }
        self.next_index = value.event.index.checked_add(1);
        self.anchor = anchor;
        self.valid_count = count;
        self.last = Some(value);
        self.pending = true;
        self.last_clock = now;
        Ok(true)
    }
    /// Confirm actual queue/store admission. This does not claim peer persistence.
    /// # Errors
    /// Unknown identity/index or no pending candidate fails; exact duplicate acknowledgement is safe.
    pub fn confirm(&mut self, measurement: u64, index: u32) -> Result<(), Error> {
        if measurement != self.measurement {
            return Err(Error::Sequence);
        }
        if self.last_confirmed == Some(index) {
            return Ok(());
        }
        if !self.pending || self.last.is_none_or(|last| last.event.index != index) {
            return Err(Error::Sequence);
        }
        self.pending = false;
        self.last_confirmed = Some(index);
        Ok(())
    }
    /// Pause only after pending data is admitted and real backend pause is confirmed by the caller.
    /// # Errors
    /// Pending data/backwards clock/finished state fails without dropping anything.
    pub fn pause(&mut self, now: u64) -> Result<(), Error> {
        if self.pending {
            return Err(Error::Busy);
        }
        if self.state == State::Finished {
            return Err(Error::InvalidState);
        }
        self.elapsed(now)?;
        self.state = State::Paused;
        self.anchor = None;
        self.last_clock = now;
        Ok(())
    }
    /// Resume without manufacturing an interval across the unsampled pause gap.
    /// # Errors
    /// Finished state/backwards clock fails; repeated resume is idempotent.
    pub fn resume(&mut self, now: u64) -> Result<(), Error> {
        if self.state == State::Finished {
            return Err(Error::InvalidState);
        }
        self.elapsed(now)?;
        if self.state == State::Paused {
            self.state = State::Running;
            self.anchor = None;
        }
        self.last_clock = now;
        Ok(())
    }
    /// Actual monotonic duration includes pauses; only real valid intervals increment the count.
    /// # Errors
    /// Clock reversal or u32 milliseconds overflow fails; no wrapping or sample sum substitutes.
    pub fn summary_at(&self, now: u64) -> Result<RriSummary, Error> {
        if now < self.last_clock {
            return Err(Error::Time);
        }
        if let Some(done) = self.finished {
            return Ok(done);
        }
        let elapsed = self.elapsed(now)?;
        Ok(RriSummary {
            measurement: self.measurement,
            duration_ms: u32::try_from(elapsed / 1000).map_err(|_| Error::Exhausted)?,
            valid_count: self.valid_count,
        })
    }
    /// Finalize only after caller-confirmed physical stop and all pending actual events are drained.
    /// # Errors
    /// Undrained event/clock overflow fails. Repeated finish preserves the original summary.
    pub fn finish(&mut self, now: u64) -> Result<RriSummary, Error> {
        if self.pending {
            return Err(Error::Busy);
        }
        let summary = self.summary_at(now)?;
        if self.finished.is_none() {
            self.finished = Some(summary);
            self.last_clock = now;
            self.state = State::Finished;
        }
        Ok(summary)
    }
}
/// Actual result origin,kept in product metadata because2C's candidate wire has no source field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RriOrigin {
    /// Actual beat detector algorithm,never60/bpm interpolation.
    DeviceAlgorithm,
    /// Actual external intervals imported with explicit product authorization.
    PhoneImport,
}
/// Actual RRI acquisition/import port. No implementation is installed by this crate.
/// Immutable `peek` data must remain retained until explicit acknowledgement. A start error must
/// leave no newly acquired resources. Persistent measurement identity and authorization are external.
pub trait RriBackend {
    /// Whether a real selected capability is available now.
    fn available(&self) -> bool;
    /// Actual origin,not automatically promoted to sensor truth.
    fn origin(&self) -> RriOrigin;
    /// Actual provider ID/version,never synthetic default strings.
    fn algorithm(&self) -> (&str, &str);
    /// Validate full64-bit parent sport against its actual current lifecycle.
    fn parent_is_valid(&self, parent: u64) -> bool;
    /// Start actual work after validated nonzero IDs.
    /// # Errors
    /// Actual unavailable/unsupported/fault; rejected start must roll back resources.
    fn start(&mut self, measurement: u64, parent: Option<u64>) -> Result<(), crate::BackendError>;
    /// Peek actual retained data,or explicit no data.
    /// # Errors
    /// Actual backend errors,never zero-filled successful events.
    fn peek(&mut self) -> Result<Option<RriObservation>, crate::BackendError>;
    /// Release a retained actual event only after admission.
    /// # Errors
    /// Failure must retain the same event for retry.
    fn acknowledge(&mut self, measurement: u64, index: u32) -> Result<(), crate::BackendError>;
    /// Stop physical work; success must mean actual stop,not merely command acceptance.
    /// # Errors
    /// Actual cleanup failure remains explicit.
    fn stop(&mut self, measurement: u64) -> Result<(), crate::BackendError>;
}
/// Start through the actual port; missing capability never yields a synthetic tracker/job.
/// # Errors
/// Bad IDs/parent,unversioned source or actual start error is explicit.
pub fn start_rri<B: RriBackend>(
    backend: &mut B,
    measurement: u64,
    parent: Option<u64>,
    now: u64,
) -> Result<RriTracker, Error> {
    let tracker = RriTracker::new(measurement, parent, now)?;
    if !backend.available() {
        return Err(Error::Unavailable);
    }
    if parent.is_some_and(|id| !backend.parent_is_valid(id)) {
        return Err(Error::InvalidField);
    }
    let (id, version) = backend.algorithm();
    if id.is_empty()
        || version.is_empty()
        || id.as_bytes().contains(&0)
        || version.as_bytes().contains(&0)
    {
        return Err(Error::InvalidField);
    }
    backend.start(measurement, parent)?;
    Ok(tracker)
}
/// Peek/validate actual data without acknowledgement. Pending retry stays byte-identical.
/// # Errors
/// Unavailable backend or lifecycle/sequence/clock violation is explicit.
pub fn poll_rri<B: RriBackend>(
    backend: &mut B,
    tracker: &mut RriTracker,
    now: u64,
) -> Result<Option<RriEvent>, Error> {
    if !backend.available() {
        return Err(Error::Unavailable);
    }
    if tracker.state != State::Running {
        return Err(Error::InvalidState);
    }
    let Some(value) = backend.peek()? else {
        return Ok(None);
    };
    tracker.accept(value, now)?;
    Ok(tracker.pending())
}
/// Confirm actual queue/store admission before releasing the backend's retained event.
/// # Errors
/// Mismatch or actual backend failure leaves the event retained for retry.
pub fn confirm_rri<B: RriBackend>(
    backend: &mut B,
    tracker: &mut RriTracker,
    measurement: u64,
    index: u32,
) -> Result<(), Error> {
    if measurement != tracker.measurement {
        return Err(Error::Sequence);
    }
    if tracker.last_confirmed == Some(index) {
        return Ok(());
    }
    if tracker.pending().is_none_or(|event| event.index != index) {
        return Err(Error::Sequence);
    }
    backend.acknowledge(measurement, index)?;
    tracker.confirm(measurement, index)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_should_stop_index_and_count_before_wrap_without_mutating_retained_data() {
        let mut t = RriTracker::new(1, None, 0).unwrap_or_else(|e| panic!("{e:?}"));
        t.next_index = Some(u32::MAX);
        let last = RriObservation {
            event: RriEvent {
                measurement: 1,
                index: u32::MAX,
                cumulative_us: 1,
                interval_us: None,
            },
            class: BeatClass::Baseline,
        };
        assert_eq!(t.accept(last, 1), Ok(true));
        t.confirm(1, u32::MAX).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(
            t.accept(
                RriObservation {
                    event: RriEvent {
                        index: 0,
                        cumulative_us: 2,
                        ..last.event
                    },
                    ..last
                },
                2
            ),
            Err(Error::Exhausted)
        );
        let mut t = RriTracker::new(1, None, 0).unwrap_or_else(|e| panic!("{e:?}"));
        t.anchor = Some(0);
        t.valid_count = u32::MAX;
        let value = RriObservation {
            event: RriEvent {
                measurement: 1,
                index: 0,
                cumulative_us: 1,
                interval_us: Some(1),
            },
            class: BeatClass::Interval,
        };
        assert_eq!(t.accept(value, 1), Err(Error::Exhausted));
        assert_eq!(t.pending(), None);
        assert_eq!(t.valid_count, u32::MAX);
    }
}
