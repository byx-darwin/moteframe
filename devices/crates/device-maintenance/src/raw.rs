//! Raw acquisition contracts and bounded engine.
use crate::{Authority, Error, Profile};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Integer channel encoding from the frozen CHANNEL dictionary.
pub enum Format {
    /// Signed16-bit channel.
    I16 = 1,
    /// Signed32-bit channel.
    I32 = 2,
    /// Unsigned16-bit channel.
    U16 = 3,
    /// Unsigned32-bit channel.
    U32 = 4,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Actual backend source descriptor; never inferred from SDK source presence.
pub struct Source {
    /// Nonzero controlled identity.
    pub id: u16,
    /// Versioned backend/source or evidence kind.
    pub kind: u8,
    /// Actual channel count, 1..16.
    pub channels: u8,
    /// Supported format bitmap bits0..3; reserved bits zero.
    pub formats: u8,
    /// Supported feature flags; reserved bits must be zero.
    pub flags: u8,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Actual backend channel format and physical scale.
pub struct Channel {
    /// Source-local channel index.
    pub index: u8,
    /// Actual integer encoding.
    pub format: Format,
    /// Frozen physical unit code; no guessed conversion.
    pub unit: u16,
    /// Physical conversion numerator.
    pub scale_num: i32,
    /// Nonzero physical conversion denominator.
    pub scale_den: u32,
    /// Frozen axis code, 0..4.
    pub axis: u8,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Raw CONFIG request; unsupported combinations must fail explicitly.
pub struct Config {
    /// Actual source identity.
    pub source: u16,
    /// Selected channels in bit-ascending interleave order.
    pub channel_mask: u32,
    /// Requested or actual frequency in millihertz.
    pub rate_mhz: u32,
    /// Zero default range; nonzero is unsupported in this version.
    pub range_code: u16,
    /// Reserved options, required zero.
    pub options: u16,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Actual configuration returned by the physical backend.
pub struct Accepted {
    /// Requested or actual frequency in millihertz.
    pub rate_mhz: u32,
    /// Actual fixed sample period in nanoseconds.
    pub period_ns: u32,
    /// Total bytes per complete multichannel sample.
    pub stride: u16,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Immutable accepted configuration bound to a nonreusing revision.
pub struct Configured {
    /// Nonzero accepted configuration revision.
    pub revision: u32,
    /// Actual backend-accepted configuration.
    pub accepted: Accepted,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// One complete real backend batch and explicit preceding hardware loss.
pub struct Polled {
    /// Real sample count or borrowed encoded sample bytes.
    pub samples: u16,
    /// Complete batch sample-byte length.
    pub bytes: usize,
    /// Real hardware samples lost before this batch.
    pub lost: u32,
    /// Actual first-sample/start monotonic microseconds.
    pub t0_mono_us: u64,
    /// Bits0 estimated timing,1 sensor overflow,2 gap; others zero.
    pub quality_flags: u16,
    /// True only for real valid samples; invalid samples count as dropped.
    pub valid: bool,
}
/// Actual acquisition backend; there is no default successful implementation.
/// The source catalogue must remain stable for the connection. `poll` must honor
/// both buffer/sample capacity and remaining produced-sample budget (including
/// loss), stop before counters overflow, and never substitute generated values.
pub trait RawPort {
    /// Return the real static source count, at most254.
    fn source_count(&self) -> usize;
    /// Return a real static source entry at the requested array index.
    fn source(&self, index: usize) -> Option<Source>;
    /// Read a real supported channel descriptor and reject invalid backend metadata.
    /// # Errors
    /// Returns explicit policy, lifecycle, identity, capacity, clock or backend failures.
    fn channel(&self, source: u16, index: u8) -> Result<Channel, Error>;
    /// Accept only supported stopped-state settings and actual backend rate/format.
    /// # Errors
    /// Returns explicit policy, lifecycle, identity, capacity, clock or backend failures.
    fn configure(&mut self, config: Config) -> Result<Accepted, Error>;
    /// Start the actual backend after obtaining the shared connection DATA lease.
    /// # Errors
    /// Returns explicit policy, lifecycle, identity, capacity, clock or backend failures.
    fn start(&mut self, source: u16) -> Result<u64, Error>;
    /// Advance from actual backend observations; never fabricate a sample or passing result.
    /// # Errors
    /// Returns explicit policy, lifecycle, identity, capacity, clock or backend failures.
    fn poll(
        &mut self,
        out: &mut [u8],
        max_samples: u16,
        remaining: u32,
    ) -> Result<Option<Polled>, Error>;
    /// Stop production and drop unstarted samples; None means a started event must drain.
    /// # Errors
    /// Returns explicit policy, lifecycle, identity, capacity, clock or backend failures.
    fn stop(&mut self) -> Result<(), Error>;
}
/// Borrowed connection-wide history/raw DATA owner; no allocator is provided.
/// Implementations must serialize all DATA streams, allocate nonzero stream and
/// message IDs monotonically without reuse, and retain IDs across raw-engine
/// replacement. `release` follows final whole-message admission or connection
/// teardown. `finish_batch` must only follow admission of the final DATA fragment.
pub trait DataLane {
    /// Report whether this connection-wide DATA owner can grant an exclusive lease.
    fn available(&self) -> bool;
    /// Grant a nonzero globally nonreusing stream lease, or reject busy/unavailable.
    /// # Errors
    /// Returns explicit policy, lifecycle, identity, capacity, clock or backend failures.
    fn acquire(&mut self) -> Result<u16, Error>;
    /// Verify that the given stream still owns this connection DATA lane.
    fn owns(&self, stream: u16) -> bool;
    /// Reserve a fresh nonzero connection-wide DATA ID for the existing lease.
    /// # Errors
    /// Returns explicit policy, lifecycle, identity, capacity, clock or backend failures.
    fn reserve_message(&mut self, stream: u16) -> Result<u16, Error>;
    /// Release the verified lease after drain or connection teardown.
    /// # Errors
    /// Returns explicit policy, lifecycle, identity, capacity, clock or backend failures.
    fn release(&mut self, stream: u16) -> Result<(), Error>;
    /// Confirm actual final-fragment admission for the current shared DATA message.
    /// # Errors
    /// Rejects an absent lease/message or another message ID, without releasing it.
    fn finish_message(&mut self, stream: u16, message: u16) -> Result<(), Error>;
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Actual backend start timestamp and connection-owned stream identity.
pub struct Started {
    /// Nonzero capture identity extending the externally owned stream ID.
    pub capture_id: u64,
    /// Nonzero stream identity supplied by the shared DATA owner.
    pub stream_id: u16,
    /// Actual first-sample/start monotonic microseconds.
    pub t0_mono_us: u64,
    /// Negotiated and buffer-bounded whole-batch sample limit.
    pub max_batch_samples: u16,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// V0.6 proposed capture status values; not a change to frozen wire1.2.
pub enum State {
    /// Actual work is running.
    Running = 1,
    /// Sampling stopped; a previously started event is draining.
    Stopping = 2,
    /// Sampling and whole-event drain completed.
    Stopped = 3,
    /// Actual test/backend work failed.
    Failed = 4,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Sample accounting; produced equals emitted plus dropped plus queued.
pub struct Counts {
    /// All real produced samples, including explicit hardware loss.
    pub produced: u32,
    /// Samples in complete events admitted to transport, not saved by phone.
    pub emitted: u32,
    /// Produced samples discarded before starting transmission.
    pub dropped: u32,
    /// Samples in pending and pinned started events.
    pub queued: u32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Current capture state and explicit sample accounting.
pub struct Status {
    /// Current frozen/proposed lifecycle state.
    pub state: State,
    /// Operation-specific reason; zero has no additional reason.
    pub reason: u16,
    /// Conserved sample counters.
    pub counts: Counts,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Final sample accounting and whole-DATA admission barrier.
pub struct StopSummary {
    /// All real produced samples, including explicit hardware loss.
    pub produced: u32,
    /// Samples in complete events admitted to transport, not saved by phone.
    pub emitted: u32,
    /// Produced samples discarded before starting transmission.
    pub dropped: u32,
    /// Last complete admitted DATA message ID, or zero if none.
    pub last_data_message_id: u16,
}
#[derive(Clone, Copy)]
/// Pinned whole-event metadata and samples; retries reuse this same view.
/// A router must encode the complete frozen BATCH and CRC before fragmentation.
/// This service does not claim GATT delivery or phone persistence.
pub struct Batch<'a> {
    /// Nonzero DATA message ID reserved from the shared owner.
    pub message_id: u16,
    /// Nonzero capture identity extending the externally owned stream ID.
    pub capture_id: u64,
    /// Nonzero stream identity supplied by the shared DATA owner.
    pub stream_id: u16,
    /// Capture-local batch sequence beginning at zero.
    pub batch_seq: u32,
    /// Produced-sample index, with lost samples occupying indices.
    pub first_sample: u32,
    /// Actual first-sample/start monotonic microseconds.
    pub t0_mono_us: u64,
    /// Actual fixed sample period in nanoseconds.
    pub period_ns: u32,
    /// Fixed configuration revision used by the stream.
    pub config_rev: u32,
    /// Cumulative device drops at whole-event admission.
    pub dropped_total: u32,
    /// Whole multichannel sample count in this event.
    pub sample_count: u16,
    /// Bits0 estimated timing,1 sensor overflow,2 gap; others zero.
    pub quality_flags: u16,
    /// Real sample count or borrowed encoded sample bytes.
    pub samples: &'a [u8],
}

#[derive(Clone, Copy)]
struct Pending {
    samples: u16,
    bytes: usize,
    first: u32,
    time: u64,
    quality: u16,
}
#[derive(Clone, Copy)]
struct Flight {
    batch: Pending,
    message: u16,
    sequence: u32,
    dropped: u32,
}
/// Bounded raw acquisition actor with separate pending and pinned batch buffers.
/// Call `advance` with trusted monotonic milliseconds before servicing operations
/// and periodically while draining. This actor does not own a timer or transport.
/// Capture IDs extend the externally unique stream ID to u64; no second connection
/// allocator is introduced. Call `disconnect` before dropping an active engine.
pub struct RawEngine<'a, const N: usize> {
    port: &'a mut dyn RawPort,
    lane: &'a mut dyn DataLane,
    auth: &'a dyn Authority,
    data_max: u16,
    configured: Option<(Config, Configured)>,
    next_config: u32,
    started: Option<Started>,
    state: State,
    produced: u32,
    counts_valid: bool,
    emitted: u32,
    dropped: u32,
    granted: u32,
    reserved: u32,
    next_batch: u32,
    last_message: u16,
    seen_message: u16,
    seen_stream: u16,
    pending: Option<Pending>,
    flight: Option<Flight>,
    pending_bytes: [u8; N],
    flight_bytes: [u8; N],
    scratch: [u8; N],
    closed: bool,
    last_now: u64,
    drain_deadline: Option<u64>,
}
impl core::fmt::Debug for Batch<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Batch")
            .field("sample_count", &self.sample_count)
            .field("sample_bytes", &self.samples.len())
            .finish_non_exhaustive()
    }
}
impl<const N: usize> core::fmt::Debug for RawEngine<'_, N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RawEngine")
            .field("state", &self.state)
            .field("closed", &self.closed)
            .finish_non_exhaustive()
    }
}
impl<'a, const N: usize> RawEngine<'a, N> {
    /// Construct only for a development/factory profile with actual ports and trusted scope.
    /// # Errors
    /// Returns explicit policy, lifecycle, identity, capacity, clock or backend failures.
    pub fn new(
        profile: Profile,
        port: Option<&'a mut dyn RawPort>,
        lane: &'a mut dyn DataLane,
        auth: &'a dyn Authority,
        data_max: u16,
    ) -> Result<Self, Error> {
        if profile == Profile::Commercial {
            return Err(Error::Commercial);
        }
        if !auth.allows(crate::Capability::Capture) {
            return Err(Error::Denied);
        }
        if !lane.available() {
            return Err(Error::Busy);
        }
        if !(128..=512).contains(&data_max) || N == 0 || N > 466 {
            return Err(Error::Capacity);
        }
        let port = port.ok_or(Error::Unsupported)?;
        let count = port.source_count();
        if count > 254 {
            return Err(Error::Invalid);
        }
        let mut previous = None;
        for i in 0..count {
            let source = port.source(i).ok_or(Error::Invalid)?;
            validate_source(source)?;
            if previous.is_some_and(|id| source.id <= id) {
                return Err(Error::Invalid);
            }
            previous = Some(source.id);
        }
        Ok(Self {
            port,
            lane,
            auth,
            data_max,
            configured: None,
            next_config: 1,
            started: None,
            state: State::Stopped,
            produced: 0,
            counts_valid: true,
            emitted: 0,
            dropped: 0,
            granted: 0,
            reserved: 0,
            next_batch: 0,
            last_message: 0,
            seen_message: 0,
            seen_stream: 0,
            pending: None,
            flight: None,
            pending_bytes: [0; N],
            flight_bytes: [0; N],
            scratch: [0; N],
            closed: false,
            last_now: 0,
            drain_deadline: None,
        })
    }
    /// Supply trusted monotonic milliseconds and enforce the ten-second STOP drain deadline.
    /// # Errors
    /// Returns explicit policy, lifecycle, identity, capacity, clock or backend failures.
    pub fn advance(&mut self, now: u64) -> Result<(), Error> {
        self.check()?;
        if now < self.last_now {
            self.disconnect()?;
            return Err(Error::Clock);
        }
        self.last_now = now;
        if self.drain_deadline.is_some_and(|deadline| now >= deadline) {
            let _ = self.disconnect();
            return Err(Error::Timeout);
        }
        Ok(())
    }
    /// Page only supported controlled catalogue entries; returns next index and exact count.
    /// # Errors
    /// Returns explicit policy, lifecycle, identity, capacity, clock or backend failures.
    pub fn list(&mut self, start: u8, max: u8, out: &mut [Source]) -> Result<(u8, usize), Error> {
        self.check()?;
        if start == 255 || !(1..=4).contains(&max) {
            return Err(Error::Invalid);
        }
        let start = usize::from(start);
        let total = self.port.source_count();
        let remaining = total.saturating_sub(start);
        let count = remaining.min(usize::from(max)).min(out.len());
        if remaining > 0 && count == 0 {
            return Err(Error::Capacity);
        }
        for (i, dst) in out[..count].iter_mut().enumerate() {
            let source = self.port.source(start + i).ok_or(Error::Invalid)?;
            validate_source(source)?;
            *dst = source;
        }
        let next = if start + count >= total {
            255
        } else {
            u8::try_from(start + count).map_err(|_| Error::Capacity)?
        };
        Ok((next, count))
    }
    /// Read a real supported channel descriptor and reject invalid backend metadata.
    /// # Errors
    /// Returns explicit policy, lifecycle, identity, capacity, clock or backend failures.
    pub fn channel(&mut self, source: u16, index: u8) -> Result<Channel, Error> {
        self.check()?;
        let src = self.find_source(source)?;
        self.read_channel(src, index)
    }
    /// Accept only supported stopped-state settings and actual backend rate/format.
    /// # Errors
    /// Returns explicit policy, lifecycle, identity, capacity, clock or backend failures.
    pub fn configure(&mut self, config: Config) -> Result<Configured, Error> {
        self.check()?;
        if matches!(self.state, State::Running | State::Stopping) {
            return Err(Error::InvalidState);
        }
        let source = self.find_source(config.source)?;
        if config.channel_mask == 0
            || config.channel_mask >> source.channels != 0
            || config.rate_mhz == 0
            || config.range_code != 0
            || config.options != 0
        {
            return Err(Error::Invalid);
        }
        let mut stride = 0u16;
        for i in 0..source.channels {
            if config.channel_mask & (1u32 << i) != 0 {
                let channel = self.read_channel(source, i)?;
                stride += match channel.format {
                    Format::I16 | Format::U16 => 2,
                    Format::I32 | Format::U32 => 4,
                };
            }
        }
        if self.next_config == 0 {
            return Err(Error::Capacity);
        }
        self.configured = None;
        let accepted = self.port.configure(config)?;
        self.check()?;
        if accepted.rate_mhz == 0
            || accepted.period_ns == 0
            || accepted.stride != stride
            || usize::from(stride) > N
            || stride > self.data_max - 46
        {
            return Err(Error::Invalid);
        }
        let result = Configured {
            revision: self.next_config,
            accepted,
        };
        self.next_config = self.next_config.checked_add(1).unwrap_or(0);
        self.configured = Some((config, result));
        Ok(result)
    }
    /// Start the actual backend after obtaining the shared connection DATA lease.
    /// # Errors
    /// Returns explicit policy, lifecycle, identity, capacity, clock or backend failures.
    pub fn start(&mut self, source: u16, revision: u32) -> Result<Started, Error> {
        self.check()?;
        if matches!(self.state, State::Running | State::Stopping) || self.flight.is_some() {
            return Err(Error::Busy);
        }
        let (config, binding) = self.configured.ok_or(Error::InvalidState)?;
        if config.source != source || binding.revision != revision {
            return Err(Error::Conflict);
        }
        let id = self.lane.acquire()?;
        if id == 0 || id <= self.seen_stream || !self.lane.owns(id) {
            return Err(Error::Identity);
        }
        self.seen_stream = id;
        let time = match self.port.start(source) {
            Ok(time) => time,
            Err(error) => {
                let cleanup = self.port.stop();
                let release = self.lane.release(id);
                cleanup?;
                release?;
                return Err(error);
            }
        };
        let max = usize::from(self.data_max - 46).min(N) / usize::from(binding.accepted.stride);
        let started = Started {
            capture_id: u64::from(id),
            stream_id: id,
            t0_mono_us: time,
            max_batch_samples: u16::try_from(max).map_err(|_| Error::Capacity)?,
        };
        self.started = Some(started);
        self.state = State::Running;
        self.produced = 0;
        self.counts_valid = true;
        self.emitted = 0;
        self.dropped = 0;
        self.granted = 0;
        self.reserved = 0;
        self.next_batch = 0;
        self.last_message = 0;
        self.pending = None;
        self.flight = None;
        self.pending_bytes.fill(0);
        self.flight_bytes.fill(0);
        self.check()?;
        Ok(started)
    }
    /// Advance from actual backend observations; never fabricate a sample or passing result.
    /// # Errors
    /// Returns explicit policy, lifecycle, identity, capacity, clock or backend failures.
    pub fn poll(&mut self) -> Result<(), Error> {
        self.check()?;
        if self.state != State::Running {
            return Err(Error::InvalidState);
        }
        let started = self.started.ok_or(Error::NotFound)?;
        let (_, binding) = self.configured.ok_or(Error::InvalidState)?;
        let remaining = u32::MAX - self.produced;
        if remaining == 0 {
            self.port.stop()?;
            self.state = State::Failed;
            return Err(Error::Capacity);
        }
        let polled = match self
            .port
            .poll(&mut self.scratch, started.max_batch_samples, remaining)
        {
            Ok(Some(p)) => p,
            Ok(None) => return Ok(()),
            Err(e) => return self.fail_observation(e),
        };
        self.check()?;
        let Some(bytes) =
            usize::from(polled.samples).checked_mul(usize::from(binding.accepted.stride))
        else {
            return self.fail_observation(Error::Capacity);
        };
        let Some(increment) = u32::from(polled.samples).checked_add(polled.lost) else {
            return self.fail_observation(Error::Capacity);
        };
        if polled.samples == 0
            || polled.samples > started.max_batch_samples
            || bytes != polled.bytes
            || bytes > N
            || polled.quality_flags & !7 != 0
            || polled.t0_mono_us < started.t0_mono_us
            || increment > remaining
        {
            return self.fail_observation(Error::Invalid);
        }
        let first = self.produced + polled.lost;
        self.produced += increment;
        self.dropped += polled.lost;
        if !polled.valid {
            self.dropped += u32::from(polled.samples);
            return Ok(());
        }
        if let Some(old) = self.pending.take() {
            self.dropped += u32::from(old.samples);
        }
        self.pending_bytes[..bytes].copy_from_slice(&self.scratch[..bytes]);
        self.scratch[..bytes].fill(0);
        self.pending = Some(Pending {
            samples: polled.samples,
            bytes,
            first,
            time: polled.t0_mono_us,
            quality: polled.quality_flags | if self.dropped > 0 { 4 } else { 0 },
        });
        Ok(())
    }
    /// Accept monotonic cumulative whole-message credit; repeated totals add nothing.
    /// # Errors
    /// Returns explicit policy, lifecycle, identity, capacity, clock or backend failures.
    pub fn grant(&mut self, stream: u16, total: u32) -> Result<(), Error> {
        self.check()?;
        self.identity(stream)?;
        if self.state != State::Running {
            return Err(Error::InvalidState);
        }
        if total < self.granted {
            return Err(Error::Conflict);
        }
        self.granted = total;
        Ok(())
    }
    /// Reserve one whole-event credit and shared message ID, pinning pending bytes.
    /// # Errors
    /// Returns explicit policy, lifecycle, identity, capacity, clock or backend failures.
    pub fn begin_batch(&mut self) -> Result<Batch<'_>, Error> {
        self.check()?;
        if self.flight.is_some() {
            return Err(Error::Busy);
        }
        if self.state != State::Running {
            return Err(Error::InvalidState);
        }
        let pending = self.pending.ok_or(Error::NotFound)?;
        if self.reserved >= self.granted {
            return Err(Error::NoCredit);
        }
        let started = self.started.ok_or(Error::NotFound)?;
        let next = self.next_batch.checked_add(1).ok_or(Error::Capacity)?;
        let id = self.lane.reserve_message(started.stream_id)?;
        if id == 0 || id <= self.seen_message || !self.lane.owns(started.stream_id) {
            return Err(Error::Identity);
        }
        self.seen_message = id;
        self.reserved += 1;
        self.flight_bytes[..pending.bytes].copy_from_slice(&self.pending_bytes[..pending.bytes]);
        self.pending_bytes[..pending.bytes].fill(0);
        self.pending = None;
        self.flight = Some(Flight {
            batch: pending,
            message: id,
            sequence: self.next_batch,
            dropped: self.dropped,
        });
        self.next_batch = next;
        self.current_batch()
    }
    /// Read the same pinned whole event for FIFO retries without another reservation.
    /// # Errors
    /// Returns explicit policy, lifecycle, identity, capacity, clock or backend failures.
    pub fn current_batch(&mut self) -> Result<Batch<'_>, Error> {
        self.check()?;
        let f = self.flight.ok_or(Error::NotFound)?;
        let started = self.started.ok_or(Error::NotFound)?;
        let (_, binding) = self.configured.ok_or(Error::InvalidState)?;
        Ok(Batch {
            message_id: f.message,
            capture_id: started.capture_id,
            stream_id: started.stream_id,
            batch_seq: f.sequence,
            first_sample: f.batch.first,
            t0_mono_us: f.batch.time,
            period_ns: binding.accepted.period_ns,
            config_rev: binding.revision,
            dropped_total: f.dropped,
            sample_count: f.batch.samples,
            quality_flags: f.batch.quality,
            samples: &self.flight_bytes[..f.batch.bytes],
        })
    }
    /// Mark final-fragment admission; only this advances emitted samples/barrier.
    /// # Errors
    /// Returns explicit policy, lifecycle, identity, capacity, clock or backend failures.
    pub fn finish_batch(&mut self, id: u16) -> Result<(), Error> {
        self.check()?;
        let f = self.flight.ok_or(Error::NotFound)?;
        if f.message != id {
            return Err(Error::Identity);
        }
        let stream = self.started.ok_or(Error::NotFound)?.stream_id;
        self.lane.finish_message(stream, id)?;
        self.check()?;
        self.emitted += u32::from(f.batch.samples);
        self.last_message = id;
        self.flight_bytes[..f.batch.bytes].fill(0);
        self.flight = None;
        if self.state == State::Stopping {
            self.finalize()?;
        }
        Ok(())
    }
    /// Stop production and drop unstarted samples; None means a started event must drain.
    /// # Errors
    /// Returns explicit policy, lifecycle, identity, capacity, clock or backend failures.
    pub fn stop(&mut self, stream: u16) -> Result<Option<StopSummary>, Error> {
        self.check()?;
        self.identity(stream)?;
        match self.state {
            State::Stopped => return Ok(Some(self.summary())),
            State::Failed => return Err(Error::Backend),
            State::Running => {
                let deadline = self.last_now.checked_add(10000).ok_or(Error::Clock)?;
                self.port.stop()?;
                self.state = State::Stopping;
                self.drain_deadline = Some(deadline);
                if let Some(p) = self.pending.take() {
                    self.dropped += u32::from(p.samples);
                    self.pending_bytes[..p.bytes].fill(0);
                }
            }
            State::Stopping => {}
        }
        if self.flight.is_some() {
            return Ok(None);
        }
        self.finalize()?;
        Ok(Some(self.summary()))
    }
    /// Read the retained actual capture state and conserved sample counts.
    /// # Errors
    /// Returns explicit policy, lifecycle, identity, capacity, clock or backend failures.
    pub fn status(&mut self, stream: u16) -> Result<Status, Error> {
        self.check()?;
        self.identity(stream)?;
        if !self.counts_valid {
            return Err(Error::Backend);
        }
        let queued = u32::from(self.pending.map_or(0, |p| p.samples))
            + u32::from(self.flight.map_or(0, |f| f.batch.samples));
        let counts = Counts {
            produced: self.produced,
            emitted: self.emitted,
            dropped: self.dropped,
            queued,
        };
        if self.produced != self.emitted + self.dropped + queued {
            return Err(Error::Backend);
        }
        Ok(Status {
            state: self.state,
            reason: 0,
            counts,
        })
    }
    /// Stop actual backend work and clear sensitive session staging/results.
    /// # Errors
    /// Returns explicit policy, lifecycle, identity, capacity, clock or backend failures.
    pub fn disconnect(&mut self) -> Result<(), Error> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        let mut result = Ok(());
        if let Some(started) = self.started {
            if matches!(self.state, State::Running | State::Stopping | State::Failed) {
                result = self.port.stop();
            }
            if self.lane.owns(started.stream_id) {
                let release = self.lane.release(started.stream_id);
                if result.is_ok() {
                    result = release;
                }
            }
        }
        self.pending = None;
        self.flight = None;
        self.pending_bytes.fill(0);
        self.flight_bytes.fill(0);
        self.scratch.fill(0);
        self.started = None;
        self.configured = None;
        result
    }
    fn fail_observation(&mut self, error: Error) -> Result<(), Error> {
        self.state = State::Failed;
        self.counts_valid = false;
        self.scratch.fill(0);
        let _ = self.port.stop();
        Err(error)
    }
    fn check(&mut self) -> Result<(), Error> {
        if self.closed {
            return Err(Error::Closed);
        }
        if !self.auth.allows(crate::Capability::Capture) {
            let _ = self.disconnect();
            return Err(Error::Denied);
        }
        Ok(())
    }
    fn find_source(&self, id: u16) -> Result<Source, Error> {
        for i in 0..self.port.source_count() {
            let source = self.port.source(i).ok_or(Error::Invalid)?;
            if source.id == id {
                validate_source(source)?;
                return Ok(source);
            }
        }
        Err(Error::Unsupported)
    }
    fn read_channel(&self, source: Source, index: u8) -> Result<Channel, Error> {
        if index >= source.channels {
            return Err(Error::Invalid);
        }
        let channel = self.port.channel(source.id, index)?;
        if channel.index != index
            || channel.scale_den == 0
            || channel.unit > 3
            || channel.axis > 4
            || source.formats & (1 << (channel.format as u8 - 1)) == 0
        {
            return Err(Error::Invalid);
        }
        Ok(channel)
    }
    fn identity(&self, id: u16) -> Result<(), Error> {
        match self.started {
            Some(s) if s.stream_id == id => Ok(()),
            _ => Err(Error::NotFound),
        }
    }
    fn finalize(&mut self) -> Result<(), Error> {
        if self.flight.is_some() {
            return Err(Error::Busy);
        }
        let started = self.started.ok_or(Error::NotFound)?;
        self.lane.release(started.stream_id)?;
        self.state = State::Stopped;
        self.drain_deadline = None;
        Ok(())
    }
    fn summary(&self) -> StopSummary {
        StopSummary {
            produced: self.produced,
            emitted: self.emitted,
            dropped: self.dropped,
            last_data_message_id: self.last_message,
        }
    }
}
fn validate_source(source: Source) -> Result<(), Error> {
    if source.id == 0
        || !matches!(source.kind, 1..=6 | 255)
        || !(1..=16).contains(&source.channels)
        || source.formats == 0
        || source.formats & !15 != 0
        || source.flags != 1
    {
        return Err(Error::Invalid);
    }
    Ok(())
}
