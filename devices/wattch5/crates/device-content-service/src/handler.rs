use crate::{
    Backend, Command, ContentAccess, ContentClock, Error, EventToken,
    state::{CleanupReport, EventSpec, Finish, Services, SharedContentOwner, Watch},
    wire::{local_error, status, wire_error},
};
use core::cell::Cell;
use device_content::{AccessPort, Scope};
use device_protocol::{Frame, Kind, ModuleEntry};
use device_services::{Reason, ServiceHandler, SessionError, Status};
/// Immutable authenticated binding and compiled module catalogue for one physical connection.
#[derive(Clone, Copy)]
pub struct Binding<'a> {
    /// Actual authenticated scope, never a packet field.
    pub scope: Scope,
    /// Sorted unique actually compiled module entries.
    pub entries: &'a [ModuleEntry],
    /// Explicit phone policy codec product ranges.
    #[cfg(any(feature = "phone", feature = "notifications"))]
    pub manifest: device_content::Manifest<'a>,
}
impl core::fmt::Debug for Binding<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Binding")
            .field("module_length", &self.entries.len())
            .finish_non_exhaustive()
    }
}
#[derive(Debug, Clone, Copy)]
enum Effect {
    #[cfg(any(feature = "phone", feature = "media", feature = "notifications"))]
    None,
    #[cfg(feature = "media")]
    Action { lane: crate::state::Lane, id: u32 },
    #[cfg(feature = "find")]
    Start { id: u32, phone: bool },
    #[cfg(feature = "find")]
    Stop(u32),
    #[cfg(feature = "find")]
    Terminal(u32),
    #[cfg(feature = "companion-radio")]
    Radio,
    #[cfg(feature = "gimbal")]
    Gimbal(u32),
}
type Executed = (usize, Effect, Option<(u32, u64)>);
#[derive(Clone, Copy)]
struct Guard {
    module: u8,
    opcode: u8,
    request_id: u16,
    payload: [u8; 11],
    payload_len: usize,
    response: [u8; 8],
    response_len: usize,
    resource: Option<(u32, u64)>,
    effect: Effect,
}
impl core::fmt::Debug for Guard {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Guard")
            .field("request_length", &self.payload_len)
            .field("response_length", &self.response_len)
            .finish_non_exhaustive()
    }
}
impl Guard {
    fn matches(self, frame: Frame<'_>, response: &[u8]) -> bool {
        self.module == frame.header.module
            && self.opcode == frame.header.opcode
            && self.request_id == frame.header.message_id
            && frame.payload == &self.payload[..self.payload_len]
            && response == &self.response[..self.response_len]
    }
}
struct CheckedAccess<'a> {
    access: &'a dyn ContentAccess,
    scope: Scope,
    module: u8,
    opcode: u8,
    id: Option<u32>,
}
impl core::fmt::Debug for CheckedAccess<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CheckedAccess").finish_non_exhaustive()
    }
}
impl AccessPort for CheckedAccess<'_> {
    fn authorize(
        &self,
        scope: Scope,
        operation: device_content::Operation,
    ) -> Result<(), device_content::Error> {
        if scope != self.scope
            || !self.access.allowed(self.module, self.opcode, None)
            || self
                .id
                .is_some_and(|id| !self.access.allowed(self.module, self.opcode, Some(id)))
        {
            return Err(device_content::Error::Denied);
        }
        self.access.authorize(scope, operation)
    }
}
/// One connection adapter routing every selected content module through the same owner and FIFO.
pub struct ContentHandler<
    'a,
    'data,
    B: Backend,
    const N: usize,
    const C: usize,
    const A: usize,
    const Q: usize,
> {
    owner: &'a SharedContentOwner<'data, B, N, C, A, Q>,
    binding: Binding<'a>,
    access: &'a dyn ContentAccess,
    clock: &'a dyn ContentClock,
    generation: u64,
    lastnow: Cell<Option<u64>>,
    closed: Cell<bool>,
    pending: Cell<Option<Guard>>,
    cached: Cell<Option<Guard>>,
    admitted: Cell<Option<Guard>>,
    last_event: Cell<Option<EventToken>>,
}
impl<B: Backend, const N: usize, const C: usize, const A: usize, const Q: usize> core::fmt::Debug
    for ContentHandler<'_, '_, B, N, C, A, Q>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ContentHandler")
            .field("module_length", &self.binding.entries.len())
            .finish_non_exhaustive()
    }
}
impl<'a, 'data, B: Backend, const N: usize, const C: usize, const A: usize, const Q: usize>
    ContentHandler<'a, 'data, B, N, C, A, Q>
{
    /// Bind all selected content routes to one actual authenticated connection and monotonic source.
    /// # Errors
    /// Rejects catalogue/version/capacity errors, scope mismatch, reused connection, or an active/failed owner.
    pub fn new(
        owner: &'a SharedContentOwner<'data, B, N, C, A, Q>,
        binding: Binding<'a>,
        access: &'a dyn ContentAccess,
        clock: &'a dyn ContentClock,
    ) -> Result<Self, Error> {
        if N == 0
            || Q == 0
            || binding.entries.is_empty()
            || binding
                .entries
                .windows(2)
                .any(|pair| pair[0].module >= pair[1].module)
            || binding.entries.iter().any(|entry| {
                entry.major != 1
                    || entry.minor != 0
                    || entry.features != 0
                    || entry.availability != 1
                    || !(128..=512).contains(&entry.input_max)
                    || !(128..=512).contains(&entry.output_max)
                    || !module_compiled(entry.module)
            })
        {
            return Err(Error::Invalid);
        }
        #[cfg(feature = "notifications")]
        if C == 0 || A == 0 {
            return Err(Error::Invalid);
        }
        if access.scope()? != binding.scope {
            return Err(Error::Denied);
        }
        let generation = access.generation()?;
        if generation == 0 {
            return Err(Error::Invalid);
        }
        if owner.report.get().failed {
            return Err(Error::Backend);
        }
        if owner.active.get().is_some() {
            return Err(Error::Busy);
        }
        if binding.scope.connection() <= owner.last_connection.get() {
            return Err(Error::Invalid);
        }
        let mut state = owner.state.try_borrow_mut().map_err(|_| Error::Busy)?;
        #[cfg(feature = "gimbal")]
        if let Some(previous) = state
            .services
            .as_ref()
            .filter(|services| services.gimbal_needs_drain)
            .map(|services| services.scope)
        {
            use crate::GimbalLifecyclePort;
            if !state.backend.gimbal().drained(previous)? {
                return Err(Error::Busy);
            }
        }
        state.services = Some(Services::new(binding.scope, generation));
        drop(state);
        owner.active.set(Some(binding.scope));
        owner.last_connection.set(binding.scope.connection());
        Ok(Self {
            owner,
            binding,
            access,
            clock,
            generation,
            lastnow: Cell::new(None),
            closed: Cell::new(false),
            pending: Cell::new(None),
            cached: Cell::new(None),
            admitted: Cell::new(None),
            last_event: Cell::new(None),
        })
    }
    fn checked(&self, module: u8, opcode: u8, id: Option<u32>) -> CheckedAccess<'_> {
        CheckedAccess {
            access: self.access,
            scope: self.binding.scope,
            module,
            opcode,
            id,
        }
    }
    fn entry(&self, module: u8) -> Option<&ModuleEntry> {
        self.binding
            .entries
            .iter()
            .find(|entry| entry.module == module)
    }
    fn ensure_module(&self, module: u8) -> Result<(), Error> {
        if self.entry(module).is_none() {
            return Err(device_content::Error::Unknown.into());
        }
        Ok(())
    }
    fn close_inner(&self) -> Result<(), Error> {
        if self.closed.replace(true) {
            return if self.owner.report.get().failed {
                Err(Error::Backend)
            } else {
                Ok(())
            };
        }
        self.pending.set(None);
        self.cached.set(None);
        self.admitted.set(None);
        self.last_event.set(None);
        let report = match self
            .owner
            .with(|services, backend| Ok(cleanup_services(services, backend)))
        {
            Ok(report) => report,
            Err(_) => CleanupReport {
                unknown_count: 0,
                failed: true,
            },
        };
        self.owner.report.set(report);
        if self.owner.active.get() == Some(self.binding.scope) {
            self.owner.active.set(None);
        }
        if report.failed {
            Err(Error::Backend)
        } else {
            Ok(())
        }
    }
    fn live(&self) -> Result<(), Error> {
        if self.closed.get() {
            return Err(Error::Closed);
        }
        let current = self.access.scope();
        let generation = self.access.generation();
        if current != Ok(self.binding.scope)
            || generation != Ok(self.generation)
            || self.owner.active.get() != Some(self.binding.scope)
        {
            let _ = self.close_inner();
            return Err(Error::Denied);
        }
        let permitted = self.owner.with(|services, _| {
            Ok(services.scope == self.binding.scope
                && services.generation == self.generation
                && services.watches.iter().flatten().all(|watch| {
                    self.access
                        .authorize(self.binding.scope, watch_operation(watch.module))
                        .is_ok()
                        && self.access.allowed(watch.module, watch.opcode, None)
                        && self
                            .access
                            .allowed(watch.module, watch.opcode, Some(watch.id))
                        && {
                            #[cfg(feature = "notifications")]
                            {
                                watch.module != 0x30
                                    || self
                                        .access
                                        .notification_current(watch.id, watch.resource_generation)
                            }
                            #[cfg(not(feature = "notifications"))]
                            {
                                watch.resource_generation == 0
                            }
                        }
                }))
        })?;
        if !permitted {
            let _ = self.close_inner();
            return Err(Error::Denied);
        }
        Ok(())
    }
    fn touch(&self) -> Result<u64, Error> {
        self.live()?;
        let now = match self.clock.now_ms() {
            Ok(now) => now,
            Err(error) => {
                let _ = self.close_inner();
                return Err(error);
            }
        };
        if self.lastnow.get().is_some_and(|last| now < last) {
            let _ = self.close_inner();
            return Err(Error::Clock);
        }
        self.lastnow.set(Some(now));
        Ok(now)
    }
    /// Explicit disconnect/CCCD cleanup. Failed hardware cleanup remains latched on the owner.
    /// # Errors
    /// Reports actual cleanup failure while keeping the handler permanently closed.
    pub fn disconnect(&mut self) -> Result<(), Error> {
        self.close_inner()
    }
    fn authorize_command(&self, frame: Frame<'_>, command: Command) -> bool {
        self.access
            .authorize(self.binding.scope, command.operation())
            .is_ok()
            && self
                .access
                .allowed(frame.header.module, frame.header.opcode, None)
            && command.id().is_none_or(|id| {
                self.access
                    .allowed(frame.header.module, frame.header.opcode, Some(id))
            })
    }
    fn replay_guard(&self, guard: Guard, frame: Frame<'_>, response: &[u8]) -> bool {
        if self.closed.get() {
            return false;
        }
        if !guard.matches(frame, response) {
            return false;
        }
        #[cfg(feature = "notifications")]
        if let Some((notification, generation)) = guard.resource
            && !self.access.notification_current(notification, generation)
        {
            return false;
        }
        #[cfg(not(feature = "notifications"))]
        if guard.resource.is_some() {
            return false;
        }
        true
    }
}
fn module_compiled(module: u8) -> bool {
    match module {
        #[cfg(feature = "find")]
        6 => true,
        #[cfg(feature = "phone")]
        7 => true,
        #[cfg(feature = "companion-radio")]
        8 => true,
        #[cfg(feature = "notifications")]
        0x30 => true,
        #[cfg(any(feature = "media", feature = "gimbal"))]
        0x33 => true,
        _ => false,
    }
}
fn opcode_compiled(module: u8, opcode: u8) -> bool {
    match (module, opcode) {
        #[cfg(feature = "find")]
        (6, 0x30..=0x32) => true,
        #[cfg(feature = "phone")]
        (7, 0x30..=0x31) => true,
        #[cfg(feature = "companion-radio")]
        (8, 0x30) => true,
        #[cfg(feature = "notifications")]
        (0x30, 0x30) => true,
        #[cfg(feature = "media")]
        (0x33, 0x30..=0x33) => true,
        #[cfg(feature = "gimbal")]
        (0x33, 0x34) => true,
        _ => false,
    }
}

