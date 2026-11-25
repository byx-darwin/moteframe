#[cfg(feature = "wear")]
use crate::WearKind;
#[cfg(feature = "sport")]
use crate::sport_control::{SportControl, SportRetention};
use crate::{
    EventBody, EventToken, Failure, LatestSelector, MotionAccess, MotionBackend, MotionClock,
    Observation, OfferedEvent, Operation, Resource, Timestamp,
};
#[cfg(feature = "sport")]
use crate::{LegacySportSummary, Snapshot};
#[cfg(feature = "sleep")]
use crate::{SleepSummary, SleepWindow};
use core::cell::{Cell, RefCell};
#[cfg(feature = "sport")]
use device_motion::{ContextAliases, MotionError, SportEngine, SportState};
use device_protocol::{Frame, Kind, ModuleEntry};
use device_services::{Reason, ServiceHandler, SessionError, Status};

struct State<B> {
    backend: B,
    #[cfg(feature = "sport")]
    engine: SportEngine,
    #[cfg(feature = "sport")]
    retention: SportRetention,
}
/// Boot-global actual backend/engine owner, shared with device UI work.
pub struct MotionOwner<B> {
    state: RefCell<State<B>>,
    active: Cell<Option<u64>>,
    last: Cell<u64>,
}
impl<B> core::fmt::Debug for MotionOwner<B> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MotionOwner").finish_non_exhaustive()
    }
}
impl<B: MotionBackend> MotionOwner<B> {
    #[cfg(feature = "sport")]
    /// Bind externally seeded boot-global engine and real resource backend.
    #[must_use]
    pub fn new(engine: SportEngine, backend: B) -> Self {
        Self {
            state: RefCell::new(State {
                backend,
                engine,
                retention: SportRetention::new(),
            }),
            active: Cell::new(None),
            last: Cell::new(0),
        }
    }
    #[cfg(not(feature = "sport"))]
    /// Bind real observation backends without compiling sport code.
    #[must_use]
    pub fn new(backend: B) -> Self {
        Self {
            state: RefCell::new(State { backend }),
            active: Cell::new(None),
            last: Cell::new(0),
        }
    }
    /// Checked device-owned backend access; no alternate owner may reset live IDs.
    /// # Errors
    /// Reentrant access is Busy; actual closure failure propagates.
    pub fn with_backend<R>(
        &self,
        f: impl FnOnce(&mut B) -> Result<R, Failure>,
    ) -> Result<R, Failure> {
        let mut state = self.state.try_borrow_mut().map_err(|_| Failure::Busy)?;
        f(&mut state.backend)
    }
    #[cfg(feature = "sport")]
    /// Checked device UI/recovery access preserving the same stable engine.
    /// # Errors
    /// Reentrant access is Busy; actual closure failure propagates.
    pub fn with_sport<R>(
        &self,
        f: impl FnOnce(&mut SportControl<'_, B>) -> Result<R, Failure>,
    ) -> Result<R, Failure> {
        let mut state = self.state.try_borrow_mut().map_err(|_| Failure::Busy)?;
        let State {
            engine,
            backend,
            retention,
        } = &mut *state;
        f(&mut SportControl::new(
            engine,
            backend,
            retention,
            self.active.get().is_some(),
        ))
    }
}
#[derive(Clone, Copy)]
struct Watch {
    module: u8,
    resource: Resource,
    operation: Operation,
    after: u64,
    events: bool,
}
#[cfg(feature = "sport")]
#[derive(Clone, Copy)]
struct Gate {
    opcode: u8,
    mid: u16,
    request: [u8; 9],
    request_len: usize,
    response: [u8; 12],
    response_len: usize,
}
#[cfg(feature = "sport")]
impl Gate {
    fn new(frame: Frame<'_>, response: &[u8]) -> Self {
        let mut gate = Self {
            opcode: frame.header.opcode,
            mid: frame.header.message_id,
            request: [0; 9],
            request_len: frame.payload.len(),
            response: [0; 12],
            response_len: response.len(),
        };
        gate.request[..frame.payload.len()].copy_from_slice(frame.payload);
        gate.response[..response.len()].copy_from_slice(response);
        gate
    }
    fn matches(self, request: Frame<'_>, response: &[u8]) -> bool {
        request.header.module == 0x22
            && request.header.opcode == self.opcode
            && request.header.message_id == self.mid
            && request.payload == &self.request[..self.request_len]
            && response == &self.response[..self.response_len]
    }
}
/// One App connection's motion wire handler. Device-owned sport lives in its owner.
pub struct MotionHandler<'a, B: MotionBackend, const N: usize> {
    owner: &'a MotionOwner<B>,
    entries: &'a [ModuleEntry],
    access: &'a dyn MotionAccess,
    clock: &'a dyn MotionClock,
    connection: u64,
    watches: [Cell<Option<Watch>>; N],
    closed: Cell<bool>,
    detached: bool,
    last_now: Cell<Option<u64>>,
    pending: Cell<Option<OfferedEvent>>,
    pending_resource: Cell<Option<Resource>>,
    previous: Option<EventToken>,
    event_bytes: RefCell<[u8; 512]>,
    cursor: usize,
    #[cfg(feature = "sport")]
    aliases: RefCell<ContextAliases<N>>,
    #[cfg(feature = "sport")]
    contexts: [Cell<Option<u64>>; N],
    #[cfg(feature = "sport")]
    stopped_watches: [Cell<Option<Watch>>; crate::SPORT_PROOF_CAPACITY],
    #[cfg(feature = "sport")]
    start_gate: Option<Gate>,
    #[cfg(feature = "sport")]
    stop_gate: Option<Gate>,
}
impl<B: MotionBackend, const N: usize> core::fmt::Debug for MotionHandler<'_, B, N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MotionHandler")
            .field("closed", &self.closed.get())
            .finish_non_exhaustive()
    }
}
impl<'a, B: MotionBackend, const N: usize> MotionHandler<'a, B, N> {
    /// Bind actual business1.0 registrations and one nonreused connection identity.
    /// # Errors
    /// Invalid registration, missing backend or existing App ownership is explicit.
    pub fn new(
        owner: &'a MotionOwner<B>,
        connection: u64,
        entries: &'a [ModuleEntry],
        access: &'a dyn MotionAccess,
        clock: &'a dyn MotionClock,
    ) -> Result<Self, Failure> {
        if N == 0
            || N > 32
            || connection == 0
            || connection <= owner.last.get()
            || entries.is_empty()
            || entries.len() > N
        {
            return Err(Failure::Invalid);
        }
        if owner.active.get().is_some() {
            return Err(Failure::Busy);
        }
        let mut previous = 0;
        for entry in entries {
            if !compiled(entry.module)
                || entry.module <= previous
                || entry.major != 1
                || entry.minor != 0
                || entry.availability != 1
                || entry.features != 0
                || !(128..=512).contains(&entry.input_max)
                || !(128..=512).contains(&entry.output_max)
            {
                return Err(Failure::Invalid);
            }
            if !owner.with_backend(|backend| Ok(backend.selected(entry.module)))? {
                return Err(Failure::Unsupported);
            }
            previous = entry.module;
        }
        let now = clock.now_ms()?;
        owner.active.set(Some(connection));
        owner.last.set(connection);
        Ok(Self {
            owner,
            entries,
            access,
            clock,
            connection,
            watches: core::array::from_fn(|_| Cell::new(None)),
            closed: Cell::new(false),
            detached: false,
            last_now: Cell::new(Some(now)),
            pending: Cell::new(None),
            pending_resource: Cell::new(None),
            previous: None,
            event_bytes: RefCell::new([0; 512]),
            cursor: 0,
            #[cfg(feature = "sport")]
            aliases: RefCell::new(ContextAliases::new()),
            #[cfg(feature = "sport")]
            contexts: core::array::from_fn(|_| Cell::new(None)),
            #[cfg(feature = "sport")]
            stopped_watches: core::array::from_fn(|_| Cell::new(None)),
            #[cfg(feature = "sport")]
            start_gate: None,
            #[cfg(feature = "sport")]
            stop_gate: None,
        })
    }
    fn invalidate(&self) {
        self.closed.set(true);
        self.pending.set(None);
        self.pending_resource.set(None);
        for watch in &self.watches {
            watch.set(None);
        }
        if let Ok(mut bytes) = self.event_bytes.try_borrow_mut() {
            bytes.fill(0);
        }
        #[cfg(feature = "sport")]
        for watch in &self.stopped_watches {
            watch.set(None);
        }
        #[cfg(feature = "sport")]
        if let Ok(mut aliases) = self.aliases.try_borrow_mut() {
            aliases.disconnect();
            for context in &self.contexts {
                context.set(None);
            }
        }
    }
    #[cfg(feature = "sport")]
    fn prune_contexts(&self) -> Result<(), Failure> {
        let state = self.owner.state.try_borrow().map_err(|_| Failure::Busy)?;
        let mut aliases = self.aliases.try_borrow_mut().map_err(|_| Failure::Busy)?;
        for context in &self.contexts {
            if let Some(id) = context.get()
                && matches!(
                    state
                        .engine
                        .session(id)
                        .and_then(device_motion::SportSession::state),
                    Ok(SportState::Stopped) | Err(MotionError::NotFound)
                )
            {
                aliases.session_ended(id);
                context.set(None);
            }
        }
        Ok(())
    }
    fn live(&self) -> bool {
        if self.closed.get() {
            return false;
        }
        #[cfg(feature = "sport")]
        if self.prune_contexts().is_err() {
            return false;
        }
        let allowed = self.watches.iter().filter_map(Cell::get).all(|watch| {
            self.access
                .allowed(watch.module, watch.operation, watch.resource)
                && (!watch.events
                    || self.access.allowed(
                        watch.module,
                        Operation::Event,
                        event_resource(watch.resource),
                    ))
        });
        #[cfg(feature = "sport")]
        let allowed = allowed
            && self
                .stopped_watches
                .iter()
                .filter_map(Cell::get)
                .all(|watch| {
                    self.access
                        .allowed(watch.module, watch.operation, watch.resource)
                        && (!watch.events
                            || self
                                .access
                                .allowed(watch.module, Operation::Event, watch.resource))
                })
            && self.contexts.iter().filter_map(Cell::get).all(|id| {
                self.access
                    .allowed(0x22, Operation::Context, Resource::Session(id))
            });
        if !allowed {
            self.invalidate();
        }
        allowed
    }
    fn now(&self) -> Result<u64, Failure> {
        if !self.live() {
            return Err(Failure::Closed);
        }
        let now = self.clock.now_ms().map_err(|_| {
            self.invalidate();
            Failure::Clock
        })?;
        if self.last_now.get().is_some_and(|last| now < last) {
            self.invalidate();
            return Err(Failure::Clock);
        }
        self.last_now.set(Some(now));
        Ok(now)
    }
    fn permission(
        &self,
        module: u8,
        operation: Operation,
        resource: Resource,
    ) -> Result<(), Failure> {
        if !self.live() {
            return Err(Failure::Closed);
        }
        if self.access.allowed(module, operation, resource) {
            Ok(())
        } else {
            Err(Failure::Denied)
        }
    }
    #[cfg(feature = "sport")]
    fn promote_stopped_watches(&self) {
        for destination in &self.watches {
            if destination.get().is_some() {
                continue;
            }
            let next = self
                .stopped_watches
                .iter()
                .enumerate()
                .filter_map(|(slot, watch)| watch.get().map(|watch| (slot, watch)))
                .min_by_key(|(_, watch)| match watch.resource {
                    Resource::Session(id) => id,
                    _ => u64::MAX,
                });
            let Some((slot, watch)) = next else { break };
            destination.set(Some(watch));
            self.stopped_watches[slot].set(None);
        }
    }
    #[cfg(feature = "sport")]
    fn stopped_watch_slot(&self, id: u64) -> Result<usize, Failure> {
        self.stopped_watches
            .iter()
            .position(|watch| {
                watch
                    .get()
                    .is_some_and(|watch| watch.resource == Resource::Session(id))
            })
            .or_else(|| {
                self.stopped_watches
                    .iter()
                    .position(|watch| watch.get().is_none())
            })
            .ok_or(Failure::Capacity)
    }
    fn slot(&self, module: u8, resource: Resource) -> Result<usize, Failure> {
        #[cfg(feature = "sport")]
        self.promote_stopped_watches();
        self.watches
            .iter()
            .position(|watch| {
                watch
                    .get()
                    .is_some_and(|value| value.module == module && value.resource == resource)
            })
            .or_else(|| self.watches.iter().position(|watch| watch.get().is_none()))
            .ok_or(Failure::Capacity)
    }
    fn watch(&self, slot: usize, module: u8, operation: Operation, resource: Resource) {
        let after = self.watches[slot]
            .get()
            .filter(|watch| watch.module == module && watch.resource == resource)
            .map_or(0, |watch| watch.after);
        self.watches[slot].set(Some(Watch {
            module,
            resource,
            operation,
            after,
            events: self
                .access
                .allowed(module, Operation::Event, event_resource(resource)),
        }));
    }
    fn metadata(&self, module: u8) -> Option<&ModuleEntry> {
        self.entries.iter().find(|entry| entry.module == module)
    }
    /// Prepare/retry one actual typed control event, without CRC/response prefix.
    /// # Errors
    /// Missing scope, real backend failure or caller capacity.
    pub fn next_event(&mut self, out: &mut [u8]) -> Result<Option<OfferedEvent>, Failure> {
        if self.now().is_err() {
            out.fill(0);
            return Err(Failure::Closed);
        }
        if let Some(event) = self.pending.get() {
            if !self.event_valid(event.token) {
                out.fill(0);
                return Err(if self.closed.get() {
                    Failure::Closed
                } else {
                    Failure::InvalidState
                });
            }
            if out.len() < event.length {
                return Err(Failure::Capacity);
            }
            let bytes = self.event_bytes.try_borrow().map_err(|_| Failure::Busy)?;
            out[..event.length].copy_from_slice(&bytes[..event.length]);
            return Ok(Some(event));
        }
        #[cfg(feature = "sport")]
        {
            self.promote_stopped_watches();
            if let Some((slot, watch)) = self
                .stopped_watches
                .iter()
                .enumerate()
                .filter_map(|(slot, watch)| {
                    watch
                        .get()
                        .filter(|watch| watch.events)
                        .map(|watch| (slot, watch))
                })
                .min_by_key(|(_, watch)| match watch.resource {
                    Resource::Session(id) => id,
                    _ => u64::MAX,
                })
            {
                let Resource::Session(queued_id) = watch.resource else {
                    return Err(Failure::Contract);
                };
                let earlier = self.watches.iter().filter_map(Cell::get).any(|current| {
                    current.module == 0x22
                        && current.events
                        && matches!(current.resource, Resource::Session(id) if id < queued_id)
                });
                if !earlier && let Some(event) = self.offer_watch(N + slot, watch, out)? {
                    return Ok(Some(event));
                }
            }
        }
        for step in 0..N {
            let slot = (self.cursor + step) % N;
            let Some(watch) = self.watches[slot].get() else {
                continue;
            };
            if let Some(event) = self.offer_watch(slot, watch, out)? {
                return Ok(Some(event));
            }
        }
        Ok(None)
    }
    fn watch_cell(&self, slot: usize) -> Option<&Cell<Option<Watch>>> {
        #[cfg(feature = "sport")]
        {
            self.watches.get(slot).or_else(|| {
                slot.checked_sub(N)
                    .and_then(|index| self.stopped_watches.get(index))
            })
        }
        #[cfg(not(feature = "sport"))]
        {
            self.watches.get(slot)
        }
    }
    fn offer_watch(
        &mut self,
        slot: usize,
        watch: Watch,
        out: &mut [u8],
    ) -> Result<Option<OfferedEvent>, Failure> {
        if !watch.events {
            return Ok(None);
        }
        #[cfg(feature = "sport")]
        if watch.module == 0x22 && (self.start_gate.is_some() || self.stop_gate.is_some()) {
            return Ok(None);
        }
        let Some(event) = self.owner.with_backend(|backend| {
            backend.pending_event(self.connection, watch.module, watch.resource, watch.after)
        })?
        else {
            return Ok(None);
        };
        if event.generation == 0 || event.generation <= watch.after {
            return Err(Failure::Contract);
        }
        let (module, opcode, resource) = event_identity(event.body);
        if module != watch.module || !compatible(watch.resource, resource) {
            return Err(Failure::Contract);
        }
        if !self.access.allowed(module, Operation::Event, resource) {
            self.invalidate();
            out.fill(0);
            return Err(Failure::Closed);
        }
        #[cfg(feature = "sport")]
        if self.discard_inactive_realtime(event, slot, watch)? {
            return Ok(None);
        }
        if !self.owner.with_backend(|backend| {
            Ok(backend.event_current(self.connection, module, event.generation))
        })? {
            return Err(Failure::InvalidState);
        }
        let mut bytes = self
            .event_bytes
            .try_borrow_mut()
            .map_err(|_| Failure::Busy)?;
        let length = encode_event(event.body, &mut bytes)?;
        if length > usize::from(self.metadata(module).ok_or(Failure::Contract)?.output_max)
            || out.len() < length
        {
            bytes.fill(0);
            return Err(Failure::Capacity);
        }
        out[..length].copy_from_slice(&bytes[..length]);
        let offered = OfferedEvent {
            token: EventToken {
                connection: self.connection,
                module,
                opcode,
                generation: event.generation,
                slot,
            },
            module,
            opcode,
            length,
        };
        self.pending.set(Some(offered));
        self.pending_resource.set(Some(resource));
        self.cursor = (slot + 1) % N;
        Ok(Some(offered))
    }
    #[cfg(feature = "sport")]
    fn discard_inactive_realtime(
        &self,
        event: crate::BackendEvent,
        slot: usize,
        watch: Watch,
    ) -> Result<bool, Failure> {
        if let EventBody::SportEnd { result, .. } = event.body {
            let state = self.owner.with_sport(|sport| {
                sport
                    .session(result.value.session)?
                    .state()
                    .map_err(motion_failure)
            })?;
            if state != SportState::Stopped {
                return Err(Failure::Contract);
            }
        }
        if let EventBody::SportRealtime(value) = event.body {
            let state = self.owner.with_sport(|sport| {
                sport
                    .session(value.session)?
                    .state()
                    .map_err(motion_failure)
            })?;
            if state != SportState::Running {
                self.owner.with_backend(|backend| {
                    backend.discard_event(self.connection, 0x22, event.generation)
                })?;
                self.watch_cell(slot)
                    .ok_or(Failure::Contract)?
                    .set(Some(Watch {
                        after: event.generation,
                        ..watch
                    }));
                return Ok(true);
            }
        }
        Ok(false)
    }
    /// Recheck scope and immutable context before each actual event-fragment admission.
    #[must_use]
    pub fn event_valid(&self, token: EventToken) -> bool {
        if !self.live()
            || self.pending.get().is_none_or(|event| event.token != token)
            || token.connection != self.connection
        {
            return false;
        }
        if self.pending_resource.get().is_none_or(|resource| {
            !self
                .access
                .allowed(token.module, Operation::Event, resource)
        }) {
            self.invalidate();
            return false;
        }
        self.owner
            .with_backend(|backend| {
                Ok(backend.event_current(self.connection, token.module, token.generation))
            })
            .unwrap_or(false)
    }
    /// Confirm only after the whole event was actually transport-admitted.
    /// # Errors
    /// Wrong/stale generation or actual backend confirmation failure.
    pub fn event_admitted(&mut self, token: EventToken) -> Result<(), Failure> {
        if !self.live() {
            return Err(Failure::Closed);
        }
        if self.previous == Some(token) && self.pending.get().is_none() {
            return Ok(());
        }
        if !self.event_valid(token) {
            return Err(Failure::InvalidState);
        }
        self.owner.with_backend(|backend| {
            backend.event_admitted(self.connection, token.module, token.generation)
        })?;
        let cell = self.watch_cell(token.slot).ok_or(Failure::InvalidState)?;
        let watch = cell.get().ok_or(Failure::InvalidState)?;
        if token.module == 0x22 && token.opcode == 0x81 {
            #[cfg(feature = "sport")]
            if let Resource::Session(id) = watch.resource {
                self.owner
                    .state
                    .try_borrow_mut()
                    .map_err(|_| Failure::Busy)?
                    .retention
                    .admitted(id);
            }
            cell.set(None);
        } else {
            cell.set(Some(Watch {
                after: token.generation,
                ..watch
            }));
        }
        self.pending.set(None);
        self.pending_resource.set(None);
        self.previous = Some(token);
        self.event_bytes
            .try_borrow_mut()
            .map_err(|_| Failure::Busy)?
            .fill(0);
        Ok(())
    }
    #[cfg(feature = "sport")]
    /// Resolve a real connection alias and recheck the stable sport's nonterminal lifetime.
    /// # Errors
    /// Unknown/stopped/closed/unauthorized context is never truncated into a session ID.
    pub fn resolve_context(&self, context: u32) -> Result<u64, Failure> {
        if !self.live() {
            return Err(Failure::Closed);
        }
        let id = {
            let aliases = self.aliases.try_borrow().map_err(|_| Failure::Busy)?;
            let state = self.owner.state.try_borrow().map_err(|_| Failure::Busy)?;
            aliases
                .resolve_active(context, &state.engine)
                .map_err(motion_failure)?
        };
        self.permission(0x22, Operation::Context, Resource::Session(id))?;
        Ok(id)
    }
    #[cfg(feature = "sport")]
    /// Read an existing frozen0022/schema1 observation without synthesizing/converting it.
    /// This is a record adapter port; normal22.02 continues to use2201/schema2.
    /// # Errors
    /// Missing actual old data, revoked scope or malformed source data.
    pub fn legacy_sport(&self) -> Result<Snapshot<LegacySportSummary>, Failure> {
        self.now()?;
        self.permission(0x22, Operation::Latest, Resource::Record(0x0022))?;
        match self
            .owner
            .with_backend(|backend| backend.latest(LatestSelector::LegacySport))?
        {
            Observation::LegacySport(value) => {
                snapshot_valid(value.generation, value.timestamp)?;
                value.value.validate().map_err(|_| Failure::Contract)?;
                self.permission(0x22, Operation::Latest, Resource::Record(0x0022))?;
                Ok(value)
            }
            Observation::Sport(_) => Err(Failure::Contract),
            #[cfg(any(feature = "sleep", feature = "activity", feature = "wear"))]
            _ => Err(Failure::Contract),
        }
    }
    fn disconnect(&mut self) -> Result<(), Failure> {
        self.invalidate();
        #[cfg(feature = "sport")]
        {
            self.start_gate = None;
            self.stop_gate = None;
        }
        if !self.detached {
            self.owner
                .with_backend(|backend| backend.detach(self.connection))?;
            #[cfg(feature = "sport")]
            self.owner
                .state
                .try_borrow_mut()
                .map_err(|_| Failure::Busy)?
                .retention
                .detach();
            self.detached = true;
            if self.owner.active.get() == Some(self.connection) {
                self.owner.active.set(None);
            }
        }
        Ok(())
    }
    fn execute(&mut self, frame: Frame<'_>, out: &mut [u8]) -> Result<usize, Failure> {
        let now = self.now()?;
        #[cfg(not(feature = "sport"))]
        let _ = now;
        let module = frame.header.module;
        let opcode = frame.header.opcode;
        let body = frame.payload;
        let (operation, resource) = scope(frame);
        self.permission(module, operation, resource)?;
        if opcode == 1 {
            if !body.is_empty() {
                return Err(Failure::Invalid);
            }
            let value = self.owner.with_backend(|backend| backend.status(module))?;
            if value.state > 3 {
                return Err(Failure::Contract);
            }
            out[4] = value.state;
            out[5..7].copy_from_slice(&value.reason.to_le_bytes());
            return Ok(7);
        }
        if opcode == 2 {
            return self.latest(module, body, out);
        }
        match (module, opcode) {
            #[cfg(feature = "sport")]
            (0x22, _) => self.sport(frame, now, out),
            #[cfg(feature = "sleep")]
            (0x21, 0x33) => {
                if body.len() != 16 {
                    return Err(Failure::Invalid);
                }
                let window = SleepWindow {
                    start_ms: read64(body, 0).cast_signed(),
                    end_ms: read64(body, 8).cast_signed(),
                };
                if window.start_ms >= window.end_ms {
                    return Err(Failure::Invalid);
                }
                let value = self
                    .owner
                    .with_backend(|backend| backend.sleep_summary(window))?;
                encode_sleep_summary(value, &mut out[4..])?;
                Ok(30)
            }
            #[cfg(feature = "activity")]
            (0x23, 0x30) => {
                if body.len() != 1 || !(1..=2).contains(&body[0]) {
                    return Err(Failure::Invalid);
                }
                let slot = self.slot(module, Resource::ActivityScope(body[0]))?;
                let value = self
                    .owner
                    .with_backend(|backend| backend.clear_activity(body[0], self.access))?;
                snapshot_valid(value.generation, value.timestamp)?;
                if value.value.kind != if body[0] == 1 { 1 } else { 3 }
                    || value.value.steps != 0
                    || value.value.distance_mm != 0
                    || value.value.energy_j != 0
                {
                    return Err(Failure::Contract);
                }
                value
                    .value
                    .encode(&mut out[4..])
                    .map_err(|_| Failure::Contract)?;
                self.watch(
                    slot,
                    module,
                    Operation::Clear,
                    Resource::ActivityScope(body[0]),
                );
                Ok(35)
            }
            _ => Err(Failure::Unsupported),
        }
    }
    #[cfg(feature = "sport")]
    fn sport_context(&self, body: &[u8], out: &mut [u8]) -> Result<usize, Failure> {
        if body.len() != 8 || read64(body, 0) == 0 {
            return Err(Failure::Invalid);
        }
        let id = read64(body, 0);
        let context_slot = self
            .contexts
            .iter()
            .position(|value| value.get() == Some(id))
            .or_else(|| self.contexts.iter().position(|value| value.get().is_none()))
            .ok_or(Failure::Capacity)?;
        let mut aliases = self.aliases.try_borrow_mut().map_err(|_| Failure::Busy)?;
        let state = self.owner.state.try_borrow().map_err(|_| Failure::Busy)?;
        let alias = aliases
            .assign(state.engine.session(id).map_err(motion_failure)?)
            .map_err(motion_failure)?;
        self.contexts[context_slot].set(Some(id));
        out[4..8].copy_from_slice(&alias.to_le_bytes());
        Ok(8)
    }
    #[cfg(feature = "sport")]
    fn sport(&mut self, frame: Frame<'_>, now: u64, out: &mut [u8]) -> Result<usize, Failure> {
        let module = frame.header.module;
        let opcode = frame.header.opcode;
        let body = frame.payload;
        match (module, opcode) {
            #[cfg(feature = "sport")]
            (0x22, 0x30) => {
                if body.len() != 3 || read16(body, 0) > 23 || body[2] > 2 {
                    return Err(Failure::Invalid);
                }
                let slot = self.slot(module, Resource::Module)?;
                let id = self.owner.with_sport(|sport| {
                    sport.start_authorized(read16(body, 0), body[2], now, Some(self.access))
                })?;
                self.watch(slot, module, Operation::Start, Resource::Session(id));
                out[4..12].copy_from_slice(&id.to_le_bytes());
                self.start_gate = Some(Gate::new(frame, &out[..12]));
                Ok(12)
            }
            #[cfg(feature = "sport")]
            (0x22, 0x31 | 0x32) => {
                if body.len() != 9
                    || read64(body, 0) == 0
                    || (opcode == 0x31 && !(1..=3).contains(&body[8]))
                    || (opcode == 0x32 && body[8] > 2)
                {
                    return Err(Failure::Invalid);
                }
                let id = read64(body, 0);
                let needed = self.owner.with_sport(|sport| sport.needs_watch(id))?;
                let destination = if needed {
                    Some(match self.slot(module, Resource::Session(id)) {
                        Ok(slot) => (slot, false),
                        Err(Failure::Capacity) if opcode == 0x31 && body[8] == 3 => {
                            (self.stopped_watch_slot(id)?, true)
                        }
                        Err(error) => return Err(error),
                    })
                } else {
                    None
                };
                if opcode == 0x31 {
                    let state = self.owner.with_sport(|sport| {
                        sport.control_authorized(id, body[8], now, Some(self.access))
                    })?;
                    out[4] = state as u8;
                    if state == SportState::Stopped {
                        self.aliases
                            .try_borrow_mut()
                            .map_err(|_| Failure::Busy)?
                            .session_ended(id);
                        for context in &self.contexts {
                            if context.get() == Some(id) {
                                context.set(None);
                            }
                        }
                        if needed {
                            self.stop_gate = Some(Gate::new(frame, &out[..5]));
                        }
                    }
                } else {
                    out[4] = u8::from(self.owner.with_sport(|sport| {
                        sport.save_authorized(id, body[8], Some(self.access))
                    })?);
                }
                let Some((slot, deferred)) = destination else {
                    return Ok(5);
                };
                if deferred {
                    self.stopped_watches[slot].set(Some(Watch {
                        module,
                        resource: Resource::Session(id),
                        operation: Operation::Control,
                        after: 0,
                        events: self.access.allowed(
                            module,
                            Operation::Event,
                            Resource::Session(id),
                        ),
                    }));
                } else {
                    self.watch(
                        slot,
                        module,
                        if opcode == 0x31 {
                            Operation::Control
                        } else {
                            Operation::Save
                        },
                        Resource::Session(id),
                    );
                }
                Ok(5)
            }
            #[cfg(feature = "sport")]
            (0x22, 0x34) => self.sport_context(body, out),
            _ => Err(Failure::Unsupported),
        }
    }
    fn latest(&mut self, module: u8, body: &[u8], out: &mut [u8]) -> Result<usize, Failure> {
        let selector = selector(module, body)?;
        let value = self
            .owner
            .with_backend(|backend| backend.latest(selector))?;
        if !selection_matches(selector, value) {
            return Err(Failure::Contract);
        }
        let resource = observation_resource(value);
        self.permission(module, Operation::Latest, resource)?;
        #[cfg(feature = "sport")]
        let needed = if let Observation::Sport(snapshot) = value {
            self.owner
                .with_sport(|sport| sport.needs_watch(snapshot.value.session))?
        } else {
            true
        };
        #[cfg(not(feature = "sport"))]
        let needed = true;
        let slot = if needed {
            Some(self.slot(module, resource)?)
        } else {
            None
        };
        let length = encode_observation(value, &mut out[4..])?;
        if let Some(slot) = slot {
            self.watch(slot, module, Operation::Latest, resource);
        }
        Ok(4 + length)
    }
}
impl<B: MotionBackend, const N: usize> ServiceHandler for MotionHandler<'_, B, N> {
    fn required_features(&self, module: &ModuleEntry, opcode: u8) -> Option<u32> {
        if self.metadata(module.module) != Some(module) {
            return None;
        }
        supported(module.module, opcode).then_some(0)
    }
    fn replay_valid(&self, request: Frame<'_>, response: &[u8]) -> bool {
        if !self.authorized(request) {
            return false;
        }
        if request.header.opcode == 2 && response.len() >= 19 && response[..4] == [0; 4] {
            let resource = if request.header.module == 0x22 && response.len() == 65 {
                Resource::Session(read64(response, 19))
            } else {
                Resource::Record(read16(response, 4))
            };
            if !self
                .access
                .allowed(request.header.module, Operation::Latest, resource)
            {
                self.invalidate();
                return false;
            }
        }
        true
    }
    fn authorized(&self, request: Frame<'_>) -> bool {
        if !self.live() {
            return false;
        }
        let (operation, resource) = scope(request);
        self.access
            .allowed(request.header.module, operation, resource)
    }
    fn must_end_session(&self) -> bool {
        !self.live()
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
        let Some(entry) = self.metadata(request.header.module) else {
            return Ok(error_response(out, Failure::Unsupported));
        };
        if !supported(entry.module, request.header.opcode) {
            return Ok(error_response(out, Failure::Unsupported));
        }
        let required = response_capacity(entry.module, request.header.opcode);
        if out.len() < required || required > usize::from(entry.output_max) {
            return Err(SessionError::Capacity);
        }
        if request.payload.len() > usize::from(entry.input_max) {
            return Ok(error_response(out, Failure::Invalid));
        }
        out[..4].fill(0);
        let allowed_before = self.authorized(request);
        let result = self.execute(request, out);
        if !self.live() || result == Err(Failure::Closed) || result == Err(Failure::Clock) {
            self.invalidate();
            out.fill(0);
            return Err(SessionError::AuthorizationRevoked);
        }
        if allowed_before && !self.authorized(request) {
            self.invalidate();
            out.fill(0);
            return Err(SessionError::AuthorizationRevoked);
        }
        match result {
            Ok(length) => Ok(length),
            Err(failure) => {
                let length = error_response(out, failure);
                #[cfg(feature = "wear")]
                if request.header.module == 0x24
                    && request.header.opcode == 2
                    && request.payload.len() == 2
                    && ![0x2401, 0x2402].contains(&read16(request.payload, 0))
                {
                    out[2..4].copy_from_slice(&(Reason::EnumUnsupported as u16).to_le_bytes());
                }
                Ok(length)
            }
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
        #[cfg(feature = "sport")]
        {
            if self
                .start_gate
                .is_some_and(|gate| gate.matches(request, response))
            {
                self.start_gate = None;
            }
            if self
                .stop_gate
                .is_some_and(|gate| gate.matches(request, response))
            {
                self.stop_gate = None;
            }
        }
        #[cfg(not(feature = "sport"))]
        let _ = response;
        Ok(())
    }
    fn connection_closed(&mut self) -> Result<(), SessionError> {
        self.disconnect()
            .map_err(|_| SessionError::InvalidHandlerResponse)
    }
}
fn compiled(module: u8) -> bool {
    match module {
        #[cfg(feature = "sleep")]
        0x21 => true,
        #[cfg(feature = "sport")]
        0x22 => true,
        #[cfg(feature = "activity")]
        0x23 => true,
        #[cfg(feature = "wear")]
        0x24 => true,
        _ => false,
    }
}
fn supported(module: u8, opcode: u8) -> bool {
    compiled(module)
        && (matches!(opcode, 1 | 2)
            || match module {
                #[cfg(feature = "sleep")]
                0x21 => opcode == 0x33,
                #[cfg(feature = "sport")]
                0x22 => matches!(opcode, 0x30..=0x32 | 0x34),
                #[cfg(feature = "activity")]
                0x23 => opcode == 0x30,
                _ => false,
            })
}
fn response_capacity(module: u8, opcode: u8) -> usize {
    match opcode {
        1 => 7,
        2 => match module {
            0x21 => 27,
            0x22 => 65,
            0x23 => 50,
            _ => 38,
        },
        0x30 if module == 0x22 => 12,
        0x30 => 35,
        0x31 | 0x32 => 5,
        0x33 => 30,
        0x34 => 8,
        _ => 4,
    }
}
fn scope(frame: Frame<'_>) -> (Operation, Resource) {
    let body = frame.payload;
    match frame.header.opcode {
        2 => (
            Operation::Latest,
            if frame.header.module == 0x24 && body.len() == 2 {
                Resource::Record(read16(body, 0))
            } else {
                Resource::Module
            },
        ),
        0x30 if frame.header.module == 0x22 => (Operation::Start, Resource::Module),
        0x30 => (
            Operation::Clear,
            if body.len() == 1 {
                Resource::ActivityScope(body[0])
            } else {
                Resource::Module
            },
        ),
        0x31 => (
            Operation::Control,
            if body.len() == 9 {
                Resource::Session(read64(body, 0))
            } else {
                Resource::Module
            },
        ),
        0x32 => (
            Operation::Save,
            if body.len() == 9 {
                Resource::Session(read64(body, 0))
            } else {
                Resource::Module
            },
        ),
        0x33 => (Operation::SleepSummary, Resource::Module),
        0x34 => (
            Operation::Context,
            if body.len() == 8 {
                Resource::Session(read64(body, 0))
            } else {
                Resource::Module
            },
        ),
        _ => (Operation::Status, Resource::Module),
    }
}
fn selector(module: u8, body: &[u8]) -> Result<LatestSelector, Failure> {
    if module != 0x24 && !body.is_empty() {
        return Err(Failure::Invalid);
    }
    match module {
        #[cfg(feature = "sleep")]
        0x21 => Ok(LatestSelector::Sleep),
        #[cfg(feature = "sport")]
        0x22 => Ok(LatestSelector::Sport),
        #[cfg(feature = "activity")]
        0x23 => Ok(LatestSelector::Activity),
        #[cfg(feature = "wear")]
        0x24 if body.len() == 2 => match read16(body, 0) {
            0x2401 => Ok(LatestSelector::Wear),
            0x2402 => Ok(LatestSelector::Posture),
            _ => Err(Failure::Invalid),
        },
        _ => Err(Failure::Invalid),
    }
}
fn selection_matches(selector: LatestSelector, value: Observation) -> bool {
    let expected = match selector {
        #[cfg(feature = "sleep")]
        LatestSelector::Sleep => 0x0021,
        #[cfg(feature = "sport")]
        LatestSelector::Sport => 0x2201,
        #[cfg(feature = "sport")]
        LatestSelector::LegacySport => 0x0022,
        #[cfg(feature = "activity")]
        LatestSelector::Activity => 0x2301,
        #[cfg(feature = "wear")]
        LatestSelector::Wear => 0x2401,
        #[cfg(feature = "wear")]
        LatestSelector::Posture => 0x2402,
    };
    let actual = match value {
        #[cfg(feature = "sleep")]
        Observation::SleepFinal(_) | Observation::SleepCurrent(_) => 0x0021,
        #[cfg(feature = "sport")]
        Observation::Sport(_) => 0x2201,
        #[cfg(feature = "sport")]
        Observation::LegacySport(_) => 0x0022,
        #[cfg(feature = "activity")]
        Observation::Activity(_) => 0x2301,
        #[cfg(feature = "wear")]
        Observation::Wear(_) => 0x2401,
        #[cfg(feature = "wear")]
        Observation::Posture(_) => 0x2402,
    };
    expected == actual
}
fn observation_resource(value: Observation) -> Resource {
    match value {
        #[cfg(feature = "sleep")]
        Observation::SleepFinal(_) | Observation::SleepCurrent(_) => Resource::Record(0x0021),
        #[cfg(feature = "sport")]
        Observation::Sport(value) => Resource::Session(value.value.session),
        #[cfg(feature = "sport")]
        Observation::LegacySport(_) => Resource::Record(0x0022),
        #[cfg(feature = "activity")]
        Observation::Activity(_) => Resource::Record(0x2301),
        #[cfg(feature = "wear")]
        Observation::Wear(_) => Resource::Record(0x2401),
        #[cfg(feature = "wear")]
        Observation::Posture(_) => Resource::Record(0x2402),
    }
}
fn snapshot_valid(generation: u64, stamp: Timestamp) -> Result<(), Failure> {
    if generation == 0 || (!stamp.valid && stamp.utc_ms != 0) {
        Err(Failure::Contract)
    } else {
        Ok(())
    }
}
fn latest_header(
    out: &mut [u8],
    record: u16,
    schema: u16,
    generation: u64,
    stamp: Timestamp,
    payload_len: u16,
) -> Result<(), Failure> {
    snapshot_valid(generation, stamp)?;
    if out.len() < 15 + usize::from(payload_len) {
        return Err(Failure::Capacity);
    }
    out[..2].copy_from_slice(&record.to_le_bytes());
    out[2..4].copy_from_slice(&schema.to_le_bytes());
    out[4..12].copy_from_slice(&stamp.utc_ms.to_le_bytes());
    out[12] = u8::from(stamp.valid);
    out[13..15].copy_from_slice(&payload_len.to_le_bytes());
    Ok(())
}
fn encode_observation(value: Observation, out: &mut [u8]) -> Result<usize, Failure> {
    macro_rules! observation {
        ($value:expr,$record:expr,$schema:expr,$len:expr) => {{
            let value = $value;
            latest_header(
                out,
                $record,
                $schema,
                value.generation,
                value.timestamp,
                $len,
            )?;
            value
                .value
                .encode(&mut out[15..])
                .map_err(|_| Failure::Contract)?;
            Ok(15 + usize::from($len))
        }};
    }
    match value {
        #[cfg(feature = "sleep")]
        Observation::SleepFinal(value) => observation!(value, 0x0021, 1, 7u16),
        #[cfg(feature = "sleep")]
        Observation::SleepCurrent(value) => observation!(value, 0x0021, 2, 8u16),
        #[cfg(feature = "sport")]
        Observation::Sport(value) => observation!(value, 0x2201, 2, 46u16),
        #[cfg(feature = "sport")]
        Observation::LegacySport(value) => observation!(value, 0x0022, 1, 20u16),
        #[cfg(feature = "activity")]
        Observation::Activity(value) => observation!(value, 0x2301, 1, 31u16),
        #[cfg(feature = "wear")]
        Observation::Wear(value) => observation!(value, 0x2401, 2, 19u16),
        #[cfg(feature = "wear")]
        Observation::Posture(value) => observation!(value, 0x2402, 2, 19u16),
    }
}
fn event_resource(resource: Resource) -> Resource {
    if matches!(resource, Resource::ActivityScope(_)) {
        Resource::Record(0x2301)
    } else {
        resource
    }
}
fn compatible(watch: Resource, event: Resource) -> bool {
    watch == event
        || matches!(watch, Resource::ActivityScope(_)) && event == Resource::Record(0x2301)
}
fn event_identity(body: EventBody) -> (u8, u8, Resource) {
    match body {
        #[cfg(feature = "sleep")]
        EventBody::Sleep(value) => (0x21, 0x80, observation_resource(value)),
        #[cfg(feature = "sport")]
        EventBody::SportRealtime(value) => (0x22, 0x80, Resource::Session(value.session)),
        #[cfg(feature = "sport")]
        EventBody::SportEnd { result, .. } => (0x22, 0x81, Resource::Session(result.value.session)),
        #[cfg(feature = "activity")]
        EventBody::ActivityCurrent(_) => (0x23, 0x80, Resource::Record(0x2301)),
        #[cfg(feature = "activity")]
        EventBody::ActivityEnd(_) => (0x23, 0x81, Resource::Record(0x2301)),
        #[cfg(feature = "wear")]
        EventBody::WearChange { kind, .. } => (
            0x24,
            0x80,
            Resource::Record(match kind {
                WearKind::Wear => 0x2401,
                WearKind::Posture => 0x2402,
            }),
        ),
        #[cfg(feature = "wear")]
        EventBody::WearEnd(_) => (0x24, 0x81, Resource::Record(0x2401)),
        #[cfg(feature = "wear")]
        EventBody::PostureEnd(_) => (0x24, 0x81, Resource::Record(0x2402)),
    }
}
fn encode_event(body: EventBody, out: &mut [u8; 512]) -> Result<usize, Failure> {
    match body {
        #[cfg(feature = "sleep")]
        EventBody::Sleep(value @ (Observation::SleepFinal(_) | Observation::SleepCurrent(_))) => {
            encode_observation(value, out)
        }
        #[cfg(all(
            feature = "sleep",
            any(feature = "sport", feature = "activity", feature = "wear")
        ))]
        EventBody::Sleep(_) => Err(Failure::Contract),
        #[cfg(feature = "sport")]
        EventBody::SportRealtime(value) => value.encode(out).map_err(|_| Failure::Contract),
        #[cfg(feature = "sport")]
        EventBody::SportEnd { reason, result } => {
            if ![0, 0x0104, 0x0105].contains(&reason) {
                return Err(Failure::Contract);
            }
            out[..2].copy_from_slice(&reason.to_le_bytes());
            Ok(2 + encode_observation(Observation::Sport(result), &mut out[2..])?)
        }
        #[cfg(feature = "activity")]
        EventBody::ActivityCurrent(value) => {
            if value.value.kind != 4 {
                return Err(Failure::Contract);
            }
            encode_observation(Observation::Activity(value), out)
        }
        #[cfg(feature = "activity")]
        EventBody::ActivityEnd(value) => {
            if ![2, 3].contains(&value.value.kind) {
                return Err(Failure::Contract);
            }
            encode_observation(Observation::Activity(value), out)
        }
        #[cfg(feature = "wear")]
        EventBody::WearChange {
            kind,
            timestamp,
            value,
        } => {
            snapshot_valid(1, timestamp)?;
            let record: u16 = match kind {
                WearKind::Wear if value <= 2 => 0x2401,
                WearKind::Posture if [0, 1, 255].contains(&value) => 0x2402,
                _ => return Err(Failure::Contract),
            };
            out[..2].copy_from_slice(&record.to_le_bytes());
            out[2..10].copy_from_slice(&timestamp.utc_ms.to_le_bytes());
            out[10] = u8::from(timestamp.valid);
            out[11] = value;
            Ok(12)
        }
        #[cfg(feature = "wear")]
        EventBody::WearEnd(value) => {
            if value.value.provisional != 0 {
                return Err(Failure::Contract);
            }
            encode_observation(Observation::Wear(value), out)
        }
        #[cfg(feature = "wear")]
        EventBody::PostureEnd(value) => {
            if value.value.provisional != 0 {
                return Err(Failure::Contract);
            }
            encode_observation(Observation::Posture(value), out)
        }
    }
}
#[cfg(feature = "sleep")]
fn encode_sleep_summary(value: SleepSummary, out: &mut [u8]) -> Result<(), Failure> {
    let fields = [
        value.total_ms,
        value.deep_ms,
        value.light_ms,
        value.awake_ms,
        value.rem_ms,
        value.interrupted_ms,
    ];
    if value.valid_mask & !63 != 0 {
        return Err(Failure::Contract);
    }
    for (index, field) in fields.iter().enumerate() {
        if value.valid_mask & (1 << index) == 0 && *field != 0 {
            return Err(Failure::Contract);
        }
    }
    if value.valid_mask & 1 != 0
        && value
            .deep_ms
            .checked_add(value.light_ms)
            .and_then(|sum| sum.checked_add(value.rem_ms))
            != Some(value.total_ms)
    {
        return Err(Failure::Contract);
    }
    for (index, field) in fields.iter().enumerate() {
        out[index * 4..index * 4 + 4].copy_from_slice(&field.to_le_bytes());
    }
    out[24..26].copy_from_slice(&value.valid_mask.to_le_bytes());
    Ok(())
}
fn error_response(out: &mut [u8], failure: Failure) -> usize {
    let (status, reason) = match failure {
        Failure::Invalid => (Status::InvalidArgument, Reason::None),
        Failure::Unsupported => (Status::UnsupportedOpcode, Reason::None),
        Failure::NotFound => (Status::NotFound, Reason::None),
        Failure::InvalidState | Failure::Closed => (Status::InvalidState, Reason::None),
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
fn read16(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}
fn read64(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes([
        bytes[at],
        bytes[at + 1],
        bytes[at + 2],
        bytes[at + 3],
        bytes[at + 4],
        bytes[at + 5],
        bytes[at + 6],
        bytes[at + 7],
    ])
}
#[cfg(feature = "sport")]
pub(crate) fn motion_failure(error: MotionError) -> Failure {
    match error {
        MotionError::Invalid => Failure::Invalid,
        MotionError::Busy => Failure::Busy,
        MotionError::NotFound => Failure::NotFound,
        MotionError::Conflict => Failure::Conflict,
        MotionError::Unavailable => Failure::Unsupported,
        MotionError::Capacity => Failure::Capacity,
        MotionError::Time => Failure::Clock,
        MotionError::Backend => Failure::Backend,
    }
}
