use crate::{
    CuffPort, Error, EventToken, HealthAccess, HealthClock, HealthProduct, LatestStore, Operation,
    ZonePort,
};
use core::{
    cell::{Cell, RefCell},
    num::NonZeroU64,
};
use device_health::{
    Backend, EndEvent, HealthStart, JobId, JobManager, Latest, Module, Outcome, ResultInfo,
};
use device_protocol::{Frame, Kind, ModuleEntry};
use device_services::{Reason, ServiceHandler, SessionError, Status};
struct State<B, const N: usize> {
    manager: JobManager<N>,
    backend: B,
}
/// One boot-global manager/backend owner. All health modules and device work must use this owner.
pub struct SharedHealthOwner<B, const N: usize> {
    state: RefCell<State<B, N>>,
    active: Cell<Option<NonZeroU64>>,
    last: Cell<u64>,
}
impl<B, const N: usize> core::fmt::Debug for SharedHealthOwner<B, N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SharedHealthOwner")
            .field("capacity", &N)
            .finish_non_exhaustive()
    }
}
impl<B, const N: usize> SharedHealthOwner<B, N> {
    /// # Errors
    /// Rejects unsupported module metadata, reused identities and an already active App connection.
    pub fn new(manager: JobManager<N>, backend: B) -> Self {
        Self {
            state: RefCell::new(State { manager, backend }),
            active: Cell::new(None),
            last: Cell::new(0),
        }
    }
    /// Checked access for real device-owned work and cleanup recovery. Never install another manager.
    /// # Errors
    /// Reports reentrant owner access as Busy and propagates real manager/backend failures.
    pub fn with_manager<R>(
        &self,
        f: impl FnOnce(&mut JobManager<N>, &mut B) -> Result<R, Error>,
    ) -> Result<R, Error> {
        let mut s = self.state.try_borrow_mut().map_err(|_| Error::Busy)?;
        let State { manager, backend } = &mut *s;
        f(manager, backend)
    }
}
/// One connection-owned handler for every selected health module. One active App connection uses
/// the boot-global owner at a time; device-owned jobs continue across App disconnects.
pub struct HealthHandler<'a, B: Backend, const N: usize, const E: usize = 512> {
    owner: &'a SharedHealthOwner<B, N>,
    entries: &'a [ModuleEntry],
    access: &'a dyn HealthAccess,
    clock: &'a dyn HealthClock,
    product: &'a dyn HealthProduct,
    latest: &'a mut dyn LatestStore,
    zones: Option<&'a mut dyn ZonePort>,
    cuff: Option<&'a mut dyn CuffPort>,
    connection: u64,
    watched: [Cell<Option<(u8, u32, u8)>>; N],
    closed: Cell<bool>,
    cleanup_failed: Cell<bool>,
    last_now: Cell<Option<u64>>,
    start_ack: Cell<Option<(u16, u8, u32)>>,
    stop_ack: Cell<Option<(u16, u8, u32)>>,
    last_admission: Cell<Option<u16>>,
    latest_guard: Cell<Option<(u16, u8, u16, u64)>>,
    event: Cell<Option<EventToken>>,
    event_len: Cell<usize>,
    event_bytes: RefCell<[u8; E]>,
    last_event: Cell<Option<EventToken>>,
}
impl<B: Backend, const N: usize, const E: usize> core::fmt::Debug for HealthHandler<'_, B, N, E> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HealthHandler")
            .field("closed", &self.closed.get())
            .finish_non_exhaustive()
    }
}
impl<'a, B: Backend, const N: usize> HealthHandler<'a, B, N> {
    /// Bind the compatibility 512-byte event workspace.
    /// # Errors
    /// Same metadata, lifecycle and capacity failures as `new_bounded`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        owner: &'a SharedHealthOwner<B, N>,
        connection: u64,
        entries: &'a [ModuleEntry],
        access: &'a dyn HealthAccess,
        clock: &'a dyn HealthClock,
        product: &'a dyn HealthProduct,
        latest: &'a mut dyn LatestStore,
        zones: Option<&'a mut dyn ZonePort>,
    ) -> Result<Self, Error> {
        Self::new_bounded(
            owner, connection, entries, access, clock, product, latest, zones,
        )
    }
}
impl<'a, B: Backend, const N: usize, const E: usize> HealthHandler<'a, B, N, E> {
    #[allow(clippy::too_many_arguments)]
    /// # Errors
    /// Rejects unsupported module metadata, reused identities and an already active App connection.
    pub fn new_bounded(
        owner: &'a SharedHealthOwner<B, N>,
        connection: u64,
        entries: &'a [ModuleEntry],
        access: &'a dyn HealthAccess,
        clock: &'a dyn HealthClock,
        product: &'a dyn HealthProduct,
        latest: &'a mut dyn LatestStore,
        zones: Option<&'a mut dyn ZonePort>,
    ) -> Result<Self, Error> {
        if !(128..=512).contains(&E) || connection == 0 || connection <= owner.last.get() {
            return Err(Error::Invalid);
        }
        if owner.active.get().is_some() {
            return Err(Error::Busy);
        }
        if entries.is_empty()
            || entries.windows(2).any(|w| w[0].module >= w[1].module)
            || entries.iter().any(|e| {
                Module::from_wire(e.module).is_err()
                    || e.major != 1
                    || e.minor != 0
                    || e.availability != 1
                    || e.features != 0
                    || !(128..=512).contains(&e.input_max)
                    || !(128..=512).contains(&e.output_max)
                    || usize::from(e.output_max) > E
            })
        {
            return Err(Error::Invalid);
        }
        if zones.is_some() && !entries.iter().any(|e| e.module == 0x20) {
            return Err(Error::Invalid);
        }
        let active = NonZeroU64::new(connection).ok_or(Error::Invalid)?;
        owner.last.set(connection);
        owner.active.set(Some(active));
        Ok(Self {
            owner,
            entries,
            access,
            clock,
            product,
            latest,
            zones,
            connection,
            cuff: None,
            watched: core::array::from_fn(|_| Cell::new(None)),
            closed: Cell::new(false),
            cleanup_failed: Cell::new(false),
            last_now: Cell::new(None),
            start_ack: Cell::new(None),
            stop_ack: Cell::new(None),
            last_admission: Cell::new(None),
            latest_guard: Cell::new(None),
            event: Cell::new(None),
            event_len: Cell::new(0),
            event_bytes: RefCell::new([0; E]),
            last_event: Cell::new(None),
        })
    }
    /// Select an actual cuff port only for a compiled/registered blood-pressure module.
    /// # Errors
    /// Rejects a missing blood-pressure route; no cuff capability is inferred from BP estimates.
    pub fn with_cuff(mut self, port: &'a mut dyn CuffPort) -> Result<Self, Error> {
        if self.entry(0x25).is_none() {
            self.close_inner()?;
            return Err(Error::Health(device_health::Error::Unsupported));
        }
        self.cuff = Some(port);
        Ok(self)
    }
    /// Prepare actual25.E82 cuff pressure. No PPG-derived estimate can create this event.
    /// # Errors
    /// Rejects missing actual cuff capability, nonrunning jobs, scope loss and carrier capacity.
    pub fn prepare_cuff(
        &mut self,
        id: u32,
        out: &mut [u8],
        max: usize,
    ) -> Result<Option<(EventToken, usize)>, Error> {
        let result = (|| {
            self.protected(0x25, Operation::CuffEvent, Some(id))?;
            if let Some(t) = self.pending_for(0x25, id, 0x82)? {
                return self.copy_event(t, out, max).map(Some);
            }
            let snapshot = self.owner.with_manager(|m, _| {
                m.query(Module::from_wire(0x25)?, JobId::new(id)?)
                    .map_err(Error::from)
            })?;
            if snapshot.state != device_health::JobState::Running {
                return Err(Error::Health(device_health::Error::InvalidState));
            }
            let cuff = self
                .cuff
                .as_ref()
                .ok_or(Error::Health(device_health::Error::Unsupported))?;
            if !cuff.running_cuff_job(id) {
                return Err(Error::Health(device_health::Error::Unavailable));
            }
            let Some((sample, pressure)) = cuff.pending(id)? else {
                return Ok(None);
            };
            if sample == 0 || pressure.job != id {
                return Err(Error::Invalid);
            }
            let mut body = [0; 9];
            pressure.encode(&mut body)?;
            let t = EventToken {
                connection: self.connection,
                module: 0x25,
                opcode: 0x82,
                job: id,
                sample,
            };
            self.stage(t, &body)?;
            self.copy_event(t, out, max).map(Some)
        })();
        if result.is_err() {
            out.fill(0);
        }
        result
    }
    fn event_context(&self, t: EventToken) -> bool {
        match (t.module, t.opcode) {
            (0x20, 0x82) => self
                .zones
                .as_ref()
                .is_some_and(|z| z.current(t.job, t.sample)),
            (0x25, 0x82) => {
                self.cuff
                    .as_ref()
                    .is_some_and(|c| c.running_cuff_job(t.job) && c.current(t.job, t.sample))
                    && self
                        .owner
                        .with_manager(|m, _| {
                            m.query(Module::from_wire(t.module)?, JobId::new(t.job)?)
                                .map(|s| s.state == device_health::JobState::Running)
                                .map_err(Error::from)
                        })
                        .unwrap_or(false)
            }
            _ => true,
        }
    }
    fn entry(&self, module: u8) -> Option<&ModuleEntry> {
        self.entries.iter().find(|e| e.module == module)
    }
    fn permits(&self, module: u8, op: Operation, id: Option<u32>) -> bool {
        self.access.allowed(module, op, None)
            && id.is_none_or(|v| self.access.allowed(module, op, Some(v)))
    }
    fn close_inner(&self) -> Result<(), Error> {
        if self.closed.replace(true) {
            return if self.cleanup_failed.get() {
                Err(Error::Backend)
            } else {
                Ok(())
            };
        }
        for slot in &self.watched {
            slot.set(None);
        }
        self.event.set(None);
        self.event_len.set(0);
        self.start_ack.set(None);
        self.stop_ack.set(None);
        self.latest_guard.set(None);
        if let Ok(mut bytes) = self.event_bytes.try_borrow_mut() {
            bytes.fill(0);
        }
        let result = self
            .owner
            .with_manager(|m, b| m.disconnect(b).map_err(Error::from));
        if self.owner.active.get().map(NonZeroU64::get) == Some(self.connection) {
            self.owner.active.set(None);
        }
        self.cleanup_failed.set(result.is_err());
        result
    }
    /// Explicit cleanup. Keep the owner for asynchronous or failed backend cleanup recovery.
    /// # Errors
    /// Actual backend cleanup failures are returned; resources remain managed for explicit recovery.
    pub fn disconnect(&mut self) -> Result<(), Error> {
        self.close_inner()
    }
    fn touch(&self) -> Result<(), Error> {
        if self.must_end_session() {
            return Err(Error::Closed);
        }
        let now = match self.clock.now_ms() {
            Ok(now) => now,
            Err(error) => {
                let _ = self.close_inner();
                return Err(error);
            }
        };
        if self.last_now.get().is_some_and(|last| now < last) {
            let _ = self.close_inner();
            return Err(Error::Clock);
        }
        self.last_now.set(Some(now));
        Ok(())
    }
    fn protected(&self, module: u8, op: Operation, id: Option<u32>) -> Result<(), Error> {
        self.touch()?;
        if self.entry(module).is_none() {
            return Err(Error::Health(device_health::Error::Unsupported));
        }
        if !self.permits(module, op, id) {
            let _ = self.close_inner();
            return Err(Error::Denied);
        }
        Ok(())
    }
    fn scope(&self, f: Frame<'_>) -> Option<(Operation, Option<u32>)> {
        match f.header.opcode {
            1 => Some((Operation::Status, None)),
            0x30 => Some((Operation::Start, None)),
            0x31 | 0x32 => Some((
                if f.header.opcode == 0x31 {
                    Operation::Stop
                } else {
                    Operation::Query
                },
                JobId::decode(f.payload).ok().map(JobId::get),
            )),
            2 => Some((
                Operation::Latest,
                latest_type(f.header.module, f.payload).ok().map(u32::from),
            )),
            0x33 if f.header.module == 0x20 && self.zones.is_some() => {
                Some((Operation::Zones, context(f.payload).ok()))
            }
            _ => None,
        }
    }
    fn is_job_live(&self, module: u8, id: u32) -> bool {
        Module::from_wire(module)
            .ok()
            .zip(JobId::new(id).ok())
            .is_some_and(|(module, id)| {
                self.owner
                    .with_manager(|m, _| m.query(module, id).map(|_| ()).map_err(Error::from))
                    .is_ok()
            })
    }
    fn validate_product(
        &self,
        module: u8,
        record_type: u16,
        schema: u16,
        payload: &[u8],
        source: device_health::ResultSource,
    ) -> Result<(), Error> {
        let declaration = self
            .product
            .declaration_for_source(module, record_type, source)?;
        let provenance = self.product.provenance(module, record_type, source)?;
        if declaration.module.wire() != module
            || declaration.record_type != record_type
            || provenance.source != source
        {
            return Err(Error::Invalid);
        }
        validate_declared(&declaration, &provenance, schema, payload)
    }
    fn validate_start(&self, r: HealthStart) -> Result<(), Error> {
        validate_start(self.product, r)
    }
    fn execute_status(&self, module: u8, payload: &[u8], out: &mut [u8]) -> Result<usize, Error> {
        if !payload.is_empty() {
            return Err(Error::Invalid);
        }
        if out.len() < 7 {
            return Err(Error::Capacity);
        }
        self.product
            .module_run_status(module)?
            .encode(&mut out[4..7])?;
        Ok(7)
    }
    fn execute_start(&self, module: Module, f: Frame<'_>, out: &mut [u8]) -> Result<usize, Error> {
        if out.len() < 8 {
            return Err(Error::Capacity);
        }
        let r = HealthStart::decode(module.wire(), f.payload)?;
        self.validate_start(r)?;
        self.protected(module.wire(), Operation::Start, None)?;
        let slot = self
            .watched
            .iter()
            .find(|s| s.get().is_none())
            .ok_or(Error::Health(device_health::Error::Capacity))?;
        let (owner, source) = self.product.start_binding(r)?;
        self.protected(module.wire(), Operation::Start, None)?;
        let started = self.owner.with_manager(|m, b| {
            let mut guarded = crate::start_guard::GuardedBackend::new(b, self.access, self.product);
            let result = m.start(r, owner, source, &mut guarded);
            if guarded.denied() {
                Err(Error::Denied)
            } else if let Some(error) = guarded.validation_failure() {
                Err(error)
            } else {
                result.map_err(Error::from)
            }
        });
        let id = match started {
            Err(Error::Denied) => {
                let _ = self.close_inner();
                return Err(Error::Denied);
            }
            result => result?,
        };
        slot.set(Some((module.wire(), id.get(), r.temp_mode())));
        self.start_ack
            .set(Some((f.header.message_id, module.wire(), id.get())));
        if !self.permits(module.wire(), Operation::Start, Some(id.get())) {
            let _ = self.close_inner();
            return Err(Error::Denied);
        }
        out[4..8].copy_from_slice(&id.get().to_le_bytes());
        Ok(8)
    }
    fn execute(&mut self, f: Frame<'_>, out: &mut [u8]) -> Result<usize, Error> {
        let module = Module::from_wire(f.header.module)?;
        match f.header.opcode {
            1 => self.execute_status(module.wire(), f.payload, out),
            0x30 => self.execute_start(module, f, out),
            0x31 => {
                let id = JobId::decode(f.payload)?;
                self.owner
                    .with_manager(|m, b| m.stop(module, id, b).map_err(Error::from))?;
                self.stop_ack
                    .set(Some((f.header.message_id, module.wire(), id.get())));
                Ok(4)
            }
            0x32 => {
                if out.len() < 7 {
                    return Err(Error::Capacity);
                }
                let id = JobId::decode(f.payload)?;
                let snapshot = self
                    .owner
                    .with_manager(|m, _| m.query(module, id).map_err(Error::from))?;
                out[4] = snapshot.state as u8;
                out[5..7].copy_from_slice(&outcome_reason(snapshot.outcome).to_le_bytes());
                Ok(7)
            }
            2 => {
                let t = latest_type(module.wire(), f.payload)?;
                let mut payload = [0; E];
                let stored = self.latest.load(module.wire(), t, &mut payload)?;
                let i = stored.info;
                if stored.generation == 0
                    || i.sample_id == 0
                    || i.record_type != t
                    || i.length > payload.len()
                {
                    return Err(Error::Backend);
                }
                self.validate_product(module.wire(), t, i.schema, &payload[..i.length], i.source)?;
                let latest = Latest::new(
                    module.wire(),
                    t,
                    i.schema,
                    i.timestamp,
                    &payload[..i.length],
                )?;
                let total = 19usize.checked_add(i.length).ok_or(Error::Capacity)?;
                if total > out.len() {
                    return Err(Error::Capacity);
                }
                latest.encode(&mut out[4..total])?;
                self.latest_guard.set(Some((
                    f.header.message_id,
                    module.wire(),
                    t,
                    stored.generation,
                )));
                Ok(total)
            }
            0x33 if module.wire() == 0x20 => {
                if out.len() < 35 {
                    return Err(Error::Capacity);
                }
                let c = context(f.payload)?;
                let snapshot = self.zones.as_ref().ok_or(Error::Invalid)?.snapshot(c)?;
                snapshot.encode(&mut out[4..35])?;
                Ok(35)
            }
            _ => Err(Error::Health(device_health::Error::Unsupported)),
        }
    }
    /// Poll real completion; a retained result must be drained before this can produce END.
    /// # Errors
    /// Rejects scope loss, unknown jobs, pending results and actual backend faults.
    pub fn poll_job(&mut self, module: u8, id: u32) -> Result<device_health::JobState, Error> {
        self.protected(module, Operation::Query, Some(id))?;
        self.owner.with_manager(|m, b| {
            m.poll_completion(Module::from_wire(module)?, JobId::new(id)?, b)
                .map_err(Error::from)
        })
    }
    fn copy_event(
        &self,
        token: EventToken,
        out: &mut [u8],
        logical_max: usize,
    ) -> Result<(EventToken, usize), Error> {
        if !(128..=512).contains(&logical_max)
            || self.event_len.get() > logical_max
            || out.len() < self.event_len.get()
        {
            return Err(Error::Capacity);
        }
        if !self.event_context(token) {
            let _ = self.close_inner();
            return Err(Error::Denied);
        }
        let bytes = self.event_bytes.try_borrow().map_err(|_| Error::Busy)?;
        let n = self.event_len.get();
        out[..n].copy_from_slice(&bytes[..n]);
        Ok((token, n))
    }
    fn pending_for(&self, module: u8, id: u32, opcode: u8) -> Result<Option<EventToken>, Error> {
        if let Some(t) = self.event.get() {
            if t.module != module || t.job != id || t.opcode != opcode {
                return Err(Error::Busy);
            }
            return Ok(Some(t));
        }
        if self
            .stop_ack
            .get()
            .is_some_and(|(_, m, j)| m == module && j == id)
        {
            return Err(Error::Busy);
        }
        Ok(None)
    }
    fn stage(&self, token: EventToken, body: &[u8]) -> Result<(), Error> {
        let n = body.len();
        if n > E {
            return Err(Error::Capacity);
        }
        let mut bytes = self.event_bytes.try_borrow_mut().map_err(|_| Error::Busy)?;
        bytes[..body.len()].copy_from_slice(body);
        self.event_len.set(n);
        self.event.set(Some(token));
        Ok(())
    }
    /// Prepare exactly the RESULT LATEST logical body, without a command prefix or CRC. Retry is immutable and does
    /// not acknowledge the backend. Caller supplies the actual negotiated outgoing logical maximum.
    /// # Errors
    /// Rejects absent permission, premature admission, invalid provenance and actual carrier/buffer limits.
    pub fn prepare_result(
        &mut self,
        module: u8,
        id: u32,
        out: &mut [u8],
        logical_max: usize,
    ) -> Result<Option<(EventToken, usize)>, Error> {
        let result = self.result_inner(module, id, out, logical_max);
        if result.is_err() {
            out.fill(0);
        }
        result
    }
    fn result_inner(
        &mut self,
        module: u8,
        id: u32,
        out: &mut [u8],
        max: usize,
    ) -> Result<Option<(EventToken, usize)>, Error> {
        self.protected(module, Operation::ResultEvent, Some(id))?;
        if let Some(t) = self.pending_for(module, id, 0x80)? {
            return self.copy_event(t, out, max).map(Some);
        }
        let mut payload = [0; E];
        let Some(e) = self.owner.with_manager(|m, b| {
            m.fetch_result(Module::from_wire(module)?, JobId::new(id)?, b, &mut payload)
                .map_err(Error::from)
        })?
        else {
            return Ok(None);
        };
        if module == 0x28
            && self
                .watched
                .iter()
                .filter_map(Cell::get)
                .find(|(m, j, _)| *m == module && *j == id)
                .is_some_and(|(_, _, mode)| e.latest.payload().get(2) != Some(&mode))
        {
            return Err(Error::Invalid);
        }
        self.validate_product(
            module,
            e.latest.record_type(),
            e.latest.schema(),
            e.latest.payload(),
            e.source,
        )?;
        let n = e
            .latest
            .check_carrier(device_health::ResultCarrier::EventLatest, max.min(E))?;
        if out.len() < n {
            return Err(Error::Capacity);
        }
        let info = ResultInfo {
            sample_id: e.sample_id,
            record_type: e.latest.record_type(),
            schema: e.latest.schema(),
            timestamp: e.latest.timestamp(),
            length: e.latest.payload().len(),
            source: e.source,
        };
        self.latest.publish(module, id, info, e.latest.payload())?;
        let mut body = [0; E];
        e.latest.encode(&mut body[..n])?;
        let token = EventToken {
            connection: self.connection,
            module,
            opcode: 0x80,
            job: id,
            sample: e.sample_id,
        };
        self.stage(token, &body[..n])?;
        self.protected(module, Operation::ResultEvent, Some(id))?;
        self.copy_event(token, out, max).map(Some)
    }
    /// Prepare a definite terminal event; STOP response admission and all results precede it.
    /// # Errors
    /// Rejects scope loss, an undrained result/event, unknown jobs and carrier/buffer limits.
    pub fn prepare_end(
        &mut self,
        module: u8,
        id: u32,
        out: &mut [u8],
        max: usize,
    ) -> Result<Option<(EventToken, usize)>, Error> {
        let result = (|| {
            self.protected(module, Operation::EndEvent, Some(id))?;
            if let Some(t) = self.pending_for(module, id, 0x81)? {
                return self.copy_event(t, out, max).map(Some);
            }
            let Some(e) = self.owner.with_manager(|m, _| {
                m.pending_end(Module::from_wire(module)?, JobId::new(id)?)
                    .map_err(Error::from)
            })?
            else {
                return Ok(None);
            };
            let mut body = [0; 10];
            encode_end(e, &mut body);
            let t = EventToken {
                connection: self.connection,
                module,
                opcode: 0x81,
                job: id,
                sample: 0,
            };
            self.stage(t, &body)?;
            self.copy_event(t, out, max).map(Some)
        })();
        if result.is_err() {
            out.fill(0);
        }
        result
    }
    /// Prepare an actual versioned context-zone event, with no implicit context allocation.
    /// # Errors
    /// Rejects missing/expired real contexts, scope loss and carrier/buffer limits.
    pub fn prepare_zones(
        &mut self,
        c: u32,
        out: &mut [u8],
        max: usize,
    ) -> Result<Option<(EventToken, usize)>, Error> {
        let result = (|| {
            self.protected(0x20, Operation::Zones, Some(c))?;
            if c == 0 {
                return Err(Error::Invalid);
            }
            if let Some(t) = self.pending_for(0x20, c, 0x82)? {
                return self.copy_event(t, out, max).map(Some);
            }
            let Some((revision, zones)) = self.zones.as_ref().ok_or(Error::Invalid)?.pending(c)?
            else {
                return Ok(None);
            };
            if revision == 0 {
                return Err(Error::Invalid);
            }
            let mut body = [0; 35];
            body[..4].copy_from_slice(&c.to_le_bytes());
            zones.encode(&mut body[4..])?;
            let t = EventToken {
                connection: self.connection,
                module: 0x20,
                opcode: 0x82,
                job: c,
                sample: revision,
            };
            self.stage(t, &body)?;
            self.copy_event(t, out, max).map(Some)
        })();
        if result.is_err() {
            out.fill(0);
        }
        result
    }
    /// Confirm only after the final fragment entered the same actual FIFO. Exact repeats are safe;
    /// failure retains the immutable event. This does not prove peer receipt or durable storage.
    /// # Errors
    /// Rejects mismatched receipts, revoked scope, stale contexts and actual acknowledgement failures.
    pub fn confirm_event(&mut self, token: EventToken) -> Result<(), Error> {
        let op = match token.opcode {
            0x80 => Operation::ResultEvent,
            0x81 => Operation::EndEvent,
            0x82 if token.module == 0x20 => Operation::Zones,
            0x82 if token.module == 0x25 => Operation::CuffEvent,
            _ => return Err(Error::Invalid),
        };
        self.protected(token.module, op, Some(token.job))?;
        if token.connection != self.connection {
            return Err(Error::Invalid);
        }
        if self.last_event.get() == Some(token) {
            return Ok(());
        }
        if self.event.get() != Some(token) {
            return Err(Error::Invalid);
        }
        if !self.event_context(token) {
            let _ = self.close_inner();
            return Err(Error::Denied);
        }
        match token.opcode {
            0x80 => self.owner.with_manager(|m, b| {
                m.confirm_result(
                    Module::from_wire(token.module)?,
                    JobId::new(token.job)?,
                    token.sample,
                    b,
                )
                .map_err(Error::from)
            })?,
            0x81 => self.owner.with_manager(|m, _| {
                m.confirm_end_event(Module::from_wire(token.module)?, JobId::new(token.job)?)
                    .map_err(Error::from)
            })?,
            0x82 if token.module == 0x25 => self
                .cuff
                .as_mut()
                .ok_or(Error::Invalid)?
                .confirm(token.job, token.sample)?,
            0x82 => self
                .zones
                .as_mut()
                .ok_or(Error::Invalid)?
                .confirm(token.job, token.sample)?,
            _ => return Err(Error::Invalid),
        }
        if token.opcode == 0x81 {
            for slot in &self.watched {
                if slot
                    .get()
                    .is_some_and(|(module, id, _)| module == token.module && id == token.job)
                {
                    slot.set(None);
                }
            }
        }
        self.event.set(None);
        self.event_len.set(0);
        self.event_bytes
            .try_borrow_mut()
            .map_err(|_| Error::Busy)?
            .fill(0);
        self.last_event.set(Some(token));
        Ok(())
    }
}
impl<B: Backend, const N: usize, const E: usize> ServiceHandler for HealthHandler<'_, B, N, E> {
    fn required_features(&self, e: &ModuleEntry, op: u8) -> Option<u32> {
        (self.entry(e.module) == Some(e)
            && (matches!(op, 1 | 2 | 0x30..=0x32)
                || (e.module == 0x20 && op == 0x33 && self.zones.is_some())))
        .then_some(0)
    }
    fn authorized(&self, f: Frame<'_>) -> bool {
        if self.closed.get() {
            return false;
        }
        let Some((op, id)) = self.scope(f) else {
            return true;
        };
        self.permits(f.header.module, op, id)
    }
    fn must_end_session(&self) -> bool {
        if !self.closed.get()
            && self.watched.iter().any(|s| {
                s.get()
                    .is_some_and(|(m, j, _)| !self.permits(m, Operation::Start, Some(j)))
            })
        {
            let _ = self.close_inner();
        }
        self.closed.get()
    }
    #[inline(never)]
    fn response_ready(&mut self, f: Frame<'_>) -> Result<bool, SessionError> {
        self.touch().map_err(local_error)?;
        if f.header.opcode == 0x31 {
            let Ok(id) = JobId::decode(f.payload) else {
                return Ok(true);
            };
            return Ok(self
                .event
                .get()
                .is_none_or(|e| e.module != f.header.module || e.job != id.get()));
        }
        Ok(true)
    }
    fn replay_valid(&self, f: Frame<'_>, response: &[u8]) -> bool {
        if self.closed.get() || response.len() < 4 {
            return false;
        }
        if response[..2] != [0, 0] {
            return true;
        }
        match f.header.opcode {
            1 => response.len() == 7,
            0x30 if response.len() == 8 => {
                let id = u32::from_le_bytes([response[4], response[5], response[6], response[7]]);
                self.permits(f.header.module, Operation::Start, Some(id))
                    && self.is_job_live(f.header.module, id)
            }
            0x31 | 0x32 => {
                JobId::decode(f.payload).is_ok_and(|id| self.is_job_live(f.header.module, id.get()))
            }
            2 => self
                .latest_guard
                .get()
                .is_some_and(|(request, module, t, generation)| {
                    request == f.header.message_id
                        && module == f.header.module
                        && self.latest.current(module, t, generation)
                }),
            0x33 => context(f.payload)
                .is_ok_and(|c| self.zones.as_ref().is_some_and(|z| z.snapshot(c).is_ok())),
            _ => false,
        }
    }
    fn response_admitted(&mut self, f: Frame<'_>, response: &[u8]) -> Result<(), SessionError> {
        self.touch().map_err(local_error)?;
        if self.last_admission.get() == Some(f.header.message_id) {
            return Ok(());
        }
        if !self.authorized(f) || !self.replay_valid(f, response) {
            let _ = self.close_inner();
            return Err(SessionError::AuthorizationRevoked);
        }
        if f.header.opcode == 0x30 {
            let Some((request, module, id)) = self.start_ack.get() else {
                return Err(SessionError::InvalidHandlerResponse);
            };
            if request != f.header.message_id
                || module != f.header.module
                || response.len() != 8
                || response[4..8] != id.to_le_bytes()
            {
                return Err(SessionError::InvalidHandlerResponse);
            }
            self.owner
                .with_manager(|m, _| {
                    m.confirm_start_response(Module::from_wire(module)?, JobId::new(id)?)
                        .map_err(Error::from)
                })
                .map_err(local_error)?;
            self.start_ack.set(None);
        }
        if f.header.opcode == 0x31 {
            self.stop_ack.set(None);
        }
        self.last_admission.set(Some(f.header.message_id));
        Ok(())
    }
    fn connection_closed(&mut self) -> Result<(), SessionError> {
        self.close_inner().map_err(local_error)
    }
    fn dispatch(&mut self, f: Frame<'_>, out: &mut [u8]) -> Result<usize, SessionError> {
        self.touch().map_err(local_error)?;
        if f.header.kind != Kind::Request
            || f.header.offset != 0
            || f.header.message_id == 0
            || usize::from(f.header.total_length) != f.payload.len()
        {
            return Err(SessionError::InvalidFrame);
        }
        if out.len() < 4 {
            return Err(SessionError::Capacity);
        }
        let Some(e) = self.entry(f.header.module).copied() else {
            return Ok(status(out, Status::UnsupportedModule, Reason::None));
        };
        if self.required_features(&e, f.header.opcode).is_none() {
            return Ok(status(out, Status::UnsupportedOpcode, Reason::None));
        }
        if f.payload.len() > usize::from(e.input_max) {
            return Ok(status(out, Status::InvalidArgument, Reason::None));
        }
        if !self.authorized(f) {
            return Ok(status(out, Status::AccessDenied, Reason::RoleRequired));
        }
        let limit = out.len().min(usize::from(e.output_max));
        let result = self.execute(f, &mut out[..limit]);
        if !self.authorized(f) || self.closed.get() {
            out.fill(0);
            let _ = self.close_inner();
            return Err(SessionError::AuthorizationRevoked);
        }
        match result {
            Ok(n) => {
                status(out, Status::Ok, Reason::None);
                Ok(n)
            }
            Err(Error::Capacity) => Err(SessionError::Capacity),
            Err(Error::Denied | Error::Closed | Error::Clock) => {
                out.fill(0);
                Err(SessionError::AuthorizationRevoked)
            }
            Err(error) => Ok(status(out, error_status(error), Reason::None)),
        }
    }
}
fn latest_type(module: u8, bytes: &[u8]) -> Result<u16, Error> {
    let t = if module == 0x2a {
        if bytes.len() != 2 {
            return Err(Error::Invalid);
        }
        u16::from_le_bytes([bytes[0], bytes[1]])
    } else {
        if !bytes.is_empty() {
            return Err(Error::Invalid);
        }
        match module {
            0x20 => 0x20,
            0x25 => 0x2501,
            0x26 => 0x2601,
            0x27 => 0x2701,
            0x28 => 0x2801,
            0x29 => 0x2901,
            0x2b => 0x2b01,
            _ => return Err(Error::Invalid),
        }
    };
    if !Module::from_wire(module)?.accepts(t) {
        return Err(Error::Invalid);
    }
    Ok(t)
}
fn context(b: &[u8]) -> Result<u32, Error> {
    let id = JobId::decode(b)?;
    Ok(id.get())
}
fn outcome_reason(o: Option<Outcome>) -> u16 {
    match o {
        Some(Outcome::Cancelled(r) | Outcome::Failed(r)) => r as u16,
        _ => 0,
    }
}
fn encode_end(e: EndEvent, b: &mut [u8; 10]) {
    b[..4].copy_from_slice(&e.job.get().to_le_bytes());
    b[4] = e.source as u8;
    b[5] = e.temp_mode;
    let s = match e.outcome {
        Outcome::Completed => Status::Ok,
        Outcome::Cancelled(_) => Status::InvalidState,
        Outcome::Failed(_) => Status::HardwareFailure,
    };
    b[6..8].copy_from_slice(&(s as u16).to_le_bytes());
    b[8..10].copy_from_slice(&outcome_reason(Some(e.outcome)).to_le_bytes());
}
fn status(b: &mut [u8], s: Status, r: Reason) -> usize {
    b[..2].copy_from_slice(&(s as u16).to_le_bytes());
    b[2..4].copy_from_slice(&(r as u16).to_le_bytes());
    4
}
fn local_error(e: Error) -> SessionError {
    match e {
        Error::Capacity => SessionError::Capacity,
        Error::Denied | Error::Closed | Error::Clock => SessionError::AuthorizationRevoked,
        _ => SessionError::InvalidHandlerResponse,
    }
}
fn error_status(e: Error) -> Status {
    match e {
        Error::Busy | Error::Health(device_health::Error::Busy) => Status::Busy,
        Error::Health(device_health::Error::NotFound) => Status::NotFound,
        Error::Health(device_health::Error::Unsupported) => Status::UnsupportedOpcode,
        Error::Health(device_health::Error::Unavailable | device_health::Error::InvalidState) => {
            Status::InvalidState
        }
        Error::Health(device_health::Error::Capacity | device_health::Error::Exhausted) => {
            Status::LimitExceeded
        }
        Error::Backend | Error::Health(device_health::Error::Backend) => Status::HardwareFailure,
        _ => Status::InvalidArgument,
    }
}
fn validate_declared(
    d: &device_health::ProductDeclaration<'_>,
    p: &device_health::Provenance<'_>,
    schema: u16,
    bytes: &[u8],
) -> Result<(), Error> {
    let (schema, bytes) = if schema == 2 && matches!(d.module.wire(), 0x25..=0x27) {
        let result = device_health::SourcedResult::decode(d.module.wire(), bytes)?;
        if result.origin().acquisition() != p.acquisition || result.origin().source() != p.source {
            return Err(Error::Invalid);
        }
        (1, result.value())
    } else {
        (schema, bytes)
    };
    device_health::validate_result_body(d.module.wire(), d.record_type, schema, bytes, p.source)?;
    match d.module.wire() {
        #[cfg(feature = "heart-rate")]
        0x20 => d.validate(&device_health::HeartRate::decode(bytes)?, p),
        #[cfg(feature = "blood-pressure")]
        0x25 => d.validate(&device_health::BloodPressure::decode(bytes)?, p),
        #[cfg(feature = "oxygen")]
        0x26 => d.validate(&device_health::Oxygen::decode(bytes)?, p),
        #[cfg(feature = "glucose")]
        0x27 => d.validate(&device_health::Glucose::decode(bytes)?, p),
        #[cfg(feature = "temperature")]
        0x28 => d.validate(&device_health::Temperature::decode(bytes)?, p),
        #[cfg(feature = "ecg")]
        0x29 => d.validate(&device_health::Ecg::decode(bytes)?, p),
        #[cfg(feature = "wellbeing")]
        0x2a if d.record_type == 0x2a01 && schema == 2 => {
            d.validate(&device_health::FatigueSnapshot::decode(bytes)?, p)
        }
        #[cfg(feature = "wellbeing")]
        0x2a => d.validate(&device_health::Wellbeing::decode(d.record_type, bytes)?, p),
        #[cfg(feature = "microcirculation")]
        0x2b => d.validate(&device_health::Alk::decode(bytes)?, p),
        _ => Err(device_health::Error::Unsupported),
    }
    .map_err(Error::from)
}