impl<B: Backend, const N: usize, const C: usize, const A: usize, const Q: usize>
    ContentHandler<'_, '_, B, N, C, A, Q>
{
    fn execute(
        &self,
        frame: Frame<'_>,
        command: Command,
        out: &mut [u8],
        now: u64,
    ) -> Result<Executed, Error> {
        let _ = (&out, now);
        let checked = self.checked(frame.header.module, frame.header.opcode, command.id());
        self.owner.with(|services, backend| {
            let _ = &backend;
            match command {
                #[cfg(feature = "media")]
                Command::CameraMode(enabled) => local_action(
                    services,
                    backend,
                    &checked,
                    device_content::Action::CameraMode(enabled),
                    0x30,
                ),
                #[cfg(feature = "media")]
                Command::Music(action) => local_action(
                    services,
                    backend,
                    &checked,
                    device_content::Action::DeviceMusic(action),
                    0x32,
                ),
                #[cfg(feature = "media")]
                Command::CameraAck { id, result } => {
                    settle_action(services, &checked, id, result, 0x33, 0x80)?;
                    Ok((4, Effect::None, None))
                }
                #[cfg(feature = "media")]
                Command::MusicAck { id, result } => {
                    settle_action(services, &checked, id, result, 0x33, 0x81)?;
                    Ok((4, Effect::None, None))
                }
                #[cfg(feature = "phone")]
                Command::SosAck { id, result } => {
                    settle_action(services, &checked, id, result, 7, 0x81)?;
                    Ok((4, Effect::None, None))
                }
                #[cfg(feature = "phone")]
                Command::Heartbeat(token) => {
                    let echo = services.heartbeat.heartbeat(token, now, &checked)?;
                    out[4..8].copy_from_slice(&echo.to_le_bytes());
                    Ok((8, Effect::None, None))
                }
                #[cfg(feature = "find")]
                Command::Find(request) => execute_find(services, backend, &checked, request, out),
                #[cfg(feature = "find")]
                Command::FindStop(id) => {
                    services.find.prepare_stop(id, &checked)?;
                    Ok((4, Effect::Stop(id), None))
                }
                #[cfg(feature = "find")]
                Command::FindResult { id, result } => {
                    services
                        .find
                        .phone_result(id, result, &checked)
                        .map_err(terminal_error)?;
                    Ok((4, Effect::Terminal(id), None))
                }
                #[cfg(feature = "companion-radio")]
                Command::Radio(enabled) => execute_radio(services, backend, &checked, enabled),
                #[cfg(feature = "gimbal")]
                Command::Gimbal(request) => execute_gimbal(services, backend, &checked, request),
                #[cfg(feature = "notifications")]
                Command::NotificationAck {
                    notification,
                    action,
                    result,
                } => {
                    let record = services
                        .notification_records
                        .iter()
                        .flatten()
                        .find(|record| record.notification == notification)
                        .ok_or(device_content::Error::NotFound)?;
                    if !self
                        .access
                        .notification_current(notification, record.generation)
                    {
                        return Err(Error::Denied);
                    }
                    let generation = record.generation;
                    services
                        .notifications
                        .settle(notification, action, result, &checked)
                        .map_err(terminal_error)?;
                    Ok((4, Effect::None, Some((notification, generation))))
                }
            }
        })
    }
    fn admit_effect(&self, guard: Guard, now: u64) -> Result<(), Error> {
        let _ = now;
        let checked = self.checked(
            guard.module,
            guard.opcode,
            Command::decode(
                guard.module,
                guard.opcode,
                &guard.payload[..guard.payload_len],
            )?
            .id(),
        );
        self.owner.with(|services, backend| {
            let _ = (&checked, &services, &backend);
            match guard.effect {
                #[cfg(any(feature = "phone", feature = "media", feature = "notifications"))]
                Effect::None => {}
                #[cfg(feature = "media")]
                Effect::Action { lane, id } => {
                    admit_action(services, backend, &checked, lane, id, false)?;
                }
                #[cfg(feature = "find")]
                Effect::Start { id, phone } => {
                    if phone {
                        let mut body = [0; 8];
                        services.find.request_event(id, &mut body)?;
                        services.stage_reserved(
                            EventSpec {
                                module: 6,
                                opcode: 0x80,
                                id,
                                resource_generation: 0,
                            },
                            &body,
                            Finish::Find(id),
                        )?;
                    } else {
                        let result =
                            services
                                .find
                                .response_admitted(id, now, &checked, backend.find());
                        find_backend_result(services, id, result)?;
                    }
                }
                #[cfg(feature = "find")]
                Effect::Stop(id) => {
                    let result = services
                        .find
                        .stop_response_admitted(id, &checked, backend.find());
                    find_backend_result(services, id, result)?;
                    stage_find_terminal(services, id, true)?;
                }
                #[cfg(feature = "find")]
                Effect::Terminal(id) => {
                    stage_find_terminal(services, id, true)?;
                }
                #[cfg(feature = "companion-radio")]
                Effect::Radio => {
                    services
                        .radio
                        .response_admitted(&checked, backend.radio())?;
                }
                #[cfg(feature = "gimbal")]
                Effect::Gimbal(id) => {
                    services
                        .gimbal
                        .response_admitted(id, &checked, backend.gimbal())?;
                }
            }
            Ok(())
        })
    }
    /// Prepare the FIFO head with unchanged bytes on every retry; uses each exact control-event body without an invented CRC.
    /// # Errors
    /// Returns revoked scope/generation, no event, or insufficient negotiated/output capacity.
    pub fn prepare_event(
        &mut self,
        out: &mut [u8],
        max: usize,
    ) -> Result<Option<(EventToken, usize)>, Error> {
        let result = (|| {
            self.touch()?;
            if self.pending.get().is_some() {
                return Ok(None);
            }
            self.owner.with(|services, _| {
                if services.queue_len == 0 {
                    return Ok(None);
                }
                let event = services.front()?;
                if !self.event_authorized(event.token) {
                    return Err(Error::Denied);
                }
                let declared = self
                    .entry(event.token.module)
                    .ok_or(Error::Invalid)?
                    .output_max;
                if event.len > out.len().min(max).min(usize::from(declared)) {
                    return Err(Error::Capacity);
                }
                out[..event.len].copy_from_slice(&event.bytes[..event.len]);
                Ok(Some((event.token, event.len)))
            })
        })();
        if matches!(result, Err(Error::Denied)) {
            let _ = self.close_inner();
        }
        if result.is_err() {
            out.fill(0);
        }
        result
    }
    pub(crate) fn event_authorized(&self, token: EventToken) -> bool {
        let operation = match token.module {
            #[cfg(feature = "find")]
            6 => device_content::Operation::Find,
            #[cfg(feature = "notifications")]
            0x30 => device_content::Operation::Notification,
            _ => device_content::Operation::Submit,
        };
        self.access.authorize(self.binding.scope, operation).is_ok()
            && token.connection == self.binding.scope.connection()
            && token.generation == self.generation
            && self.access.allowed(token.module, token.opcode, None)
            && self
                .access
                .allowed(token.module, token.opcode, Some(token.id))
            && {
                #[cfg(feature = "notifications")]
                {
                    if token.module == 0x30
                        && !self
                            .access
                            .notification_current(token.id, token.resource_generation)
                    {
                        return false;
                    }
                }
                #[cfg(feature = "companion-radio")]
                {
                    if token.module == 8
                        && token.opcode == 0x90
                        && !self
                            .access
                            .radio_current(token.id, token.resource_generation)
                    {
                        return false;
                    }
                }
                true
            }
    }
    /// Confirm only after the final fragment entered the same actual FIFO. Exact repeat is harmless.
    /// # Errors
    /// Rejects stale/out-of-order receipt, revoked context or actual post-admission backend failure.
    pub fn confirm_event(&mut self, token: EventToken) -> Result<(), Error> {
        let now = self.touch()?;
        let _ = now;
        if !self.event_authorized(token) {
            let _ = self.close_inner();
            return Err(Error::Denied);
        }
        if self.last_event.get() == Some(token) {
            return Ok(());
        }
        let checked = self.checked(token.module, token.opcode, Some(token.id));
        let result = self.owner.with(|services, backend| {
            let _ = (&backend, &checked);
            let event = services.front()?;
            if event.token != token {
                return Err(Error::Invalid);
            }
            match event.finish {
                #[cfg(any(feature = "find", feature = "gimbal", feature = "companion-radio"))]
                Finish::None => {}
                #[cfg(any(feature = "phone", feature = "media"))]
                Finish::Action { lane, id } => {
                    admit_action(services, backend, &checked, lane, id, true)?;
                }
                #[cfg(feature = "find")]
                Finish::Find(id) => {
                    let result = services
                        .find
                        .response_admitted(id, now, &checked, backend.find());
                    find_backend_result(services, id, result)?;
                }
                #[cfg(feature = "notifications")]
                Finish::Notification {
                    notification,
                    action,
                } => {
                    services.notifications.action_response_admitted(
                        notification,
                        action,
                        &checked,
                        backend.notifications(),
                    )?;
                }
            }
            services.pop()?;
            Ok(())
        });
        if self.live().is_err() {
            return Err(Error::Denied);
        }
        if result.is_ok() {
            self.last_event.set(Some(token));
        } else if matches!(
            result,
            Err(Error::Content(
                device_content::Error::Backend
                    | device_content::Error::Unavailable
                    | device_content::Error::Denied
            ))
        ) {
            let _ = self.close_inner();
        }
        #[cfg(feature = "find")]
        self.close_uncertain_find();
        result
    }
}
#[cfg(any(feature = "phone", feature = "media"))]
fn action_lane(action: device_content::Action) -> crate::state::Lane {
    match action {
        #[cfg(feature = "phone")]
        device_content::Action::Sos { .. } | device_content::Action::PhoneStatus => {
            crate::state::Lane::Phone
        }
        #[cfg(feature = "media")]
        device_content::Action::CameraShutter | device_content::Action::PhoneMusic(_) => {
            crate::state::Lane::Media
        }
        #[cfg(feature = "media")]
        device_content::Action::CameraMode(_) | device_content::Action::DeviceMusic(_) => {
            crate::state::Lane::Local
        }
    }
}
#[cfg(any(feature = "phone", feature = "media"))]
fn action_ledger<'borrow, const N: usize, const C: usize, const A: usize, const Q: usize>(
    services: &'borrow mut Services<'_, N, C, A, Q>,
    lane: crate::state::Lane,
) -> &'borrow mut device_content::ActionService<N> {
    match lane {
        #[cfg(feature = "phone")]
        crate::state::Lane::Phone => &mut services.phone_actions,
        #[cfg(feature = "media")]
        crate::state::Lane::Media => &mut services.media_actions,
        #[cfg(feature = "media")]
        crate::state::Lane::Local => &mut services.local_actions,
    }
}
#[cfg(any(feature = "phone", feature = "media"))]
fn prepare_action<B: Backend, const N: usize, const C: usize, const A: usize, const Q: usize>(
    services: &mut Services<'_, N, C, A, Q>,
    backend: &mut B,
    access: &impl AccessPort,
    action: device_content::Action,
    module: u8,
    opcode: u8,
) -> Result<u32, Error> {
    use crate::state::ActionRecord;
    let lane = action_lane(action);
    if !services.watch_available() && !coalesces(services, action) {
        return Err(Error::Capacity);
    }
    let id = action_ledger(services, lane).prepare(action, access, backend.actions())?;
    if let Some(record) = services
        .action_records
        .iter()
        .flatten()
        .find(|record| record.id == id && record.lane == lane)
    {
        if record.action != action {
            return Err(device_content::Error::Conflict.into());
        }
        return Ok(id);
    }
    let record = services
        .action_records
        .iter_mut()
        .find(|record| record.is_none())
        .ok_or(Error::Capacity)?;
    *record = Some(ActionRecord {
        id,
        lane,
        action,
        module,
        opcode: match action {
            #[cfg(feature = "media")]
            device_content::Action::CameraShutter => 0x80,
            #[cfg(feature = "media")]
            device_content::Action::PhoneMusic(_) => 0x81,
            #[cfg(feature = "phone")]
            device_content::Action::Sos { .. } => 0x81,
            #[cfg(feature = "phone")]
            device_content::Action::PhoneStatus => 0x80,
            #[cfg(feature = "media")]
            device_content::Action::CameraMode(_) | device_content::Action::DeviceMusic(_) => 0,
        },
        queued: false,
    });
    services.watch(Watch {
        module,
        opcode,
        id,
        resource_generation: 0,
    })?;
    Ok(id)
}
#[cfg(any(feature = "phone", feature = "media"))]
fn settle_action<const N: usize, const C: usize, const A: usize, const Q: usize>(
    services: &mut Services<'_, N, C, A, Q>,
    access: &impl AccessPort,
    id: u32,
    result: u8,
    module: u8,
    opcode: u8,
) -> Result<(), Error> {
    let record = services
        .action_records
        .iter()
        .flatten()
        .find(|record| record.id == id && record.module == module && record.opcode != 0)
        .copied()
        .ok_or(device_content::Error::NotFound)?;
    if record.opcode != opcode {
        return Err(device_content::Error::State.into());
    }
    action_ledger(services, record.lane)
        .settle(id, result, access)
        .map_err(terminal_error)?;
    Ok(())
}
#[cfg(any(feature = "phone", feature = "media"))]
fn admit_action<B: Backend, const N: usize, const C: usize, const A: usize, const Q: usize>(
    services: &mut Services<'_, N, C, A, Q>,
    backend: &mut B,
    access: &impl AccessPort,
    lane: crate::state::Lane,
    id: u32,
    remote: bool,
) -> Result<(), Error> {
    let state = action_ledger(services, lane).response_admitted(id, access, backend.actions())?;
    if remote && state != device_content::ActionState::AwaitingResult {
        return Err(Error::Backend);
    }
    Ok(())
}
#[cfg(feature = "find")]
fn stage_find_terminal<const N: usize, const C: usize, const A: usize, const Q: usize>(
    services: &mut Services<'_, N, C, A, Q>,
    id: u32,
    reserved: bool,
) -> Result<(), Error> {
    let queued = services
        .find_records
        .iter()
        .flatten()
        .find(|record| record.0 == id)
        .ok_or(device_content::Error::NotFound)?
        .1;
    if queued {
        if reserved {
            services.release()?;
        }
        return Ok(());
    }
    let mut body = [0; 5];
    services.find.result_event(id, &mut body)?;
    let event = EventSpec {
        module: 6,
        opcode: 0x81,
        id,
        resource_generation: 0,
    };
    if reserved {
        services.stage_reserved(event, &body, Finish::None)?;
    } else {
        services.stage(event, &body, Finish::None)?;
    }
    let record = services
        .find_records
        .iter_mut()
        .flatten()
        .find(|record| record.0 == id)
        .ok_or(Error::Invalid)?;
    record.1 = true;
    Ok(())
}
fn reservation(command: Command) -> bool {
    #[cfg(feature = "find")]
    {
        if let Command::Find(request) = command {
            return request.target == device_content::FindTarget::Phone;
        }
        if matches!(command, Command::FindStop(_) | Command::FindResult { .. }) {
            return true;
        }
    }
    let _ = command;
    false
}
fn success_length(command: Command) -> usize {
    #[cfg(feature = "find")]
    if let Command::Find(_) = command {
        return 8;
    }
    #[cfg(feature = "phone")]
    if let Command::Heartbeat(_) = command {
        return 8;
    }
    let _ = command;
    4
}
impl<B: Backend, const N: usize, const C: usize, const A: usize, const Q: usize> ServiceHandler
    for ContentHandler<'_, '_, B, N, C, A, Q>
{
    fn required_features(&self, entry: &ModuleEntry, opcode: u8) -> Option<u32> {
        (self.entry(entry.module) == Some(entry)
            && entry.major == 1
            && entry.minor == 0
            && opcode_compiled(entry.module, opcode))
        .then_some(0)
    }
    fn authorized(&self, request: Frame<'_>) -> bool {
        if self.closed.get() {
            return false;
        }
        match Command::decode(
            request.header.module,
            request.header.opcode,
            request.payload,
        ) {
            Ok(command) => self.authorize_command(request, command),
            Err(_) => self
                .access
                .allowed(request.header.module, request.header.opcode, None),
        }
    }
    fn must_end_session(&self) -> bool {
        self.live().is_err()
    }
    fn response_ready(&mut self, request: Frame<'_>) -> Result<bool, SessionError> {
        self.touch().map_err(local_error)?;
        if let Some(pending) = self.pending.get() {
            return Ok(pending.module == request.header.module
                && pending.opcode == request.header.opcode
                && pending.request_id == request.header.message_id
                && request.payload == &pending.payload[..pending.payload_len]);
        }
        if self
            .owner
            .with(|services, _| Ok(services.queue_len > 0))
            .map_err(local_error)?
        {
            return Ok(false);
        }
        let Ok(command) = Command::decode(
            request.header.module,
            request.header.opcode,
            request.payload,
        ) else {
            return Ok(true);
        };
        self.owner
            .with(|services, backend| {
                let _ = &backend;
                #[cfg(feature = "gimbal")]
                if !command_ready(services, backend, command)? {
                    return Ok(false);
                }
                if reservation(command) && !services.queue_available() {
                    return Ok(false);
                }
                #[cfg(feature = "find")]
                if let Command::FindStop(id) = command
                    && services
                        .queue
                        .iter()
                        .flatten()
                        .any(|event| event.token.module == 6 && event.token.id == id)
                {
                    return Ok(false);
                }
                Ok(true)
            })
            .map_err(local_error)
    }
    fn replay_valid(&self, request: Frame<'_>, response: &[u8]) -> bool {
        self.live().is_ok()
            && self
                .cached
                .get()
                .is_some_and(|guard| self.replay_guard(guard, request, response))
    }
    fn response_admitted(
        &mut self,
        request: Frame<'_>,
        response: &[u8],
    ) -> Result<(), SessionError> {
        let now = self.touch().map_err(local_error)?;
        if self
            .admitted
            .get()
            .is_some_and(|guard| self.replay_guard(guard, request, response))
        {
            return Ok(());
        }
        let guard = self
            .pending
            .get()
            .ok_or(SessionError::InvalidHandlerResponse)?;
        if !self.replay_guard(guard, request, response) || !self.authorized(request) {
            let _ = self.close_inner();
            return Err(SessionError::AuthorizationRevoked);
        }
        if let Err(error) = self.admit_effect(guard, now) {
            #[cfg(feature = "gimbal")]
            {
                let _ = self.owner.with(|services, _| {
                    mark_uncertain(services, guard.effect);
                    Ok(())
                });
            }
            let _ = self.close_inner();
            return Err(local_error(error));
        }
        if self.live().is_err() || !self.authorized(request) {
            let _ = self.close_inner();
            return Err(SessionError::AuthorizationRevoked);
        }
        self.pending.set(None);
        self.admitted.set(Some(guard));
        Ok(())
    }
    fn connection_closed(&mut self) -> Result<(), SessionError> {
        self.close_inner().map_err(local_error)
    }
    fn dispatch(&mut self, request: Frame<'_>, out: &mut [u8]) -> Result<usize, SessionError> {
        let now = self.touch().map_err(local_error)?;
        if request.header.kind != Kind::Request
            || request.header.message_id == 0
            || request.header.offset != 0
            || usize::from(request.header.total_length) != request.payload.len()
        {
            return Err(SessionError::InvalidFrame);
        }
        if out.len() < 4 {
            return Err(SessionError::Capacity);
        }
        let Some(entry) = self.entry(request.header.module) else {
            return Ok(status(out, Status::UnsupportedModule, Reason::None));
        };
        if self
            .required_features(entry, request.header.opcode)
            .is_none()
        {
            return Ok(status(out, Status::UnsupportedOpcode, Reason::None));
        }
        let command = match Command::decode(
            request.header.module,
            request.header.opcode,
            request.payload,
        ) {
            Ok(command) => command,
            Err(error) => {
                let (status_value, reason) = wire_error(error);
                return Ok(status(out, status_value, reason));
            }
        };
        if !self.authorize_command(request, command) {
            return Ok(status(out, Status::AccessDenied, Reason::RoleRequired));
        }
        if self.pending.get().is_some()
            || self
                .owner
                .with(|services, _| Ok(services.queue_len > 0))
                .map_err(local_error)?
        {
            return Err(SessionError::Deferred);
        }
        if request.payload.len() > usize::from(entry.input_max) {
            return Ok(status(out, Status::InvalidArgument, Reason::None));
        }
        let limit = out.len().min(usize::from(entry.output_max));
        if limit < success_length(command) {
            return Err(SessionError::Capacity);
        }
        let reserved = reservation(command);
        if reserved && let Err(error) = self.owner.with(|services, _| services.reserve()) {
            let (status_value, reason) = wire_error(error);
            return Ok(status(out, status_value, reason));
        }
        let executed = self.execute(request, command, &mut out[..limit], now);
        if self.live().is_err() || !self.authorized(request) {
            out.fill(0);
            let _ = self.close_inner();
            return Err(SessionError::AuthorizationRevoked);
        }
        match executed {
            Ok((length, effect, resource)) => {
                status(out, Status::Ok, Reason::None);
                let mut guard = Guard {
                    module: request.header.module,
                    opcode: request.header.opcode,
                    request_id: request.header.message_id,
                    payload: [0; 11],
                    payload_len: request.payload.len(),
                    response: [0; 8],
                    response_len: length,
                    resource,
                    effect,
                };
                guard.payload[..request.payload.len()].copy_from_slice(request.payload);
                guard.response[..length].copy_from_slice(&out[..length]);
                self.cached.set(Some(guard));
                self.pending.set(Some(guard));
                Ok(length)
            }
            Err(error) => {
                if reserved {
                    let _ = self.owner.with(|services, _| services.release());
                }
                let (status_value, reason) = wire_error(error);
                Ok(status(out, status_value, reason))
            }
        }
    }
}
impl<B: Backend, const N: usize, const C: usize, const A: usize, const Q: usize> Drop
    for ContentHandler<'_, '_, B, N, C, A, Q>
{
    fn drop(&mut self) {
        let _ = self.close_inner();
    }
}
impl<B: Backend, const N: usize, const C: usize, const A: usize, const Q: usize>
    ContentHandler<'_, '_, B, N, C, A, Q>
{
    /// Queue one actually supported device-originated camera/music/SOS/status request.
    /// No backend action occurs until the final event FIFO fragment is confirmed.
    /// # Errors
    /// Rejects local-only action, missing real backend, revoked scope or exhausted ledger/FIFO capacity.
    #[cfg(any(feature = "phone", feature = "media"))]
    pub fn request_action(&mut self, action: device_content::Action) -> Result<u32, Error> {
        self.touch()?;
        let (module, opcode) = match action {
            #[cfg(feature = "media")]
            device_content::Action::CameraShutter => (0x33, 0x80),
            #[cfg(feature = "media")]
            device_content::Action::PhoneMusic(_) => (0x33, 0x81),
            #[cfg(feature = "phone")]
            device_content::Action::Sos { .. } => (7, 0x81),
            #[cfg(feature = "phone")]
            device_content::Action::PhoneStatus => (7, 0x80),
            #[cfg(feature = "media")]
            device_content::Action::CameraMode(_) | device_content::Action::DeviceMusic(_) => {
                return Err(Error::Invalid);
            }
        };
        self.ensure_module(module)?;
        let checked = self.checked(module, opcode, None);
        self.owner.with(|services, backend| {
            #[cfg(feature = "phone")]
            if action == device_content::Action::PhoneStatus
                && let Some(record) = services.action_records.iter().flatten().find(|record| {
                    record.action == action
                        && record.queued
                        && services.phone_actions.state(record.id).is_ok_and(|state| {
                            matches!(
                                state,
                                device_content::ActionState::Prepared
                                    | device_content::ActionState::AwaitingResult
                            )
                        })
                })
            {
                return Ok(record.id);
            }
            #[cfg(feature = "media")]
            if module == 0x33
                && services.action_records.iter().flatten().any(|record| {
                    record.lane == crate::state::Lane::Media
                        && services.media_actions.state(record.id).is_ok_and(|state| {
                            matches!(
                                state,
                                device_content::ActionState::Prepared
                                    | device_content::ActionState::AwaitingResult
                            )
                        })
                })
            {
                return Err(Error::Busy);
            }
            if !services.queue_available() {
                return Err(Error::Busy);
            }
            let id = prepare_action(services, backend, &checked, action, module, opcode)?;
            let mut body = [0; 14];
            let lane = action_lane(action);
            let length = action_ledger(services, lane).event_bytes(id, &mut body)?;
            services.stage(
                EventSpec {
                    module,
                    opcode,
                    id,
                    resource_generation: 0,
                },
                &body[..length],
                Finish::Action { lane, id },
            )?;
            let record = services
                .action_records
                .iter_mut()
                .flatten()
                .find(|record| record.id == id && record.lane == lane)
                .ok_or(Error::Invalid)?;
            record.queued = true;
            Ok(id)
        })
    }
    /// Configure the pure heartbeat monitor only from actual product ranges and trusted policy.
    /// # Errors
    /// Returns invalid/missing ranges, revoked scope or failed external monotonic clock.
    #[cfg(feature = "phone")]
    pub fn configure_heartbeat(
        &mut self,
        policy: device_content::HeartbeatPolicy,
    ) -> Result<(), Error> {
        let now = self.touch()?;
        self.ensure_module(7)?;
        let checked = self.checked(7, 0x30, None);
        self.owner.with(|services, _| {
            services
                .heartbeat
                .configure(policy, &self.binding.manifest, now, &checked)?;
            Ok(())
        })
    }
    /// Settle a coalesced status request only after decoding an actual source-phone P0002 push.
    /// # Errors
    /// Returns malformed typed status, revoked source, wrong event lane or unknown identity.
    #[cfg(feature = "phone")]
    pub fn phone_status_received(&mut self, id: u32, bytes: &[u8]) -> Result<(), Error> {
        self.touch()?;
        self.ensure_module(7)?;
        device_content::decode_property(7, 2, bytes, &self.binding.manifest)?;
        let checked = self.checked(7, 0x80, Some(id));
        self.owner.with(|services, _| {
            let record = services
                .action_records
                .iter()
                .flatten()
                .find(|record| record.id == id && record.module == 7)
                .ok_or(device_content::Error::NotFound)?;
            if record.action != device_content::Action::PhoneStatus {
                return Err(device_content::Error::State.into());
            }
            services.phone_actions.status_received(id, &checked)?;
            Ok(())
        })
    }
    /// Poll actual find timing and queue E81 only when its immutable terminal result exists.
    /// A full FIFO retains the terminal result for a later retry.
    /// # Errors
    /// Returns revoked scope, unknown identity, monotonic clock failure, FIFO busy or actual backend failure.
    #[cfg(feature = "find")]
    pub fn poll_find(&mut self, id: u32) -> Result<device_content::FindState, Error> {
        let now = self.touch()?;
        self.ensure_module(6)?;
        let result = self.owner.with(|services, backend| {
            let result = services.find.poll(id, now, backend.find());
            let state = find_backend_result(services, id, result)?;
            if matches!(state, device_content::FindState::Terminal(_)) {
                stage_find_terminal(services, id, false)?;
            }
            Ok(state)
        });
        self.close_uncertain_find();
        result
    }
    #[cfg(feature = "find")]
    fn close_uncertain_find(&self) {
        if self
            .owner
            .with(|services, _| Ok(services.find_uncertain.is_some()))
            .is_ok_and(core::convert::identity)
        {
            let _ = self.close_inner();
        }
    }
    /// Confirm a real local find observation and retain its result event until FIFO admission.
    /// # Errors
    /// Returns unknown/wrong target, actual backend stop failure, revoked access or a full FIFO.
    #[cfg(feature = "find")]
    pub fn found_device(
        &mut self,
        id: u32,
        result: device_content::FindResult,
    ) -> Result<(), Error> {
        self.touch()?;
        self.ensure_module(6)?;
        let checked = self.checked(6, 0x81, Some(id));
        checked.authorize(self.binding.scope, device_content::Operation::Find)?;
        let outcome = self.owner.with(|services, backend| {
            let result = services.find.device_result(id, result, backend.find());
            find_backend_result(services, id, result)?;
            stage_find_terminal(services, id, false)
        });
        self.close_uncertain_find();
        outcome
    }
    /// Poll an actual gimbal ticket; generate E82 once and retain it across FIFO retries.
    /// # Errors
    /// Returns invalid lifecycle, unknown identity, revoked access, full FIFO or actual backend failure.
    #[cfg(feature = "gimbal")]
    pub fn poll_gimbal(&mut self, id: u32) -> Result<device_content::GimbalState, Error> {
        self.touch()?;
        self.ensure_module(51)?;
        let checked = self.checked(0x33, 0x34, Some(id));
        let result = self.owner.with(|services, backend| {
            let before = services.gimbal.state(id)?;
            let state = match services.gimbal.poll(id, &checked, backend.gimbal()) {
                Ok(state) => state,
                Err(error) => {
                    if before == device_content::GimbalState::Running
                        && let Some(record) = services
                            .gimbal_records
                            .iter_mut()
                            .flatten()
                            .find(|record| record.id == id)
                    {
                        record.uncertain = true;
                    }
                    return Err(error.into());
                }
            };
            if matches!(state, device_content::GimbalState::Terminal { .. }) {
                let queued = services
                    .gimbal_records
                    .iter()
                    .flatten()
                    .find(|record| record.id == id)
                    .ok_or(device_content::Error::NotFound)?
                    .queued;
                if !queued {
                    let mut body = [0; 7];
                    services.gimbal.result_event(id, &mut body)?;
                    services.stage(
                        EventSpec {
                            module: 0x33,
                            opcode: 0x82,
                            id,
                            resource_generation: 0,
                        },
                        &body,
                        Finish::None,
                    )?;
                    let record = services
                        .gimbal_records
                        .iter_mut()
                        .flatten()
                        .find(|record| record.id == id)
                        .ok_or(Error::Invalid)?;
                    record.queued = true;
                }
            }
            Ok(state)
        });
        let uncertain = match self.owner.with(|services, _| {
            Ok(services
                .gimbal_records
                .iter()
                .flatten()
                .any(|record| record.id == id && record.uncertain))
        }) {
            Ok(uncertain) => uncertain,
            Err(error) => {
                let _ = self.close_inner();
                return Err(error);
            }
        };
        if uncertain {
            let _ = self.close_inner();
        }
        result
    }
    /// Poll actual independent radio readback, atomically publish to the real shared property store,
    /// and queue frozen generic E90 only with the store's actual revision/generation.
    /// # Errors
    /// Returns revoked source, absent/failed real ports, failed storage, invalid revision or full FIFO.
    #[cfg(feature = "companion-radio")]
    pub fn poll_radio(&mut self) -> Result<device_content::CompanionRadioState, Error> {
        use crate::{ClassicPort, RadioStateStore};
        self.touch()?;
        self.ensure_module(8)?;
        let checked = self.checked(8, 0x30, None);
        self.owner.with(|services, backend| {
            if !services.queue_available()
                || services
                    .queue
                    .iter()
                    .flatten()
                    .any(|event| event.token.module == 8 && event.token.opcode == 0x90)
            {
                return Err(Error::Busy);
            }
            let before = services.radio.state();
            if before == device_content::CompanionRadioState::Prepared {
                return Err(Error::Busy);
            }
            let state = services.radio.poll(&checked, backend.radio())?;
            let value = if before == device_content::CompanionRadioState::Switching {
                services.radio.readback()?
            } else {
                backend.radio().observe(services.scope)?
            };
            let mut encoded = [0; 9];
            device_content::encode_property(
                device_content::Property::ClassicBluetooth(value),
                &mut encoded,
                &device_content::Manifest {
                    independent_call_bluetooth: true,
                    ..device_content::Manifest::default()
                },
            )?;
            let snapshot = backend.radio_state().publish(services.scope, value)?;
            if snapshot.revision == 0
                || snapshot.generation == 0
                || !self
                    .access
                    .radio_current(snapshot.revision, snapshot.generation)
            {
                return Err(Error::Denied);
            }
            if services.radio_snapshot != Some(snapshot) {
                let mut body = [0; 6];
                body[..2].copy_from_slice(&1u16.to_le_bytes());
                body[2..].copy_from_slice(&snapshot.revision.to_le_bytes());
                services.stage(
                    EventSpec {
                        module: 8,
                        opcode: 0x90,
                        id: snapshot.revision,
                        resource_generation: snapshot.generation,
                    },
                    &body,
                    Finish::None,
                )?;
                services.radio_snapshot = Some(snapshot);
            }
            Ok(state)
        })
    }
}
#[cfg(feature = "notifications")]
struct CommitAccess<'a> {
    access: &'a dyn ContentAccess,
    scope: Scope,
    object: u32,
    notification: u32,
}
#[cfg(feature = "notifications")]
impl core::fmt::Debug for CommitAccess<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CommitAccess").finish_non_exhaustive()
    }
}
#[cfg(feature = "notifications")]
impl AccessPort for CommitAccess<'_> {
    fn authorize(
        &self,
        scope: Scope,
        operation: device_content::Operation,
    ) -> Result<(), device_content::Error> {
        if scope != self.scope
            || !self
                .access
                .notification_commit_allowed(self.object, self.notification)
        {
            return Err(device_content::Error::Denied);
        }
        self.access.authorize(scope, operation)
    }
}
#[cfg(feature = "notifications")]
impl<'data, B: Backend, const N: usize, const C: usize, const A: usize, const Q: usize>
    ContentHandler<'_, 'data, B, N, C, A, Q>
{
    /// Stage actual committed schema2 notification bytes retained by the caller's stable storage view.
    /// The object's durable COMMIT and FIFO response admission are coordinated by its object handler.
    /// # Errors
    /// Returns malformed object, unavailable limits/backend, source/actual-generation denial or capacity.
    pub fn stage_notification_bytes(
        &mut self,
        object: u32,
        bytes: &'data [u8],
        generation: u64,
    ) -> Result<device_content::NotificationChange, Error> {
        self.touch()?;
        self.ensure_module(48)?;
        if object == 0 || generation == 0 {
            return Err(Error::Invalid);
        }
        let device_content::Object::Notification(value) =
            device_content::decode_object(0x30, 1, bytes, &self.binding.manifest)?
        else {
            return Err(Error::Invalid);
        };
        if !self
            .access
            .notification_current(value.notification, generation)
        {
            return Err(Error::Denied);
        }
        let checked = CommitAccess {
            access: self.access,
            scope: self.binding.scope,
            object,
            notification: value.notification,
        };
        self.owner.with(|services, backend| {
            let existing = services
                .notification_records
                .iter()
                .flatten()
                .find(|record| record.object == object)
                .copied();
            if existing.is_none() && !services.notification_records.iter().any(Option::is_none) {
                return Err(Error::Capacity);
            }
            let change = services.notifications.register(
                object,
                value,
                &checked,
                backend.notifications(),
            )?;
            if let Some(record) = services
                .notification_records
                .iter_mut()
                .flatten()
                .find(|record| record.object == object)
            {
                record.generation = generation;
                record.presented =
                    change == device_content::NotificationChange::Unchanged && record.presented;
            } else {
                let slot = services
                    .notification_records
                    .iter_mut()
                    .find(|record| record.is_none())
                    .ok_or(Error::Capacity)?;
                *slot = Some(crate::state::NotificationRecord {
                    object,
                    notification: value.notification,
                    generation,
                    presented: false,
                });
            }
            Ok(change)
        })
    }
    /// Present a committed notification only after its complete successful COMMIT response entered
    /// the same real transport FIFO. Exact retry does not present or alert twice.
    /// # Errors
    /// Returns stale/untrusted source, unknown object or actual presentation failure; failure closes.
    pub fn notification_response_admitted(&mut self, object: u32) -> Result<(), Error> {
        self.touch()?;
        self.ensure_module(48)?;
        let result = self.owner.with(|services, backend| {
            let record = services
                .notification_records
                .iter()
                .flatten()
                .find(|record| record.object == object)
                .copied()
                .ok_or(device_content::Error::NotFound)?;
            if !self
                .access
                .notification_current(record.notification, record.generation)
            {
                return Err(Error::Denied);
            }
            let checked = CommitAccess {
                access: self.access,
                scope: self.binding.scope,
                object,
                notification: record.notification,
            };
            services.notifications.content_response_admitted(
                object,
                &checked,
                backend.notifications(),
            )?;
            let record = services
                .notification_records
                .iter_mut()
                .flatten()
                .find(|record| record.object == object)
                .ok_or(Error::Invalid)?;
            record.presented = true;
            Ok(())
        });
        if matches!(
            result,
            Err(Error::Denied
                | Error::Content(
                    device_content::Error::Backend
                        | device_content::Error::Denied
                        | device_content::Error::Unavailable
                ))
        ) {
            let _ = self.close_inner();
        }
        result
    }
    /// Queue one supported incoming-call action for the authenticated source notification.
    /// Existing pending/terminal tuples never emit a duplicate action event.
    /// # Errors
    /// Returns missing call, unsupported action, stale object, untrusted source or full FIFO/ledger.
    pub fn request_notification_action(
        &mut self,
        notification: u32,
        action: u8,
    ) -> Result<(), Error> {
        self.touch()?;
        self.ensure_module(0x30)?;
        if !(1..=3).contains(&action) {
            return Err(Error::EnumUnsupported);
        }
        let checked = self.checked(0x30, 0x82, Some(notification));
        self.owner.with(|services,backend|{
  let record=services.notification_records.iter().flatten().find(|record|record.notification==notification).copied().ok_or(device_content::Error::NotFound)?;
  if !record.presented{return Err(device_content::Error::State.into());}
  if !self.access.notification_current(notification,record.generation){return Err(Error::Denied);}
  checked.authorize(services.scope,device_content::Operation::Notification)?;
  match services.notifications.action_state(notification,action){
   Ok(device_content::NotificationActionState::AwaitingResult|device_content::NotificationActionState::Settled(_))=>return Ok(()),
   Ok(device_content::NotificationActionState::Failed|device_content::NotificationActionState::Unknown)=>return Err(Error::Backend),
   Ok(device_content::NotificationActionState::Prepared)=>{
    let queued=services.queue.iter().flatten().any(|event|matches!(event.finish,Finish::Notification{notification:old,action:old_action} if old==notification&&old_action==action));
    if queued{return Ok(());}
   },
   Err(device_content::Error::NotFound)=>{},
   Err(error)=>return Err(error.into()),
  }
  if !services.queue_available(){return Err(Error::Busy);}
  let watched=services.watches.iter().flatten().any(|watch|watch.module==0x30&&watch.id==notification);
  if !services.watch_available()&&!watched{return Err(Error::Capacity);}
  services.notifications.prepare_action(notification,action,&checked,backend.notifications())?;
  let mut body=[0;5];services.notifications.action_event(notification,action,&mut body)?;
  services.stage(EventSpec{module:0x30,opcode:0x82,id:notification,resource_generation:record.generation},&body,Finish::Notification{notification,action})?;
  services.watch(Watch{module:0x30,opcode:0x82,id:notification,resource_generation:record.generation})?;
  Ok(())
 })
    }
    /// Check whether an object's started/pending event and response have drained before durable DELETE.
    /// # Errors
    /// Returns revoked scope or unknown object. False is a local prerequisite, never a wire Pending.
    pub fn notification_delete_ready(&self, object: u32) -> Result<bool, Error> {
        self.live()?;
        self.ensure_module(0x30)?;
        self.owner.with(|services, _| {
            let record = services
                .notification_records
                .iter()
                .flatten()
                .find(|record| record.object == object)
                .ok_or(device_content::Error::NotFound)?;
            Ok(record.presented
                && !services.queue.iter().flatten().any(|event| {
                    event.token.module == 0x30 && event.token.id == record.notification
                })
                && self.pending.get().is_none_or(|guard| {
                    guard
                        .resource
                        .is_none_or(|resource| resource.0 != record.notification)
                }))
        })
    }
    /// Apply an actual durable object deletion after its reply is FIFO-admitted; clear call ledger
    /// and withdraw real UI content even though the old store generation is now absent.
    /// # Errors
    /// Rejects wrong source, undrained action, unknown object or actual backend withdrawal failure.
    pub fn notification_deleted(&mut self, object: u32) -> Result<(), Error> {
        if self.closed.get()
            || self.access.scope() != Ok(self.binding.scope)
            || self.access.generation() != Ok(self.generation)
        {
            let _ = self.close_inner();
            return Err(Error::Denied);
        }
        self.ensure_module(0x30)?;
        let result =
            self.owner.with(|services, backend| {
                let record = services
                    .notification_records
                    .iter()
                    .flatten()
                    .find(|record| record.object == object)
                    .copied()
                    .ok_or(device_content::Error::NotFound)?;
                if services.queue.iter().flatten().any(|event| {
                    event.token.module == 0x30 && event.token.id == record.notification
                }) {
                    return Err(Error::Busy);
                }
                let checked = CommitAccess {
                    access: self.access,
                    scope: self.binding.scope,
                    object,
                    notification: record.notification,
                };
                services.notifications.delete_response_admitted(
                    object,
                    &checked,
                    backend.notifications(),
                )?;
                for slot in &mut services.notification_records {
                    if slot.is_some_and(|value| value.object == object) {
                        *slot = None;
                    }
                }
                for watch in &mut services.watches {
                    if watch.is_some_and(|watch| {
                        watch.module == 0x30 && watch.id == record.notification
                    }) {
                        *watch = None;
                    }
                }
                Ok(())
            });
        if matches!(
            result,
            Err(
                Error::Content(device_content::Error::Backend | device_content::Error::Denied)
                    | Error::Denied
            )
        ) {
            let _ = self.close_inner();
        }
        result
    }
}

