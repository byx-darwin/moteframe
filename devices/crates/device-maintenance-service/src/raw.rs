use crate::common::{Guard, entry, failure, frame, put16, put32, put64, status, u16_at, u32_at};
use crate::{CleanupTrace, Clock, Error, RawPortObserver};
use core::cell::{Cell, RefCell};
use device_connection::{FirstFragmentQueue, FirstSlotError, FirstSlotOutcome};
use device_maintenance::{
    Authority, Capability, Profile,
    raw::{Config, DataLane, RawEngine, Source, Started, StopSummary},
};
use device_protocol::{Frame, Header, Kind, ModuleEntry, RawBatch};
use device_services::{Reason, ServiceHandler, SessionError, Status};
/// End the SAME actual connection-wide DATA owner before disconnecting the engine. All producers
/// must discard old queued work; this is not normal STOP release and must never reopen that owner.
pub trait DataSessionCloser {
    /// # Errors
    /// Actual teardown/reentrant access failures remain explicit; engine cleanup is attempted anyway.
    fn end_data_session(&self) -> Result<(), Error>;
}
#[derive(Clone, Copy)]
struct Tx {
    message: u16,
    stream: u16,
    length: usize,
    offset: usize,
    frame_max: usize,
    prepared: Option<RawFragment>,
    started: u64,
    last: u64,
    budget: u64,
}
struct RawState<'a, const N: usize> {
    engine: RawEngine<'a, N>,
    started: Option<Started>,
    start_request: Option<u16>,
    admitted: bool,
    running: bool,
    stride: usize,
    bytes: [u8; 512],
    tx: Option<Tx>,
    last_confirmed: Option<RawFragment>,
    lease_active: bool,
    granted: u32,
}
/// Opaque prepared continuation receipt. The first fragment is admitted only through the real
/// reserved-slot closure, so no token can turn a FIFO-full preparation into a fake started message.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct RawFragment {
    stream: u16,
    message: u16,
    offset: usize,
    end: usize,
}
impl core::fmt::Debug for RawFragment {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RawFragment")
            .field("payload_length", &(self.end - self.offset))
            .finish_non_exhaustive()
    }
}
/// First-slot outcome, with actual in-flight message identity only after real FIFO admission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BatchStart {
    NoSlot,
    NoData,
    NoCredit,
    Admitted { message: u16, complete: bool },
}
/// Shared raw actor, borrowed by the70 handler and the trusted core07 resource bridge. `DataLane`
/// must be the same actual history/raw connection owner, not a private per-module allocator.
pub struct RawActor<'a, const N: usize> {
    state: RefCell<RawState<'a, N>>,
    authority: &'a dyn Authority,
    station: u32,
    guard: Guard<'a>,
    closer: &'a dyn DataSessionCloser,
    data_max: usize,
    cleanup: Cell<Option<Error>>,
    cleaned: Cell<bool>,
    trace: &'a CleanupTrace,
}
impl<const N: usize> core::fmt::Debug for RawActor<'_, N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RawActor")
            .field("capacity", &N)
            .finish_non_exhaustive()
    }
}
impl<'a, const N: usize> RawActor<'a, N> {
    /// `data_max` MUST be the actual successfully negotiated DATA bound for this connection.
    /// The mandatory observed port retains real stop failures outside the frozen engine.
    /// # Errors
    /// Commercial/missing authenticated capture permission, retained failed cleanup,
    /// invalid limits/source or busy lane fail.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        profile: Profile,
        port: &'a mut RawPortObserver<'_>,
        lane: &'a mut dyn DataLane,
        authority: &'a dyn Authority,
        clock: &'a dyn Clock,
        closer: &'a dyn DataSessionCloser,
        data_max: u16,
    ) -> Result<Self, Error> {
        let station = authority
            .station_id()
            .filter(|v| *v != 0)
            .ok_or(Error::Denied)?;
        let trace = port.trace;
        if let Some(error) = trace.failure() {
            return Err(error);
        }
        let engine = RawEngine::new(profile, Some(port), lane, authority, data_max)?;
        Ok(Self {
            state: RefCell::new(RawState {
                engine,
                started: None,
                start_request: None,
                admitted: false,
                running: false,
                stride: 0,
                bytes: [0; 512],
                tx: None,
                last_confirmed: None,
                lease_active: false,
                granted: 0,
            }),
            authority,
            station,
            guard: Guard::new(clock),
            closer,
            data_max: usize::from(data_max),
            cleanup: Cell::new(None),
            cleaned: Cell::new(false),
            trace,
        })
    }
    fn scope(&self) -> bool {
        self.authority.station_id() == Some(self.station)
            && self.authority.allows(Capability::Capture)
    }
    fn close(&self) -> Result<(), Error> {
        self.guard.close();
        if self.cleaned.get() {
            return self.cleanup.get().map_or(Ok(()), Err);
        }
        let end = self.closer.end_data_session();
        let Ok(mut s) = self.state.try_borrow_mut() else {
            self.cleanup.set(Some(Error::Busy));
            return Err(Error::Busy);
        };
        self.cleaned.set(true);
        s.bytes.fill(0);
        s.tx = None;
        s.last_confirmed = None;
        s.start_request = None;
        s.admitted = false;
        s.running = false;
        s.lease_active = false;
        let cleanup = s.engine.disconnect().map_err(Error::from);
        let error = self.trace.failure().or(end.err()).or(cleanup.err());
        self.cleanup.set(error);
        error.map_or(Ok(()), Err)
    }
    fn complete_engine<T>(&self, result: Result<T, Error>) -> Result<T, Error> {
        if let Some(error) = self.trace.failure() {
            let _ = self.close();
            return Err(error);
        }
        result
    }
    fn check(&self) -> Result<u64, Error> {
        if let Some(error) = self.trace.failure() {
            let _ = self.close();
            return Err(error);
        }
        if !self.scope() {
            let _ = self.close();
            return Err(Error::Denied);
        }
        let now = match self.guard.now() {
            Ok(n) => n,
            Err(e) => {
                let _ = self.close();
                return Err(e);
            }
        };
        let result = {
            let mut s = self.state.try_borrow_mut().map_err(|_| Error::Busy)?;
            if s.tx.is_some_and(|t| {
                now.saturating_sub(t.last) > 2000 || now.saturating_sub(t.started) > t.budget
            }) {
                Err(Error::Timeout)
            } else {
                s.engine.advance(now).map_err(Error::from)
            }
        };
        if let Err(e) = result {
            let _ = self.close();
            return Err(e);
        }
        Ok(now)
    }
    /// Drive real sampling/deadlines even during FIFO backpressure.
    /// # Errors
    /// Actual source/lane/time/scope failures permanently end this actor.
    pub fn poll(&self) -> Result<(), Error> {
        self.check()?;
        if !self.state.try_borrow().map_err(|_| Error::Busy)?.running {
            return Ok(());
        }
        let result = self
            .state
            .try_borrow_mut()
            .map_err(|_| Error::Busy)?
            .engine
            .poll()
            .map_err(Error::from);
        if result.is_err() {
            let _ = self.close();
        }
        result
    }
    /// Shared core07 credit bridge. Cumulative totals are accepted only for the currently admitted,
    /// running exact raw stream. No history credit or phantom stream is manufactured.
    /// # Errors
    /// Invalid/revoked/stopped stream, decreasing total and backend errors remain explicit.
    pub fn grant(&self, stream: u16, total: u32) -> Result<u32, Error> {
        self.check()?;
        let result = (|| {
            let mut s = self.state.try_borrow_mut().map_err(|_| Error::Busy)?;
            if !s.admitted || !s.running {
                return Err(Error::Unavailable);
            }
            s.engine.grant(stream, total)?;
            s.granted = total;
            Ok(total)
        })();
        self.complete_engine(result)
    }
    /// Actual live Capture authorization independent of whether any stream currently exists.
    pub fn grant_authorized(&self) -> bool {
        if self.trace.failure().is_some() {
            let _ = self.close();
        }
        !self.guard.closed() && self.scope()
    }
    /// Verify the exact current raw stream/phase and monotonic cumulative total without mutation.
    /// # Errors
    /// Scope loss, absent/stopped stream, reentrant access or decreasing total are explicit.
    pub fn validate_grant(&self, stream: u16, cumulative: u32) -> Result<(), Error> {
        if !self.grant_authorized() {
            return Err(Error::Denied);
        }
        let s = self.state.try_borrow().map_err(|_| Error::Busy)?;
        if !s.admitted || !s.running || !s.lease_active {
            return Err(Error::Unavailable);
        }
        if s.started.is_none_or(|v| v.stream_id != stream) {
            return Err(Error::NotFound);
        }
        if cumulative < s.granted {
            return Err(Error::Conflict);
        }
        Ok(())
    }
    /// A cached accepted grant remains bound to this stream until its actual release/connection end.
    pub fn grant_replay_valid(&self, stream: u16, accepted: u32) -> bool {
        self.grant_authorized()
            && self.state.try_borrow().is_ok_and(|s| {
                s.admitted
                    && s.lease_active
                    && s.started.is_some_and(|v| v.stream_id == stream)
                    && accepted <= s.granted
            })
    }
    /// Side-effect-free context check for the trusted `CoreResources` bridge; lifecycle teardown of the
    /// shared owner must close all producers at the same physical connection boundary.
    pub fn grant_context(&self, stream: u16) -> bool {
        self.grant_authorized()
            && self.state.try_borrow().is_ok_and(|s| {
                s.admitted && s.running && s.started.is_some_and(|v| v.stream_id == stream)
            })
    }
    /// Begin and actually admit the first fragment under the queue's real reservation, not `is_ready`.
    /// # Errors
    /// Failed/lost reservation after begin closes DATA and stops capture; it is never FIFO-full retry.
    pub fn start_batch<Q: FirstFragmentQueue>(
        &self,
        queue: &mut Q,
        frame_max: usize,
    ) -> Result<BatchStart, Error> {
        let now = self.check()?;
        if !(20..=512).contains(&frame_max) {
            return Err(Error::Invalid);
        }
        let mut called = false;
        let mut begun = false;
        let result = {
            let mut s = self.state.try_borrow_mut().map_err(|_| Error::Busy)?;
            if !s.admitted || !s.running {
                return Err(Error::Unavailable);
            }
            if s.tx.is_some() {
                return Err(Error::Busy);
            }
            queue.with_reserved_slot(|slot| {
                called = true;
                self.fill_first(&mut s, slot, frame_max, now, &mut begun)
            })
        };
        match result {
            Ok(FirstSlotOutcome::NoSlot) if !called => Ok(BatchStart::NoSlot),
            Ok(FirstSlotOutcome::Admitted((message, complete))) if called && begun => {
                if !self.scope() {
                    let _ = self.close();
                    return Err(Error::Denied);
                }
                let now = match self.guard.now() {
                    Ok(now) => now,
                    Err(error) => {
                        let _ = self.close();
                        return Err(error);
                    }
                };
                let finished = {
                    let mut s = self.state.try_borrow_mut().map_err(|_| Error::Busy)?;
                    if let Some(t) = s.tx.as_mut() {
                        t.started = now;
                        t.last = now;
                    }
                    if complete {
                        let result = s.engine.finish_batch(message).map_err(Error::from);
                        if result.is_ok() {
                            s.tx = None;
                            if !s.running {
                                s.lease_active = false;
                            }
                            s.bytes.fill(0);
                        }
                        result
                    } else {
                        Ok(())
                    }
                };
                if let Err(error) = finished {
                    let _ = self.close();
                    return Err(error);
                }
                Ok(BatchStart::Admitted { message, complete })
            }
            Err(FirstSlotError::Fill(Error::Engine(device_maintenance::Error::NotFound)))
                if called && !begun =>
            {
                Ok(BatchStart::NoData)
            }
            Err(FirstSlotError::Fill(Error::Engine(device_maintenance::Error::NoCredit)))
                if called && !begun =>
            {
                Ok(BatchStart::NoCredit)
            }
            Err(FirstSlotError::Fill(error)) => {
                let _ = self.close();
                Err(error)
            }
            _ => {
                let _ = self.close();
                Err(Error::Backend)
            }
        }
    }
    fn fill_first(
        &self,
        state: &mut RawState<'a, N>,
        slot: &mut [u8],
        frame_max: usize,
        now: u64,
        begun: &mut bool,
    ) -> Result<(usize, (u16, bool)), Error> {
        if slot.len() < frame_max || !self.scope() {
            return Err(Error::Invalid);
        }
        let batch = state.engine.begin_batch()?;
        *begun = true;
        let message = batch.message_id;
        let stream = batch.stream_id;
        let wire = RawBatch {
            capture_id: batch.capture_id,
            stream_id: stream,
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
        let length = wire
            .encode(&mut state.bytes, self.data_max, state.stride)
            .map_err(|_| Error::Invalid)?;
        let end = length.min(frame_max - 10);
        let header = Header {
            kind: Kind::Event,
            module: 0x70,
            opcode: 0x80,
            message_id: message,
            offset: 0,
            total_length: u16::try_from(length).map_err(|_| Error::Capacity)?,
        };
        let n = Frame {
            header,
            payload: &state.bytes[..end],
        }
        .encode(slot)
        .map_err(|_| Error::Capacity)?;
        let fragments = length.div_ceil(frame_max - 10);
        let budget = 2000 + u64::try_from(fragments - 1).map_err(|_| Error::Invalid)? * 100;
        state.tx = Some(Tx {
            message,
            stream,
            length,
            offset: end,
            frame_max,
            prepared: None,
            started: now,
            last: now,
            budget,
        });
        Ok((n, (message, end == length)))
    }
    /// Prepare an immutable continuation. Caller confirms only after actual queue admission.
    /// # Errors
    /// No flight, scope/deadline loss and capacity errors are explicit; errors clear caller bytes.
    pub fn next_frame(&self, out: &mut [u8]) -> Result<(RawFragment, usize), Error> {
        let result = (|| {
            self.check()?;
            let mut s = self.state.try_borrow_mut().map_err(|_| Error::Busy)?;
            let tx = s.tx.ok_or(Error::NotFound)?;
            if let Some(t) = s.tx.as_mut() {
                t.prepared = None;
            }
            let end = (tx.offset + tx.frame_max - 10).min(tx.length);
            let fragment = RawFragment {
                stream: tx.stream,
                message: tx.message,
                offset: tx.offset,
                end,
            };
            let header = Header {
                kind: Kind::Event,
                module: 0x70,
                opcode: 0x80,
                message_id: tx.message,
                offset: u16::try_from(tx.offset).map_err(|_| Error::Capacity)?,
                total_length: u16::try_from(tx.length).map_err(|_| Error::Capacity)?,
            };
            let n = Frame {
                header,
                payload: &s.bytes[tx.offset..end],
            }
            .encode(out)
            .map_err(|_| Error::Capacity)?;
            if let Some(t) = s.tx.as_mut() {
                t.prepared = Some(fragment);
            }
            Ok((fragment, n))
        })();
        if result.is_err() {
            out.fill(0);
        }
        result
    }
    /// # Errors
    /// Wrong/unprepared fragment or scope/time loss cannot advance or count a sample as emitted.
    pub fn confirm_frame(&self, fragment: RawFragment) -> Result<(), Error> {
        let now = self.check()?;
        let result = {
            let mut s = self.state.try_borrow_mut().map_err(|_| Error::Busy)?;
            if s.last_confirmed == Some(fragment) {
                return Ok(());
            }
            let tx = s.tx.ok_or(Error::NotFound)?;
            if tx.prepared != Some(fragment)
                || tx.message != fragment.message
                || tx.stream != fragment.stream
                || tx.offset != fragment.offset
            {
                return Err(Error::Invalid);
            }
            if fragment.end == tx.length {
                s.engine
                    .finish_batch(tx.message)
                    .map_err(Error::from)
                    .map(|()| {
                        s.bytes.fill(0);
                        s.tx = None;
                        if !s.running {
                            s.lease_active = false;
                        }
                        s.last_confirmed = Some(fragment);
                    })
            } else {
                if let Some(t) = s.tx.as_mut() {
                    t.offset = fragment.end;
                    t.last = now;
                    t.prepared = None;
                }
                s.last_confirmed = Some(fragment);
                Ok(())
            }
        };
        if result.is_err() {
            let _ = self.close();
        }
        result
    }
    /// Explicit connection teardown, including CCCD loss. Normal STOP does not call this.
    /// # Errors
    /// Real shared-owner and physical cleanup failures remain explicit.
    pub fn disconnect(&self) -> Result<(), Error> {
        self.close()
    }
    fn stop(&self, stream: u16) -> Result<Option<StopSummary>, Error> {
        self.check()?;
        let result = (|| {
            let mut s = self.state.try_borrow_mut().map_err(|_| Error::Busy)?;
            let result = s.engine.stop(stream)?;
            s.running = false;
            if result.is_some() {
                s.lease_active = false;
            }
            Ok(result)
        })();
        self.complete_engine(result)
    }
}
///70/v1.2 control adapter. Session supplies the sole request/response cache and actual response ACK.
pub struct RawHandler<'a, 'ports, const N: usize> {
    actor: &'a RawActor<'ports, N>,
    metadata: ModuleEntry,
}
impl<const N: usize> core::fmt::Debug for RawHandler<'_, '_, N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RawHandler").finish_non_exhaustive()
    }
}
impl<'a, 'ports, const N: usize> RawHandler<'a, 'ports, N> {
    /// # Errors
    /// Rejects metadata other than the frozen70/v1.2 entry and legal actual control capacities.
    pub fn new(actor: &'a RawActor<'ports, N>, metadata: ModuleEntry) -> Result<Self, Error> {
        entry(metadata, 0x70, 1, 2)?;
        Ok(Self { actor, metadata })
    }
    fn list(&self, payload: &[u8], out: &mut [u8]) -> Result<usize, Error> {
        if payload.len() != 2 {
            return Err(Error::Invalid);
        }
        let max = payload[1];
        if !(1..=4).contains(&max) {
            return Err(Error::Invalid);
        }
        if out.len() < 6 + 6 * usize::from(max) {
            return Err(Error::Capacity);
        }
        let mut entries = [Source {
            id: 0,
            kind: 0,
            channels: 0,
            formats: 0,
            flags: 0,
        }; 4];
        let (next, count) = self
            .actor
            .state
            .try_borrow_mut()
            .map_err(|_| Error::Busy)?
            .engine
            .list(payload[0], max, &mut entries)?;
        out[4] = next;
        out[5] = u8::try_from(count).map_err(|_| Error::Invalid)?;
        for (i, e) in entries[..count].iter().enumerate() {
            let p = 6 + i * 6;
            put16(out, p, e.id);
            out[p + 2] = e.kind;
            out[p + 3] = e.channels;
            out[p + 4] = e.formats;
            out[p + 5] = e.flags;
        }
        status(out, Status::Ok, Reason::None);
        Ok(6 + 6 * count)
    }
    fn channel(&self, payload: &[u8], out: &mut [u8]) -> Result<usize, Error> {
        if payload.len() != 3 {
            return Err(Error::Invalid);
        }
        if out.len() < 17 {
            return Err(Error::Capacity);
        }
        let c = self
            .actor
            .state
            .try_borrow_mut()
            .map_err(|_| Error::Busy)?
            .engine
            .channel(u16_at(payload, 0), payload[2])?;
        out[4] = c.index;
        out[5] = c.format as u8;
        put16(out, 6, c.unit);
        out[8..12].copy_from_slice(&c.scale_num.to_le_bytes());
        put32(out, 12, c.scale_den);
        out[16] = c.axis;
        status(out, Status::Ok, Reason::None);
        Ok(17)
    }
    fn execute(&mut self, f: Frame<'_>, out: &mut [u8]) -> Result<usize, Error> {
        match f.header.opcode {
            1 => self.list(f.payload, out),
            2 => self.channel(f.payload, out),
            3 => {
                if f.payload.len() != 14 {
                    return Err(Error::Invalid);
                }
                if out.len() < 18 {
                    return Err(Error::Capacity);
                }
                let c = Config {
                    source: u16_at(f.payload, 0),
                    channel_mask: u32_at(f.payload, 2),
                    rate_mhz: u32_at(f.payload, 6),
                    range_code: u16_at(f.payload, 10),
                    options: u16_at(f.payload, 12),
                };
                let mut s = self.actor.state.try_borrow_mut().map_err(|_| Error::Busy)?;
                let configured = s.engine.configure(c)?;
                s.stride = usize::from(configured.accepted.stride);
                status(out, Status::Ok, Reason::None);
                put32(out, 4, configured.revision);
                put32(out, 8, configured.accepted.rate_mhz);
                put32(out, 12, configured.accepted.period_ns);
                put16(out, 16, configured.accepted.stride);
                Ok(18)
            }
            4 => {
                if f.payload.len() != 6 {
                    return Err(Error::Invalid);
                }
                if out.len() < 24 {
                    return Err(Error::Capacity);
                }
                let mut s = self.actor.state.try_borrow_mut().map_err(|_| Error::Busy)?;
                let started = s.engine.start(u16_at(f.payload, 0), u32_at(f.payload, 2))?;
                s.started = Some(started);
                s.start_request = Some(f.header.message_id);
                s.admitted = false;
                s.running = true;
                s.lease_active = true;
                s.granted = 0;
                encode_started(started, out);
                Ok(24)
            }
            5 => {
                if f.payload.len() != 2 || u16_at(f.payload, 0) == 0 {
                    return Err(Error::Invalid);
                }
                if out.len() < 18 {
                    return Err(Error::Capacity);
                }
                let Some(summary) = self.actor.stop(u16_at(f.payload, 0))? else {
                    return Err(Error::Busy);
                };
                status(out, Status::Ok, Reason::None);
                put32(out, 4, summary.produced);
                put32(out, 8, summary.emitted);
                put32(out, 12, summary.dropped);
                put16(out, 16, summary.last_data_message_id);
                Ok(18)
            }
            6 => {
                if f.payload.len() != 2 || u16_at(f.payload, 0) == 0 {
                    return Err(Error::Invalid);
                }
                if out.len() < 19 {
                    return Err(Error::Capacity);
                }
                let r = self
                    .actor
                    .state
                    .try_borrow_mut()
                    .map_err(|_| Error::Busy)?
                    .engine
                    .status(u16_at(f.payload, 0))?;
                status(out, Status::Ok, Reason::None);
                out[4] = r.state as u8;
                put16(out, 5, r.reason);
                put32(out, 7, r.counts.produced);
                put32(out, 11, r.counts.emitted);
                put32(out, 15, r.counts.dropped);
                Ok(19)
            }
            _ => Err(Error::Unsupported),
        }
    }
}
impl<const N: usize> ServiceHandler for RawHandler<'_, '_, N> {
    fn required_features(&self, e: &ModuleEntry, op: u8) -> Option<u32> {
        (*e == self.metadata && matches!(op, 1..=6)).then_some(0)
    }
    fn authorized(&self, _: Frame<'_>) -> bool {
        self.actor.grant_authorized()
    }
    fn must_end_session(&self) -> bool {
        if !self.actor.scope() || self.actor.trace.failure().is_some() {
            let _ = self.actor.close();
        }
        self.actor.guard.closed()
    }
    fn response_ready(&mut self, f: Frame<'_>) -> Result<bool, SessionError> {
        self.actor.check().map_err(crate::common::local)?;
        if f.header.opcode != 5 {
            return Ok(true);
        }
        if f.payload.len() != 2 || u16_at(f.payload, 0) == 0 {
            return Ok(true);
        }
        match self.actor.stop(u16_at(f.payload, 0)) {
            Ok(summary) => Ok(summary.is_some()),
            Err(Error::Engine(device_maintenance::Error::NotFound)) => Ok(true),
            Err(error) => Err(crate::common::local(error)),
        }
    }
    fn replay_valid(&self, f: Frame<'_>, response: &[u8]) -> bool {
        if self.actor.guard.closed() || response.len() < 4 {
            return false;
        }
        if response[..2] != [0, 0] {
            return true;
        }
        if !self.actor.scope() {
            return false;
        }
        let Ok(s) = self.actor.state.try_borrow() else {
            return false;
        };
        match f.header.opcode {
            4 => s.started.is_some_and(|v| {
                response.len() == 24
                    && crate::common::u16_at(response, 12) == v.stream_id
                    && response[4..12] == v.capture_id.to_le_bytes()
            }),
            5 | 6 => {
                f.payload.len() == 2
                    && s.started
                        .is_some_and(|v| v.stream_id == u16_at(f.payload, 0))
            }
            1..=3 => true,
            _ => false,
        }
    }
    fn response_admitted(&mut self, f: Frame<'_>, response: &[u8]) -> Result<(), SessionError> {
        self.actor.check().map_err(crate::common::local)?;
        if f.header.opcode == 4 {
            let mut s = self
                .actor
                .state
                .try_borrow_mut()
                .map_err(|_| SessionError::InvalidHandlerResponse)?;
            let Some(started) = s.started else {
                return Err(SessionError::InvalidHandlerResponse);
            };
            let mut expected = [0; 24];
            encode_started(started, &mut expected);
            if s.start_request != Some(f.header.message_id) || response != expected {
                return Err(SessionError::InvalidHandlerResponse);
            }
            s.admitted = true;
        }
        Ok(())
    }
    fn connection_closed(&mut self) -> Result<(), SessionError> {
        self.actor.close().map_err(crate::common::local)
    }
    fn dispatch(&mut self, f: Frame<'_>, out: &mut [u8]) -> Result<usize, SessionError> {
        frame(f, self.metadata)?;
        if self.must_end_session() {
            out.fill(0);
            return Err(SessionError::AuthorizationRevoked);
        }
        self.actor.check().map_err(crate::common::local)?;
        if out.len() < 4 {
            return Err(SessionError::Capacity);
        }
        let result = self.execute(f, out);
        let result = self.actor.complete_engine(result);
        if !self.authorized(f) || self.must_end_session() {
            out.fill(0);
            return Err(SessionError::AuthorizationRevoked);
        }
        match result {
            Ok(n) => Ok(n),
            Err(Error::Capacity) => Err(SessionError::Capacity),
            Err(e) => Ok(failure(out, e)),
        }
    }
}

fn encode_started(s: Started, out: &mut [u8]) {
    status(out, Status::Ok, Reason::None);
    put64(out, 4, s.capture_id);
    put16(out, 12, s.stream_id);
    put64(out, 14, s.t0_mono_us);
    put16(out, 22, s.max_batch_samples);
}