pub(crate) fn validate_start(product: &dyn HealthProduct, r: HealthStart) -> Result<(), Error> {
    let d = product.declaration(r.module().wire(), r.record_type())?;
    if d.module != r.module() || d.record_type != r.record_type() || !valid_declaration(&d) {
        return Err(Error::Invalid);
    }
    if d.acquisition == device_health::AcquisitionKind::PhoneImport
        || (r.module().wire() == 0x28 && (d.temperature_modes & (1 << (r.temp_mode() - 1)) == 0))
    {
        return Err(Error::Health(device_health::Error::Unsupported));
    }
    Ok(())
}

fn valid_declaration(d: &device_health::ProductDeclaration<'_>) -> bool {
    fn identity(s: &str) -> bool {
        !s.is_empty() && !s.as_bytes().contains(&0) && u16::try_from(s.len()).is_ok()
    }
    if !identity(d.algorithm_id) || !identity(d.algorithm_version) {
        return false;
    }
    if d.module.wire() == 0x28 {
        if d.temperature_modes == 0 || d.temperature_modes & !15 != 0 {
            return false;
        }
    } else if d.temperature_modes != 0 {
        return false;
    }
    if d.record_type == 0x2a03 {
        if d.mood_labels.is_empty() || u8::try_from(d.mood_labels.len()).is_err() {
            return false;
        }
        for (i, label) in d.mood_labels.iter().enumerate() {
            if !identity(label.label)
                || d.mood_labels[..i]
                    .iter()
                    .any(|old| old.value == label.value)
            {
                return false;
            }
        }
    } else if !d.mood_labels.is_empty() {
        return false;
    }
    true
}