#[cfg(feature = "media")]
fn local_action<B: Backend, const N: usize, const C: usize, const A: usize, const Q: usize>(
    services: &mut Services<'_, N, C, A, Q>,
    backend: &mut B,
    access: &impl AccessPort,
    action: device_content::Action,
    opcode: u8,
) -> Result<Executed, Error> {
    let id = prepare_action(services, backend, access, action, 0x33, opcode)?;
    Ok((
        4,
        Effect::Action {
            lane: crate::state::Lane::Local,
            id,
        },
        None,
    ))
}
#[cfg(feature = "find")]
fn execute_find<B: Backend, const N: usize, const C: usize, const A: usize, const Q: usize>(
    services: &mut Services<'_, N, C, A, Q>,
    backend: &mut B,
    access: &impl AccessPort,
    request: device_content::FindRequest,
    out: &mut [u8],
) -> Result<Executed, Error> {
    if !services.watch_available() {
        return Err(Error::Capacity);
    }
    let id = services.find.prepare(request, access, backend.find())?;
    let record = services
        .find_records
        .iter_mut()
        .find(|record| record.is_none())
        .ok_or(Error::Capacity)?;
    *record = Some((id, false));
    services.watch(Watch {
        module: 6,
        opcode: 0x30,
        id,
        resource_generation: 0,
    })?;
    out[4..8].copy_from_slice(&id.to_le_bytes());
    Ok((
        8,
        Effect::Start {
            id,
            phone: request.target == device_content::FindTarget::Phone,
        },
        None,
    ))
}
#[cfg(feature = "gimbal")]
fn execute_gimbal<B: Backend, const N: usize, const C: usize, const A: usize, const Q: usize>(
    services: &mut Services<'_, N, C, A, Q>,
    backend: &mut B,
    access: &impl AccessPort,
    request: device_content::GimbalRequest,
) -> Result<Executed, Error> {
    let known = services.gimbal.state(request.action_id).is_ok();
    if !known && !services.watch_available() {
        return Err(Error::Capacity);
    }
    services.gimbal.prepare(request, access, backend.gimbal())?;
    if !known {
        let record = services
            .gimbal_records
            .iter_mut()
            .find(|record| record.is_none())
            .ok_or(Error::Capacity)?;
        *record = Some(crate::state::GimbalRecord {
            id: request.action_id,
            queued: false,
            uncertain: false,
        });
        services.watch(Watch {
            module: 0x33,
            opcode: 0x34,
            id: request.action_id,
            resource_generation: 0,
        })?;
    }
    Ok((4, Effect::Gimbal(request.action_id), None))
}

