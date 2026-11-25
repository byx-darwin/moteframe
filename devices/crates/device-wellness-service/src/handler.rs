#[cfg(any(feature = "rri", feature = "uv"))]
use crate::Origin;
#[cfg(feature = "rri")]
use crate::RriView;
use crate::{
    Access, Backend, Clock, ClockReading, EventToken, Failure, ModuleStatus, OfferedEvent,
    Operation, Resource,
};
#[cfg(feature = "uv")]
use crate::{CancelTicket, UvEvent};
use core::cell::{Cell, RefCell};
use device_protocol::{Frame, Kind, ModuleEntry};
use device_services::{Reason, ServiceHandler, SessionError, Status};
#[cfg(feature = "rri")]
use device_wellness::{RriEvent, RriOrigin};
#[cfg(feature = "uv")]
use device_wellness::{UvCancelRequest, UvValueOrigin};
/// Actual device-owned port owner; App handlers never own acquisition lifecycle.
pub struct WellnessOwner<B> {
    backend: RefCell<B>,
    active: Cell<Option<u64>>,
    last: Cell<u64>,
}
impl<B> core::fmt::Debug for WellnessOwner<B> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("WellnessOwner").finish_non_exhaustive()
    }
}
impl<B: Backend> WellnessOwner<B> {
    /// Bind actual device task ports.
    #[must_use]
    pub fn new(backend: B) -> Self {
        Self {
            backend: RefCell::new(backend),
            active: Cell::new(None),
            last: Cell::new(0),
        }
    }
    /// Checked device task/recovery access; preserves the same tracker/engine.
    /// # Errors
    /// Reentrant work is Busy, closure errors propagate.
    pub fn with_backend<R>(
        &self,
        f: impl FnOnce(&mut B) -> Result<R, Failure>,
    ) -> Result<R, Failure> {
        let mut backend = self.backend.try_borrow_mut().map_err(|_| Failure::Busy)?;
        f(&mut backend)
    }
}
#[cfg(feature = "rri")]
#[derive(Clone, Copy, PartialEq, Eq)]
struct RriBinding {
    generation: u64,
    id: u64,
    parent: Option<u64>,
    origin: RriOrigin,
}
#[cfg(feature = "rri")]
impl RriBinding {
    fn resource(self) -> Resource {
        Resource::Measurement {
            id: self.id,
            parent: self.parent,
        }
    }
}
#[cfg(feature = "uv")]
#[derive(Clone, Copy, PartialEq, Eq)]
struct UvBinding {
    generation: u64,
    origin: UvValueOrigin,
}
#[cfg(feature = "uv")]
#[derive(Clone, Copy)]
struct Cancel {
    ticket: CancelTicket,
    binding: UvBinding,
    mid: u16,
}
#[cfg(feature = "uv")]
impl Cancel {
    fn matches(self, request: Frame<'_>, response: &[u8]) -> bool {
        request.header.module == 0x2e
            && request.header.opcode == 0x30
            && request.header.message_id == self.mid
            && request.header.kind == Kind::Request
            && request.header.offset == 0
            && request.header.total_length == 4
            && request.payload == self.ticket.event_id.to_le_bytes()
            && response == [0; 4]
    }
}
#[cfg(any(feature = "rri", feature = "uv"))]
#[derive(Clone, Copy, PartialEq, Eq)]
enum Payload {
    #[cfg(feature = "rri")]
    Rri(RriBinding, RriEvent),
    #[cfg(feature = "uv")]
    Uv(UvBinding, UvEvent),
}
#[cfg(any(feature = "rri", feature = "uv"))]
#[derive(Clone, Copy)]
struct Prepared {
    offer: OfferedEvent,
    payload: Payload,
    bytes: [u8; 25],
}
/// One App connection; exact business1.0 registrations only. Acquisition stays device-owned.
pub struct WellnessHandler<'a, B: Backend> {
    owner: &'a WellnessOwner<B>,
    connection: u64,
    entries: &'a [ModuleEntry],
    access: &'a dyn Access,
    clock: &'a dyn Clock,
    closed: Cell<bool>,
    detached: bool,
    last_now: Cell<u64>,
    #[cfg(feature = "rri")]
    rri: Cell<Option<RriBinding>>,
    #[cfg(feature = "rri")]
    rri_read: Cell<bool>,
    #[cfg(feature = "rri")]
    rri_events: Cell<bool>,
    #[cfg(feature = "uv")]
    uv: Cell<Option<UvBinding>>,
    #[cfg(feature = "uv")]
    uv_read: Cell<bool>,
    #[cfg(feature = "uv")]
    uv_events: Cell<bool>,
    #[cfg(feature = "uv")]
    cancel: Cell<Option<Cancel>>,
    #[cfg(feature = "uv")]
    completed_cancel: Option<Cancel>,
    #[cfg(any(feature = "rri", feature = "uv"))]
    pending: RefCell<Option<Prepared>>,
    #[cfg(any(feature = "rri", feature = "uv"))]
    previous: Option<EventToken>,
    #[cfg(any(feature = "rri", feature = "uv"))]
    next_serial: u64,
}
impl<B: Backend> core::fmt::Debug for WellnessHandler<'_, B> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("WellnessHandler")
            .field("closed", &self.closed.get())
            .finish_non_exhaustive()
    }
}
impl<'a, B: Backend> WellnessHandler<'a, B> {
    /// Bind selected ports without starting device acquisition. Identity is nonzero and nonreused.
    /// # Errors
    /// Invalid metadata/identity or actual port absence is explicit.
    pub fn new(
        owner: &'a WellnessOwner<B>,
        connection: u64,
        entries: &'a [ModuleEntry],
        access: &'a dyn Access,
        clock: &'a dyn Clock,
    ) -> Result<Self, Failure> {
        if connection == 0
            || connection <= owner.last.get()
            || entries.is_empty()
            || entries.len() > 3
        {
            return Err(Failure::Invalid);
        }
        if owner.active.get().is_some() {
            return Err(Failure::Busy);
        }
        let mut prior = 0;
        for entry in entries {
            if !compiled(entry.module)
                || entry.module <= prior
                || entry.major != 1
                || entry.minor != 0
                || entry.features != 0
                || entry.availability != 1
                || !(128..=512).contains(&entry.input_max)
                || !(128..=512).contains(&entry.output_max)
            {
                return Err(Failure::Invalid);
            }
            if !owner.with_backend(|backend| Ok(backend.selected(entry.module)))? {
                return Err(Failure::Unsupported);
            }
            #[cfg(feature = "uv")]
            if entry.module == 0x2e {
                owner.with_backend(|backend| uv_binding(backend.uv()?).map(|_| ()))?;
            }
            prior = entry.module;
        }
        let now = clock.now()?;
        owner.active.set(Some(connection));
        owner.last.set(connection);
        Ok(Self {
            owner,
            connection,
            entries,
            access,
            clock,
            closed: Cell::new(false),
            detached: false,
            last_now: Cell::new(now.monotonic_us),
            #[cfg(feature = "rri")]
            rri: Cell::new(None),
            #[cfg(feature = "rri")]
            rri_read: Cell::new(false),
            #[cfg(feature = "rri")]
            rri_events: Cell::new(false),
            #[cfg(feature = "uv")]
            uv: Cell::new(None),
            #[cfg(feature = "uv")]
            uv_read: Cell::new(false),
            #[cfg(feature = "uv")]
            uv_events: Cell::new(false),
            #[cfg(feature = "uv")]
            cancel: Cell::new(None),
            #[cfg(feature = "uv")]
            completed_cancel: None,
            #[cfg(any(feature = "rri", feature = "uv"))]
            pending: RefCell::new(None),
            #[cfg(any(feature = "rri", feature = "uv"))]
            previous: None,
            #[cfg(any(feature = "rri", feature = "uv"))]
            next_serial: 1,
        })
    }
    fn invalidate(&self) {
        self.closed.set(true);
        #[cfg(feature = "rri")]
        {
            self.rri.set(None);
            self.rri_read.set(false);
            self.rri_events.set(false);
        }
        #[cfg(feature = "uv")]
        {
            self.uv.set(None);
            self.uv_read.set(false);
            self.uv_events.set(false);
            self.cancel.set(None);
        }
        #[cfg(any(feature = "rri", feature = "uv"))]
        if let Ok(mut pending) = self.pending.try_borrow_mut() {
            if let Some(value) = pending.as_mut() {
                value.bytes.fill(0);
            }
            *pending = None;
        }
    }
    fn live(&self) -> bool {
        if self.closed.get() {
            return false;
        }
        #[cfg(feature = "uv")]
        if self.entry(0x2e).is_some()
            && self
                .owner
                .with_backend(|backend| uv_binding(backend.uv()?).map(|_| ()))
                .is_err()
        {
            self.invalidate();
            return false;
        }
        #[cfg(feature = "rri")]
        if let Some(binding) = self.rri.get() {
            let resource = binding.resource();
            if !self
                .access
                .source_allowed(0x2c, Origin::Rri(binding.origin))
                || self.rri_read.get() && !self.access.allowed(0x2c, Operation::Latest, resource)
                || self.rri_events.get()
                    && (!self.access.allowed(0x2c, Operation::Subscribe, resource)
                        || !self.access.allowed(0x2c, Operation::Event, resource))
            {
                self.invalidate();
                return false;
            }
        }
        #[cfg(feature = "uv")]
        if let Some(binding) = self.uv.get()
            && (!self
                .owner
                .with_backend(|backend| Ok(uv_binding(backend.uv()?)? == binding))
                .unwrap_or(false)
                || !self.access.source_allowed(0x2e, Origin::Uv(binding.origin))
                || self.uv_read.get()
                    && !self
                        .access
                        .allowed(0x2e, Operation::Latest, Resource::Module)
                || self.uv_events.get()
                    && (!self
                        .access
                        .allowed(0x2e, Operation::Subscribe, Resource::Module)
                        || !self
                            .access
                            .allowed(0x2e, Operation::Event, Resource::Module)))
        {
            self.invalidate();
            return false;
        }
        #[cfg(feature = "uv")]
        if let Some(cancel) = self.cancel.get()
            && !self.access.allowed(
                0x2e,
                Operation::Cancel,
                Resource::Alarm(cancel.ticket.event_id),
            )
        {
            self.invalidate();
            return false;
        }
        true
    }
    fn now(&self) -> Result<ClockReading, Failure> {
        if !self.live() {
            return Err(Failure::Closed);
        }
        let value = self.clock.now().map_err(|_| {
            self.invalidate();
            Failure::Clock
        })?;
        if value.monotonic_us < self.last_now.get() {
            self.invalidate();
            return Err(Failure::Clock);
        }
        self.last_now.set(value.monotonic_us);
        Ok(value)
    }
    fn allowed(&self, module: u8, op: Operation, resource: Resource) -> Result<(), Failure> {
        if !self.live() {
            return Err(Failure::Closed);
        }
        if self.access.allowed(module, op, resource) {
            Ok(())
        } else {
            Err(Failure::Denied)
        }
    }
    fn entry(&self, module: u8) -> Option<&ModuleEntry> {
        self.entries.iter().find(|entry| entry.module == module)
    }
    /// Subscribe only to a real existing validated source, never start one.
    /// # Errors
    /// No real source/role/capability is explicit.
    pub fn subscribe(&mut self, module: u8) -> Result<(), Failure> {
        let now = self.now()?;
        if self.entry(module).is_none() {
            return Err(Failure::Unsupported);
        }
        match module {
            #[cfg(feature = "rri")]
            0x2c => {
                let binding = self
                    .owner
                    .with_backend(|backend| rri_binding(backend.rri_view(self.connection)?, now))?;
                self.rri_permission(binding, Operation::Subscribe)?;
                self.rri_permission(binding, Operation::Event)?;
                self.rri.set(Some(binding));
                self.rri_events.set(true);
                Ok(())
            }
            #[cfg(feature = "uv")]
            0x2e => {
                let binding = self
                    .owner
                    .with_backend(|backend| uv_binding(backend.uv()?))?;
                self.uv_permission(binding, Operation::Subscribe, Resource::Module)?;
                self.uv_permission(binding, Operation::Event, Resource::Module)?;
                self.uv.set(Some(binding));
                self.uv_events.set(true);
                Ok(())
            }
            _ => {
                let _ = now;
                Err(Failure::Unsupported)
            }
        }
    }
    #[cfg(feature = "rri")]
    fn rri_permission(&self, binding: RriBinding, op: Operation) -> Result<(), Failure> {
        self.allowed(0x2c, op, binding.resource())?;
        if self
            .access
            .source_allowed(0x2c, Origin::Rri(binding.origin))
        {
            Ok(())
        } else {
            Err(Failure::Denied)
        }
    }
    #[cfg(feature = "uv")]
    fn uv_permission(
        &self,
        binding: UvBinding,
        op: Operation,
        resource: Resource,
    ) -> Result<(), Failure> {
        self.allowed(0x2e, op, resource)?;
        if self.access.source_allowed(0x2e, Origin::Uv(binding.origin)) {
            Ok(())
        } else {
            Err(Failure::Denied)
        }
    }
    /// Inspect routing without pinning/encoding/consuming an event, for first-queue reservation.
    /// # Errors
    /// Actual source/scope/clock failure remains explicit.
    pub fn event_route(&self) -> Result<Option<crate::EventRoute>, Failure> {
        if !self.live() {
            return Err(Failure::Closed);
        }
        #[cfg(any(feature = "rri", feature = "uv"))]
        {
            let now = self.clock.now()?;
            if now.monotonic_us < self.last_now.get() {
                return Err(Failure::Clock);
            }
            let pending = *self.pending.try_borrow().map_err(|_| Failure::Busy)?;
            if let Some(value) = pending {
                if !self.event_valid(value.offer.token) {
                    return Err(Failure::InvalidState);
                }
                return Ok(Some(crate::EventRoute {
                    module: value.offer.module,
                    opcode: value.offer.opcode,
                    length: value.offer.length,
                }));
            }
            self.find_event(now)?.map(payload_route).transpose()
        }
        #[cfg(not(any(feature = "rri", feature = "uv")))]
        Ok(None)
    }
    /// Prepare/retry exact typed actual event bytes. No status prefix or additional CRC.
    /// # Errors
    /// Missing scope/source/capacity or real backend failure.
    pub fn next_event(&mut self, out: &mut [u8]) -> Result<Option<OfferedEvent>, Failure> {
        let now = self.now().map_err(|_| {
            out.fill(0);
            Failure::Closed
        })?;
        #[cfg(any(feature = "rri", feature = "uv"))]
        {
            let pending = *self.pending.try_borrow().map_err(|_| Failure::Busy)?;
            if let Some(value) = pending {
                if !self.event_valid(value.offer.token) {
                    out.fill(0);
                    return Err(if self.closed.get() {
                        Failure::Closed
                    } else {
                        Failure::InvalidState
                    });
                }
                if out.len() < value.offer.length {
                    return Err(Failure::Capacity);
                }
                out[..value.offer.length].copy_from_slice(&value.bytes[..value.offer.length]);
                return Ok(Some(value.offer));
            }
            let Some(payload) = self.find_event(now)? else {
                return Ok(None);
            };
            let mut bytes = [0; 25];
            let (module, opcode, length) = encode_event(payload, &mut bytes)?;
            if out.len() < length {
                return Err(Failure::Capacity);
            }
            let serial = self.next_serial;
            self.next_serial = serial.checked_add(1).ok_or(Failure::Capacity)?;
            let offer = OfferedEvent {
                token: EventToken {
                    connection: self.connection,
                    serial,
                    module,
                    opcode,
                },
                module,
                opcode,
                length,
            };
            out[..length].copy_from_slice(&bytes[..length]);
            *self.pending.try_borrow_mut().map_err(|_| Failure::Busy)? = Some(Prepared {
                offer,
                payload,
                bytes,
            });
            Ok(Some(offer))
        }
        #[cfg(not(any(feature = "rri", feature = "uv")))]
        {
            let _ = (now, out);
            Ok(None)
        }
    }
    #[cfg(any(feature = "rri", feature = "uv"))]
    fn find_event(&self, now: ClockReading) -> Result<Option<Payload>, Failure> {
        #[cfg(feature = "rri")]
        if self.rri_events.get()
            && let Some(binding) = self.rri.get()
        {
            let event = self.owner.with_backend(|backend| {
                let view = backend.rri_view(self.connection)?;
                if rri_binding(view, now)? != binding {
                    return Err(Failure::Conflict);
                }
                Ok(view.tracker.pending())
            })?;
            if let Some(value) = event {
                self.rri_permission(binding, Operation::Event)?;
                return Ok(Some(Payload::Rri(binding, value)));
            }
        }
        #[cfg(feature = "uv")]
        if self.uv_events.get()
            && let Some(binding) = self.uv.get()
        {
            let event = self.owner.with_backend(|backend| {
                let port = backend.uv()?;
                if uv_binding(port)? != binding {
                    return Err(Failure::Conflict);
                }
                Ok(port
                    .state()
                    .pending_report()
                    .map(UvEvent::Report)
                    .or_else(|| port.state().pending_alarm().map(UvEvent::Alarm)))
            })?;
            if let Some(value) = event {
                let resource = uv_event_resource(value);
                self.uv_permission(binding, Operation::Event, resource)?;
                return Ok(Some(Payload::Uv(binding, value)));
            }
        }
        let _ = now;
        Ok(None)
    }
    /// Pure exact event/source/scope check before each fragment admission.
    #[must_use]
    pub fn event_valid(&self, token: EventToken) -> bool {
        if !self.live() || token.connection != self.connection {
            return false;
        }
        #[cfg(any(feature = "rri", feature = "uv"))]
        {
            let Ok(pending) = self.pending.try_borrow() else {
                return false;
            };
            let Some(value) = *pending else { return false };
            if value.offer.token != token {
                return false;
            }
            self.payload_current(value.payload).unwrap_or(false)
        }
        #[cfg(not(any(feature = "rri", feature = "uv")))]
        false
    }
    #[cfg(any(feature = "rri", feature = "uv"))]
    fn payload_current(&self, payload: Payload) -> Result<bool, Failure> {
        match payload {
            #[cfg(feature = "rri")]
            Payload::Rri(binding, event) => {
                self.rri_permission(binding, Operation::Event)?;
                self.owner.with_backend(|backend| {
                    let view = backend.rri_view(self.connection)?;
                    Ok(view.generation == binding.generation
                        && view.origin == binding.origin
                        && view.tracker.parent_sport() == binding.parent
                        && view.tracker.pending() == Some(event))
                })
            }
            #[cfg(feature = "uv")]
            Payload::Uv(binding, event) => {
                if self
                    .uv_permission(binding, Operation::Event, uv_event_resource(event))
                    .is_err()
                {
                    self.invalidate();
                    return Ok(false);
                }
                self.owner.with_backend(|backend| {
                    let port = backend.uv()?;
                    Ok(uv_binding(port)? == binding
                        && match event {
                            UvEvent::Report(value) => port.state().pending_report() == Some(value),
                            UvEvent::Alarm(value) => port.state().pending_alarm() == Some(value),
                        })
                })
            }
        }
    }
    /// Confirm after real complete queue admission, never phone persistence.
    /// # Errors
    /// Wrong/stale source or actual backend failure retains bytes.
    pub fn event_admitted(&mut self, token: EventToken) -> Result<(), Failure> {
        if !self.live() {
            return Err(Failure::Closed);
        }
        #[cfg(any(feature = "rri", feature = "uv"))]
        {
            if self.previous == Some(token)
                && self
                    .pending
                    .try_borrow()
                    .map_err(|_| Failure::Busy)?
                    .is_none()
            {
                return Ok(());
            }
            if !self.event_valid(token) {
                return Err(Failure::InvalidState);
            }
            let value = (*self.pending.try_borrow().map_err(|_| Failure::Busy)?)
                .ok_or(Failure::InvalidState)?;
            match value.payload {
                #[cfg(feature = "rri")]
                Payload::Rri(binding, event) => self.owner.with_backend(|backend| {
                    backend.rri_event_admitted(
                        self.connection,
                        binding.generation,
                        event.measurement,
                        event.index,
                    )
                })?,
                #[cfg(feature = "uv")]
                Payload::Uv(_, event) => self
                    .owner
                    .with_backend(|backend| backend.uv_mut()?.event_admitted(event))?,
            }
            *self.pending.try_borrow_mut().map_err(|_| Failure::Busy)? = None;
            self.previous = Some(token);
            Ok(())
        }
        #[cfg(not(any(feature = "rri", feature = "uv")))]
        {
            let _ = token;
            Err(Failure::Unsupported)
        }
    }
    fn execute(&mut self, request: Frame<'_>, out: &mut [u8]) -> Result<usize, Failure> {
        let now = self.now()?;
        #[cfg(not(any(feature = "rri", feature = "uv")))]
        let _ = now;
        let module = request.header.module;
        let (operation, resource) = scope(request);
        self.allowed(module, operation, resource)?;
        if request.header.opcode == 1 {
            if !request.payload.is_empty() {
                return Err(Failure::Invalid);
            }
            let value = self.module_status(module)?;
            out[4] = value.state;
            out[5..7].copy_from_slice(&value.reason.to_le_bytes());
            return Ok(7);
        }
        if request.header.opcode == 2 {
            if !request.payload.is_empty() {
                return Err(Failure::Invalid);
            }
            #[cfg(any(feature = "rri", feature = "uv"))]
            return self.latest(module, now, out);
            #[cfg(not(any(feature = "rri", feature = "uv")))]
            return Err(Failure::Unsupported);
        }
        #[cfg(feature = "uv")]
        if module == 0x2e && request.header.opcode == 0x30 {
            return self.cancel_prepare(request);
        }
        Err(Failure::Unsupported)
    }
    fn module_status(&self, module: u8) -> Result<ModuleStatus, Failure> {
        self.owner.with_backend(|backend| {
            let status = backend.status(module)?;
            if status.state > 3 {
                return Err(Failure::Contract);
            }
            #[cfg(feature = "womens-health")]
            if module == 0x2d && matches!(status.state, 1 | 2) {
                let port = backend.womens()?;
                if !port.available() {
                    return Err(Failure::Unsupported);
                }
                let config = port
                    .load()
                    .map_err(|_| Failure::Storage)?
                    .ok_or(Failure::NotFound)?;
                config.validate().map_err(|_| Failure::Contract)?;
                if status.state == 2 && !config.enabled {
                    return Err(Failure::Contract);
                }
            }
            Ok(status)
        })
    }
    #[cfg(any(feature = "rri", feature = "uv"))]
    fn latest(&mut self, module: u8, now: ClockReading, out: &mut [u8]) -> Result<usize, Failure> {
        match module {
            #[cfg(feature = "rri")]
            0x2c => {
                let (binding, timestamp, summary) = self.owner.with_backend(|backend| {
                    let view = backend.rri_view(self.connection)?;
                    let binding = rri_binding(RriView { ..view }, now)?;
                    let summary = view
                        .tracker
                        .summary_at(now.monotonic_us)
                        .map_err(wellness_failure)?;
                    Ok((binding, view.timestamp, summary))
                })?;
                self.rri_permission(binding, Operation::Latest)?;
                latest_header(&mut out[4..], 0x2c01, timestamp, 16);
                summary
                    .encode(&mut out[19..])
                    .map_err(|_| Failure::Contract)?;
                self.rri.set(Some(binding));
                self.rri_read.set(true);
                if self
                    .access
                    .allowed(module, Operation::Subscribe, binding.resource())
                    && self
                        .access
                        .allowed(module, Operation::Event, binding.resource())
                {
                    self.rri_events.set(true);
                }
                Ok(35)
            }
            #[cfg(feature = "uv")]
            0x2e => {
                let (binding, sample) = self.owner.with_backend(|backend| {
                    let port = backend.uv()?;
                    Ok((uv_binding(port)?, port.state().latest()?))
                })?;
                if sample.sample_id == 0 || !sample.reading.has_value() {
                    return Err(Failure::NotFound);
                }
                self.uv_permission(binding, Operation::Latest, Resource::Module)?;
                latest_header(&mut out[4..], 0x2e01, sample.timestamp, 7);
                sample
                    .reading
                    .encode(&mut out[19..])
                    .map_err(|_| Failure::Contract)?;
                self.uv.set(Some(binding));
                self.uv_read.set(true);
                if self
                    .access
                    .allowed(module, Operation::Subscribe, Resource::Module)
                    && self
                        .access
                        .allowed(module, Operation::Event, Resource::Module)
                {
                    self.uv_events.set(true);
                }
                Ok(26)
            }
            _ => {
                let _ = (now, out);
                Err(Failure::Unsupported)
            }
        }
    }
    #[cfg(feature = "uv")]
    fn cancel_prepare(&mut self, request: Frame<'_>) -> Result<usize, Failure> {
        let id = UvCancelRequest::decode(request.payload)
            .map_err(|_| Failure::Invalid)?
            .event_id;
        if self.cancel.get().is_some() {
            return Err(Failure::Busy);
        }
        let (binding, ticket) = self.owner.with_backend(|backend| {
            let port = backend.uv()?;
            let binding = uv_binding(port)?;
            let ticket = port.validate_cancel(id)?;
            if ticket.event_id != id
                || ticket.generation != binding.generation
                || ticket.revision == 0
                || !port.ticket_current(ticket)
            {
                return Err(Failure::Contract);
            }
            Ok((binding, ticket))
        })?;
        self.uv_permission(binding, Operation::Cancel, Resource::Alarm(id))?;
        self.uv.set(Some(binding));
        self.cancel.set(Some(Cancel {
            ticket,
            binding,
            mid: request.header.message_id,
        }));
        if self
            .access
            .allowed(0x2e, Operation::Subscribe, Resource::Module)
            && self
                .access
                .allowed(0x2e, Operation::Event, Resource::Module)
        {
            self.uv_events.set(true);
        }
        Ok(4)
    }
    fn disconnect(&mut self) -> Result<(), Failure> {
        self.invalidate();
        #[cfg(feature = "uv")]
        {
            self.completed_cancel = None;
        }
        if !self.detached {
            self.owner
                .with_backend(|backend| backend.detach(self.connection))?;
            self.detached = true;
            if self.owner.active.get() == Some(self.connection) {
                self.owner.active.set(None);
            }
        }
        Ok(())
    }
}
impl<B: Backend> ServiceHandler for WellnessHandler<'_, B> {
    fn required_features(&self, module: &ModuleEntry, opcode: u8) -> Option<u32> {
        (self.entry(module.module) == Some(module) && supported(module.module, opcode)).then_some(0)
    }
    fn authorized(&self, request: Frame<'_>) -> bool {
        if !self.live() {
            return false;
        }
        let (op, resource) = scope(request);
        self.access.allowed(request.header.module, op, resource)
    }
    fn must_end_session(&self) -> bool {
        !self.live()
    }
    fn replay_valid(&self, request: Frame<'_>, response: &[u8]) -> bool {
        if !self.authorized(request) {
            return false;
        }
        #[cfg(feature = "uv")]
        if request.header.module == 0x2e && request.header.opcode == 0x30 && response == [0; 4] {
            if let Some(cancel) = self.cancel.get() {
                if !cancel.matches(request, response) {
                    return false;
                }
                return self
                    .owner
                    .with_backend(|backend| {
                        let port = backend.uv()?;
                        Ok(uv_binding(port)? == cancel.binding
                            && port.ticket_current(cancel.ticket))
                    })
                    .unwrap_or(false);
            }
            if let Some(cancel) = self.completed_cancel {
                if !cancel.matches(request, response) {
                    return false;
                }
                return self
                    .owner
                    .with_backend(|backend| {
                        let port = backend.uv()?;
                        if uv_binding(port)? != cancel.binding {
                            return Ok(false);
                        }
                        Ok(port
                            .validate_cancel(cancel.ticket.event_id)?
                            .already_cancelled)
                    })
                    .unwrap_or(false);
            }
            return false;
        }
        #[cfg(feature = "rri")]
        if request.header.module == 0x2c
            && request.header.opcode == 2
            && response.len() == 35
            && response[..4] == [0; 4]
        {
            return self.rri.get().is_some_and(|binding| {
                response[19..27] == binding.id.to_le_bytes()
                    && self.rri_permission(binding, Operation::Latest).is_ok()
            });
        }
        let _ = response;
        true
    }
    fn dispatch(&mut self, request: Frame<'_>, out: &mut [u8]) -> Result<usize, SessionError> {
        if !self.live() {
            out.fill(0);
            return Err(SessionError::AuthorizationRevoked);
        }
        if out.len() < 4 {
            return Err(SessionError::Capacity);
        }
        if request.header.kind != Kind::Request
            || request.header.offset != 0
            || request.header.message_id == 0
            || usize::from(request.header.total_length) != request.payload.len()
        {
            return Ok(error_response(out, Failure::Invalid));
        }
        let Some(entry) = self.entry(request.header.module) else {
            return Ok(error_response(out, Failure::Unsupported));
        };
        if !supported(entry.module, request.header.opcode) {
            return Ok(error_response(out, Failure::Unsupported));
        }
        let capacity = match request.header.opcode {
            1 => 7,
            2 if entry.module == 0x2c => 35,
            2 => 26,
            _ => 4,
        };
        if out.len() < capacity || capacity > usize::from(entry.output_max) {
            return Err(SessionError::Capacity);
        }
        if request.payload.len() > usize::from(entry.input_max) {
            return Ok(error_response(out, Failure::Invalid));
        }
        out[..4].fill(0);
        let prior = self.authorized(request);
        let result = self.execute(request, out);
        if !self.live()
            || matches!(result, Err(Failure::Closed | Failure::Clock))
            || prior && !self.authorized(request)
        {
            self.invalidate();
            out.fill(0);
            return Err(SessionError::AuthorizationRevoked);
        }
        match result {
            Ok(length) => Ok(length),
            Err(error) => Ok(error_response(out, error)),
        }
    }
    fn response_admitted(
        &mut self,
        request: Frame<'_>,
        response: &[u8],
    ) -> Result<(), SessionError> {
        if !self.live() || !self.authorized(request) {
            self.invalidate();
            return Err(SessionError::AuthorizationRevoked);
        }
        #[cfg(feature = "uv")]
        {
            if let Some(cancel) = self.cancel.get()
                && cancel.matches(request, response)
            {
                let result = self.apply_cancel(cancel);
                if result.is_err() {
                    self.invalidate();
                    return Err(
                        if result == Err(Failure::Denied) || result == Err(Failure::Closed) {
                            SessionError::AuthorizationRevoked
                        } else {
                            SessionError::InvalidHandlerResponse
                        },
                    );
                }
                self.cancel.set(None);
                self.completed_cancel = Some(cancel);
            }
        }
        let _ = response;
        Ok(())
    }
    fn connection_closed(&mut self) -> Result<(), SessionError> {
        self.disconnect()
            .map_err(|_| SessionError::InvalidHandlerResponse)
    }
}
#[cfg(feature = "uv")]
impl<B: Backend> WellnessHandler<'_, B> {
    fn apply_cancel(&self, cancel: Cancel) -> Result<(), Failure> {
        let when = self.now()?;
        self.uv_permission(
            cancel.binding,
            Operation::Cancel,
            Resource::Alarm(cancel.ticket.event_id),
        )?;
        self.owner.with_backend(|backend| {
            let port = backend.uv_mut()?;
            if uv_binding(port)? != cancel.binding || !port.ticket_current(cancel.ticket) {
                return Err(Failure::Conflict);
            }
            port.cancel_atomic(self.connection, cancel.ticket, when, self.access)?;
            if uv_binding(port)? != cancel.binding
                || !port
                    .validate_cancel(cancel.ticket.event_id)?
                    .already_cancelled
            {
                return Err(Failure::Contract);
            }
            if !cancel.ticket.already_cancelled {
                let event = port.state().pending_alarm().ok_or(Failure::Contract)?;
                if event.event_id != cancel.ticket.event_id
                    || event.source != device_wellness::UvSource::AppRequest
                    || event.transition != device_wellness::AlarmTransition::AppCancel
                    || event.timestamp != when.timestamp
                {
                    return Err(Failure::Contract);
                }
                event.validate().map_err(|_| Failure::Contract)?;
                if !event.reading.has_value() {
                    return Err(Failure::Contract);
                }
            }
            if !self.access.allowed(
                0x2e,
                Operation::Cancel,
                Resource::Alarm(cancel.ticket.event_id),
            ) {
                return Err(Failure::Denied);
            }
            Ok(())
        })
    }
}
fn compiled(module: u8) -> bool {
    match module {
        #[cfg(feature = "rri")]
        0x2c => true,
        #[cfg(feature = "womens-health")]
        0x2d => true,
        #[cfg(feature = "uv")]
        0x2e => true,
        _ => false,
    }
}
fn supported(module: u8, opcode: u8) -> bool {
    compiled(module)
        && (opcode == 1 || opcode == 2 && module != 0x2d || module == 0x2e && opcode == 0x30)
}
fn scope(request: Frame<'_>) -> (Operation, Resource) {
    match request.header.opcode {
        2 => (Operation::Latest, Resource::Module),
        0x30 => (
            Operation::Cancel,
            if request.payload.len() == 4 {
                Resource::Alarm(u32::from_le_bytes([
                    request.payload[0],
                    request.payload[1],
                    request.payload[2],
                    request.payload[3],
                ]))
            } else {
                Resource::Module
            },
        ),
        _ => (Operation::Status, Resource::Module),
    }
}
#[cfg(feature = "rri")]
fn rri_binding(view: RriView<'_>, now: ClockReading) -> Result<RriBinding, Failure> {
    if view.generation == 0
        || view.provider_id.is_empty()
        || view.provider_version.is_empty()
        || view.provider_id.as_bytes().contains(&0)
        || view.provider_version.as_bytes().contains(&0)
    {
        return Err(Failure::Contract);
    }
    let summary = view
        .tracker
        .summary_at(now.monotonic_us)
        .map_err(wellness_failure)?;
    summary.validate().map_err(|_| Failure::Contract)?;
    Ok(RriBinding {
        generation: view.generation,
        id: summary.measurement,
        parent: view.tracker.parent_sport(),
        origin: view.origin,
    })
}
#[cfg(feature = "uv")]
fn uv_binding(port: &dyn crate::UvControlPort) -> Result<UvBinding, Failure> {
    if port.identity_policy() != crate::UvIdentityPolicy::PersistentNonReuse {
        return Err(Failure::Unavailable);
    }
    let provenance = port.provenance();
    provenance.validate().map_err(|_| Failure::Contract)?;
    if port.generation() == 0 {
        return Err(Failure::Contract);
    }
    Ok(UvBinding {
        generation: port.generation(),
        origin: provenance.origin,
    })
}
#[cfg(feature = "uv")]
fn uv_event_resource(event: UvEvent) -> Resource {
    match event {
        UvEvent::Report(_) => Resource::Module,
        UvEvent::Alarm(value) => Resource::Alarm(value.event_id),
    }
}
#[cfg(any(feature = "rri", feature = "uv"))]
fn latest_header(out: &mut [u8], record: u16, timestamp: device_wellness::Timestamp, payload: u16) {
    out[..2].copy_from_slice(&record.to_le_bytes());
    out[2..4].copy_from_slice(&1u16.to_le_bytes());
    out[4..12].copy_from_slice(&timestamp.utc_ms().to_le_bytes());
    out[12] = u8::from(timestamp.is_valid());
    out[13..15].copy_from_slice(&payload.to_le_bytes());
}
#[cfg(any(feature = "rri", feature = "uv"))]
fn encode_event(payload: Payload, out: &mut [u8; 25]) -> Result<(u8, u8, usize), Failure> {
    match payload {
        #[cfg(feature = "rri")]
        Payload::Rri(_, value) => {
            let length = value.encode(out).map_err(|_| Failure::Contract)?;
            Ok((0x2c, 0x80, length))
        }
        #[cfg(feature = "uv")]
        Payload::Uv(_, UvEvent::Report(value)) => {
            if !value.reading.has_value() {
                return Err(Failure::Contract);
            }
            let length = value.encode(out).map_err(|_| Failure::Contract)?;
            Ok((0x2e, 0x80, length))
        }
        #[cfg(feature = "uv")]
        Payload::Uv(_, UvEvent::Alarm(value)) => {
            if !value.reading.has_value() {
                return Err(Failure::Contract);
            }
            let length = value.encode(out).map_err(|_| Failure::Contract)?;
            Ok((0x2e, 0x81, length))
        }
    }
}
#[cfg(feature = "rri")]
fn wellness_failure(error: device_wellness::Error) -> Failure {
    match error {
        device_wellness::Error::NotFound => Failure::NotFound,
        device_wellness::Error::Unavailable | device_wellness::Error::Unsupported => {
            Failure::Unsupported
        }
        device_wellness::Error::Busy => Failure::Busy,
        device_wellness::Error::Capacity | device_wellness::Error::Exhausted => Failure::Capacity,
        device_wellness::Error::Time => Failure::Clock,
        device_wellness::Error::Backend => Failure::Backend,
        _ => Failure::Contract,
    }
}
fn error_response(out: &mut [u8], error: Failure) -> usize {
    let (status, reason) = match error {
        Failure::Invalid => (Status::InvalidArgument, Reason::None),
        Failure::Unsupported => (Status::UnsupportedOpcode, Reason::None),
        Failure::NotFound => (Status::NotFound, Reason::None),
        Failure::Unavailable | Failure::InvalidState | Failure::Closed => {
            (Status::InvalidState, Reason::None)
        }
        Failure::Busy => (Status::Busy, Reason::None),
        Failure::Conflict => (Status::Conflict, Reason::None),
        Failure::Capacity => (Status::LimitExceeded, Reason::None),
        Failure::Denied => (Status::AccessDenied, Reason::RoleRequired),
        Failure::Storage => (Status::StorageFailure, Reason::None),
        Failure::Backend | Failure::Clock => (Status::HardwareFailure, Reason::None),
        Failure::Contract => (Status::InternalFailure, Reason::None),
    };
    out[..2].copy_from_slice(&(status as u16).to_le_bytes());
    out[2..4].copy_from_slice(&(reason as u16).to_le_bytes());
    4
}

#[cfg(any(feature = "rri", feature = "uv"))]
fn payload_route(payload: Payload) -> Result<crate::EventRoute, Failure> {
    let (module, opcode, length) = match payload {
        #[cfg(feature = "rri")]
        Payload::Rri(_, event) => {
            event.validate().map_err(|_| Failure::Contract)?;
            (0x2c, 0x80, 25)
        }
        #[cfg(feature = "uv")]
        Payload::Uv(_, UvEvent::Report(event)) => {
            if event.event_id == 0 || !event.reading.has_value() {
                return Err(Failure::Contract);
            }
            (0x2e, 0x80, 21)
        }
        #[cfg(feature = "uv")]
        Payload::Uv(_, UvEvent::Alarm(event)) => {
            event.validate().map_err(|_| Failure::Contract)?;
            if !event.reading.has_value() {
                return Err(Failure::Contract);
            }
            (0x2e, 0x81, 22)
        }
    };
    Ok(crate::EventRoute {
        module,
        opcode,
        length,
    })
}