#[cfg(any(
    feature = "phone",
    feature = "media",
    feature = "find",
    feature = "notifications"
))]
fn terminal_error(error: device_content::Error) -> Error {
    if error == device_content::Error::Conflict {
        Error::TerminalConflict
    } else {
        error.into()
    }
}

#[cfg(feature = "companion-radio")]
fn execute_radio<B: Backend, const N: usize, const C: usize, const A: usize, const Q: usize>(
    services: &mut Services<'_, N, C, A, Q>,
    backend: &mut B,
    access: &impl AccessPort,
    enabled: bool,
) -> Result<Executed, Error> {
    if !services.watch_available()
        && !services
            .watches
            .iter()
            .flatten()
            .any(|watch| watch.module == 8)
    {
        return Err(Error::Capacity);
    }

    services.radio.prepare(enabled, access, backend.radio())?;
    if !services
        .watches
        .iter()
        .flatten()
        .any(|watch| watch.module == 8)
    {
        services.watch(Watch {
            module: 8,
            opcode: 0x30,
            id: 0,
            resource_generation: 0,
        })?;
    }
    Ok((4, Effect::Radio, None))
}

#[cfg(any(feature = "phone", feature = "media"))]
fn coalesces<const N: usize, const C: usize, const A: usize, const Q: usize>(
    services: &Services<'_, N, C, A, Q>,
    action: device_content::Action,
) -> bool {
    match action {
        #[cfg(feature = "media")]
        device_content::Action::CameraMode(_) => services
            .action_records
            .iter()
            .flatten()
            .rev()
            .find(|record| {
                record.lane == crate::state::Lane::Local
                    && matches!(record.action, device_content::Action::CameraMode(_))
            })
            .is_some_and(|record| {
                record.action == action
                    && services.local_actions.state(record.id).is_ok_and(|state| {
                        !matches!(
                            state,
                            device_content::ActionState::Failed
                                | device_content::ActionState::Unknown
                        )
                    })
            }),
        #[cfg(feature = "phone")]
        device_content::Action::PhoneStatus => {
            services.action_records.iter().flatten().any(|record| {
                record.action == action
                    && record.lane == crate::state::Lane::Phone
                    && services.phone_actions.state(record.id).is_ok_and(|state| {
                        matches!(
                            state,
                            device_content::ActionState::Prepared
                                | device_content::ActionState::AwaitingResult
                        )
                    })
            })
        }
        #[cfg(feature = "phone")]
        device_content::Action::Sos { .. } => false,
        #[cfg(feature = "media")]
        device_content::Action::CameraShutter
        | device_content::Action::PhoneMusic(_)
        | device_content::Action::DeviceMusic(_) => false,
    }
}

fn watch_operation(module: u8) -> device_content::Operation {
    match module {
        6 => device_content::Operation::Find,
        0x30 => device_content::Operation::Notification,
        _ => device_content::Operation::Submit,
    }
}

#[cfg(feature = "find")]
fn find_backend_result<T, const N: usize, const C: usize, const A: usize, const Q: usize>(
    services: &mut Services<'_, N, C, A, Q>,
    id: u32,
    result: Result<T, device_content::Error>,
) -> Result<T, Error> {
    if result.is_err()
        && services.find.state(id)
            == Ok(device_content::FindState::Terminal(
                device_content::FindResult::BackendFailure,
            ))
    {
        services.find_uncertain = Some(id);
    }
    result.map_err(Error::from)
}

fn cleanup_services<B: Backend, const N: usize, const C: usize, const A: usize, const Q: usize>(
    services: &mut Services<'_, N, C, A, Q>,
    backend: &mut B,
) -> CleanupReport {
    let mut report = CleanupReport::default();
    let _ = &backend;

    services.queue = [None; Q];
    services.queue_len = 0;
    services.reserved = 0;
    services.watches = [None; N];
    #[cfg(feature = "phone")]
    {
        report.unknown_count += services.phone_actions.invalidate().len();
    }
    #[cfg(feature = "media")]
    {
        report.unknown_count +=
            services.media_actions.invalidate().len() + services.local_actions.invalidate().len();
    }
    #[cfg(feature = "phone")]
    services.heartbeat.invalidate();
    #[cfg(feature = "find")]
    {
        report.unknown_count += services
            .find_records
            .iter()
            .flatten()
            .filter(|record| {
                services.find_uncertain == Some(record.0)
                    || services.find.state(record.0).is_ok_and(|state| {
                        matches!(
                            state,
                            device_content::FindState::Prepared
                                | device_content::FindState::Active
                                | device_content::FindState::StopPrepared
                        )
                    })
            })
            .count();
        if services.find.invalidate(backend.find()).is_err() {
            report.failed = true;
        }
        if let Some(id) = services.find_uncertain
            && device_content::FindPort::stop(backend.find(), services.scope, id).is_err()
        {
            report.failed = true;
        }
    }
    #[cfg(feature = "notifications")]
    {
        report.unknown_count += services.notifications.invalidate().len();
        services.notification_records = [None; C];
    }
    #[cfg(feature = "companion-radio")]
    {
        if matches!(
            services.radio.state(),
            device_content::CompanionRadioState::Prepared
                | device_content::CompanionRadioState::Switching
        ) {
            report.unknown_count += 1;
        }
        if services.radio.invalidate(backend.radio()).is_err() {
            report.failed = true;
        }
    }
    #[cfg(feature = "gimbal")]
    {
        use crate::GimbalLifecyclePort;
        report.unknown_count += services
            .gimbal_records
            .iter()
            .flatten()
            .filter(|record| {
                record.uncertain
                    || services.gimbal.state(record.id).is_ok_and(|state| {
                        matches!(
                            state,
                            device_content::GimbalState::Prepared
                                | device_content::GimbalState::Running
                        )
                    })
            })
            .count();
        services.gimbal_needs_drain = services.gimbal_records.iter().flatten().any(|record| {
            record.uncertain
                || services.gimbal.state(record.id) == Ok(device_content::GimbalState::Running)
        });
        if services.gimbal.invalidate(backend.gimbal()).is_err() {
            report.failed = true;
        }
        if services.gimbal_needs_drain && backend.gimbal().cancel_scope(services.scope).is_err() {
            report.failed = true;
        }
    }
    report
}

#[cfg(feature = "gimbal")]
fn command_ready<B: Backend, const N: usize, const C: usize, const A: usize, const Q: usize>(
    services: &Services<'_, N, C, A, Q>,
    backend: &mut B,
    command: Command,
) -> Result<bool, Error> {
    use crate::GimbalLifecyclePort;
    match command {
        Command::Gimbal(request) => match backend.gimbal().ready(services.scope, request) {
            Ok(ready) => Ok(ready),
            Err(device_content::Error::Unavailable) => Ok(true),
            Err(error) => Err(error.into()),
        },
        #[cfg(any(
            feature = "phone",
            feature = "media",
            feature = "find",
            feature = "notifications",
            feature = "companion-radio"
        ))]
        _ => Ok(true),
    }
}
#[cfg(feature = "gimbal")]
fn mark_uncertain<const N: usize, const C: usize, const A: usize, const Q: usize>(
    services: &mut Services<'_, N, C, A, Q>,
    effect: Effect,
) {
    match effect {
        Effect::Gimbal(id) => {
            if let Some(record) = services
                .gimbal_records
                .iter_mut()
                .flatten()
                .find(|record| record.id == id)
            {
                record.uncertain = true;
            }
        }
        #[cfg(any(
            feature = "phone",
            feature = "media",
            feature = "find",
            feature = "notifications",
            feature = "companion-radio"
        ))]
        _ => {}
    }
}
#[cfg(feature = "phone")]
impl<B: Backend, const N: usize, const C: usize, const A: usize, const Q: usize>
    ContentHandler<'_, '_, B, N, C, A, Q>
{
    /// Poll liveness using the actual external monotonic clock, without triggering a fabricated action.
    /// # Errors
    /// Returns revoked scope, omitted module or a missing/reversed clock.
    pub fn poll_heartbeat(&mut self) -> Result<device_content::HeartbeatState, Error> {
        let now = self.touch()?;
        self.ensure_module(7)?;
        self.owner
            .with(|services, _| services.heartbeat.poll(now).map_err(Error::from))
    }
}

#[cfg(feature = "find")]
impl<B: Backend, const N: usize, const C: usize, const A: usize, const Q: usize>
    ContentHandler<'_, '_, B, N, C, A, Q>
{
    /// Queue an actual device/UI-originated FIND request with its exact requested pattern.
    /// Backend capability validation precedes queueing; actual start waits for final E80 admission.
    /// # Errors
    /// Rejects omitted/unauthorized scope, unsupported real patterns/duration, active find or full FIFO/ledger.
    pub fn request_find(&mut self, request: device_content::FindRequest) -> Result<u32, Error> {
        self.touch()?;
        self.ensure_module(6)?;
        let checked = self.checked(6, 0x80, None);
        self.owner.with(|services, backend| {
            if !services.queue_available() {
                return Err(Error::Busy);
            }
            if !services.watch_available() {
                return Err(Error::Capacity);
            }
            let id = services.find.prepare(request, &checked, backend.find())?;
            let record = services
                .find_records
                .iter_mut()
                .find(|record| record.is_none())
                .ok_or(Error::Capacity)?;
            *record = Some((id, false));
            services.watch(Watch {
                module: 6,
                opcode: 0x80,
                id,
                resource_generation: 0,
            })?;
            let mut body = [0; 8];
            services.find.request_event(id, &mut body)?;
            services.stage(
                EventSpec {
                    module: 6,
                    opcode: 0x80,
                    id,
                    resource_generation: 0,
                },
                &body,
                Finish::Find(id),
            )?;
            Ok(id)
        })
    }
}

#[cfg(feature = "event-pump")]
impl<B: Backend, const N: usize, const C: usize, const A: usize, const Q: usize>
    ContentHandler<'_, '_, B, N, C, A, Q>
{
    pub(crate) fn pending_event_route(&self) -> Result<Option<(u8, u8)>, Error> {
        self.live()?;
        if self.pending.get().is_some() {
            return Ok(None);
        }
        self.owner.with(|services, _| {
            if services.queue_len == 0 {
                return Ok(None);
            }
            let event = services.front()?;
            if !self.event_authorized(event.token) {
                return Err(Error::Denied);
            }
            Ok(Some((event.token.module, event.token.opcode)))
        })
    }
}
