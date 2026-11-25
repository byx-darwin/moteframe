use crate::{Backend, Error, EventToken};
use core::cell::{Cell, RefCell};
use device_content::Scope;

#[derive(Debug, Clone, Copy)]
pub(crate) struct Watch {
    pub module: u8,
    pub opcode: u8,
    pub id: u32,
    pub resource_generation: u64,
}
#[derive(Debug, Clone, Copy)]
pub(crate) enum Finish {
    #[cfg(any(feature = "find", feature = "gimbal", feature = "companion-radio"))]
    None,
    #[cfg(any(feature = "phone", feature = "media"))]
    Action { lane: Lane, id: u32 },
    #[cfg(feature = "find")]
    Find(u32),
    #[cfg(feature = "notifications")]
    Notification { notification: u32, action: u8 },
}
#[derive(Clone, Copy)]
pub(crate) struct Queued {
    pub token: EventToken,
    pub bytes: [u8; 32],
    pub len: usize,
    pub finish: Finish,
}
impl core::fmt::Debug for Queued {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Queued")
            .field("length", &self.len)
            .finish_non_exhaustive()
    }
}
#[cfg(any(feature = "phone", feature = "media"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lane {
    #[cfg(feature = "phone")]
    Phone,
    #[cfg(feature = "media")]
    Media,
    #[cfg(feature = "media")]
    Local,
}
#[cfg(any(feature = "phone", feature = "media"))]
#[derive(Debug, Clone, Copy)]
pub(crate) struct ActionRecord {
    pub id: u32,
    pub lane: Lane,
    pub action: device_content::Action,
    pub module: u8,
    pub opcode: u8,
    pub queued: bool,
}
#[cfg(feature = "notifications")]
#[derive(Debug, Clone, Copy)]
pub(crate) struct NotificationRecord {
    pub object: u32,
    pub notification: u32,
    pub generation: u64,
    pub presented: bool,
}
#[cfg(feature = "gimbal")]
#[derive(Debug, Clone, Copy)]
pub(crate) struct GimbalRecord {
    pub id: u32,
    pub queued: bool,
    pub uncertain: bool,
}
/// Cleanup observations retained by the shared owner even if an adapter is dropped.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CleanupReport {
    /// Number of unconfirmed action/find/notification/gimbal operations now Unknown.
    pub unknown_count: usize,
    /// A real backend cleanup failed; this owner refuses a new connection.
    pub failed: bool,
}
pub(crate) struct Services<'data, const N: usize, const C: usize, const A: usize, const Q: usize> {
    pub scope: Scope,
    pub generation: u64,
    pub watches: [Option<Watch>; N],
    pub queue: [Option<Queued>; Q],
    pub queue_len: usize,
    pub reserved: usize,
    pub next_event: u64,
    pub marker: core::marker::PhantomData<&'data ()>,
    #[cfg(feature = "phone")]
    pub phone_actions: device_content::ActionService<N>,
    #[cfg(feature = "media")]
    pub media_actions: device_content::ActionService<N>,
    #[cfg(feature = "media")]
    pub local_actions: device_content::ActionService<N>,
    #[cfg(any(feature = "phone", feature = "media"))]
    pub action_records: [Option<ActionRecord>; N],
    #[cfg(feature = "phone")]
    pub heartbeat: device_content::HeartbeatMonitor,
    #[cfg(feature = "find")]
    pub find: device_content::FindService<N>,
    #[cfg(feature = "find")]
    pub find_records: [Option<(u32, bool)>; N],
    #[cfg(feature = "find")]
    pub find_uncertain: Option<u32>,
    #[cfg(feature = "notifications")]
    pub notifications: device_content::NotificationService<'data, C, A>,
    #[cfg(feature = "notifications")]
    pub notification_records: [Option<NotificationRecord>; C],
    #[cfg(feature = "companion-radio")]
    pub radio: device_content::CompanionRadioService,
    #[cfg(feature = "companion-radio")]
    pub radio_snapshot: Option<crate::PropertySnapshot>,
    #[cfg(feature = "gimbal")]
    pub gimbal: device_content::GimbalService<N>,
    #[cfg(feature = "gimbal")]
    pub gimbal_records: [Option<GimbalRecord>; N],
    #[cfg(feature = "gimbal")]
    pub gimbal_needs_drain: bool,
}
impl<const N: usize, const C: usize, const A: usize, const Q: usize> Services<'_, N, C, A, Q> {
    pub fn new(scope: Scope, generation: u64) -> Self {
        Self {
            scope,
            generation,
            watches: [None; N],
            queue: [None; Q],
            queue_len: 0,
            reserved: 0,
            next_event: 1,
            marker: core::marker::PhantomData,
            #[cfg(feature = "phone")]
            phone_actions: device_content::ActionService::new(scope),
            #[cfg(feature = "media")]
            media_actions: device_content::ActionService::new(scope),
            #[cfg(feature = "media")]
            local_actions: device_content::ActionService::new(scope),
            #[cfg(any(feature = "phone", feature = "media"))]
            action_records: [None; N],
            #[cfg(feature = "phone")]
            heartbeat: device_content::HeartbeatMonitor::new(scope),
            #[cfg(feature = "find")]
            find: device_content::FindService::new(scope),
            #[cfg(feature = "find")]
            find_records: [None; N],
            #[cfg(feature = "find")]
            find_uncertain: None,
            #[cfg(feature = "notifications")]
            notifications: device_content::NotificationService::new(scope),
            #[cfg(feature = "notifications")]
            notification_records: [None; C],
            #[cfg(feature = "companion-radio")]
            radio: device_content::CompanionRadioService::new(scope),
            #[cfg(feature = "companion-radio")]
            radio_snapshot: None,
            #[cfg(feature = "gimbal")]
            gimbal: device_content::GimbalService::new(scope),
            #[cfg(feature = "gimbal")]
            gimbal_records: [None; N],
            #[cfg(feature = "gimbal")]
            gimbal_needs_drain: false,
        }
    }
    pub fn queue_available(&self) -> bool {
        self.queue_len + self.reserved < Q
    }
    pub fn reserve(&mut self) -> Result<(), Error> {
        if !self.queue_available() {
            return Err(Error::Busy);
        }
        self.reserved += 1;
        Ok(())
    }
    pub fn release(&mut self) -> Result<(), Error> {
        self.reserved = self.reserved.checked_sub(1).ok_or(Error::Invalid)?;
        Ok(())
    }
    #[cfg(feature = "find")]
    pub fn stage_reserved(
        &mut self,
        event: EventSpec,
        body: &[u8],
        finish: Finish,
    ) -> Result<EventToken, Error> {
        self.release()?;
        self.stage(event, body, finish)
    }
    pub fn stage(
        &mut self,
        event: EventSpec,
        body: &[u8],
        finish: Finish,
    ) -> Result<EventToken, Error> {
        if !self.queue_available() {
            return Err(Error::Busy);
        }
        let len = body.len();
        if len > 32 {
            return Err(Error::Capacity);
        }
        let next = self.next_event.checked_add(1).ok_or(Error::Capacity)?;
        let token = EventToken {
            connection: self.scope.connection(),
            generation: self.generation,
            sequence: self.next_event,
            module: event.module,
            opcode: event.opcode,
            id: event.id,
            resource_generation: event.resource_generation,
        };
        let mut bytes = [0; 32];
        bytes[..body.len()].copy_from_slice(body);
        self.queue[self.queue_len] = Some(Queued {
            token,
            bytes,
            len,
            finish,
        });
        self.queue_len += 1;
        self.next_event = next;
        Ok(token)
    }
    pub fn front(&self) -> Result<Queued, Error> {
        self.queue
            .first()
            .copied()
            .flatten()
            .ok_or(Error::Content(device_content::Error::NotFound))
    }
    pub fn pop(&mut self) -> Result<(), Error> {
        if self.queue_len == 0 {
            return Err(Error::Invalid);
        }
        for index in 1..self.queue_len {
            self.queue[index - 1] = self.queue[index];
        }
        self.queue_len -= 1;
        self.queue[self.queue_len] = None;
        Ok(())
    }
    pub fn watch_available(&self) -> bool {
        self.watches.iter().any(Option::is_none)
    }
    pub fn watch(&mut self, watch: Watch) -> Result<(), Error> {
        if self.watches.iter().flatten().any(|old| {
            old.module == watch.module && old.opcode == watch.opcode && old.id == watch.id
        }) {
            return Ok(());
        }
        let slot = self
            .watches
            .iter_mut()
            .find(|slot| slot.is_none())
            .ok_or(Error::Capacity)?;
        *slot = Some(watch);
        Ok(())
    }
}
#[derive(Debug, Clone, Copy)]
pub(crate) struct EventSpec {
    pub module: u8,
    pub opcode: u8,
    pub id: u32,
    pub resource_generation: u64,
}
pub(crate) struct State<'data, B, const N: usize, const C: usize, const A: usize, const Q: usize> {
    pub backend: B,
    pub services: Option<Services<'data, N, C, A, Q>>,
}
/// One boot-global owner for all selected content modules, action identities and event FIFO.
/// Exactly one physical App connection can borrow this owner at a time.
pub struct SharedContentOwner<
    'data,
    B,
    const N: usize,
    const C: usize,
    const A: usize,
    const Q: usize,
> {
    pub(crate) state: RefCell<State<'data, B, N, C, A, Q>>,
    pub(crate) active: Cell<Option<Scope>>,
    pub(crate) last_connection: Cell<u64>,
    pub(crate) report: Cell<CleanupReport>,
}
impl<B, const N: usize, const C: usize, const A: usize, const Q: usize> core::fmt::Debug
    for SharedContentOwner<'_, B, N, C, A, Q>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SharedContentOwner")
            .field("ledger_capacity", &N)
            .field("queue_capacity", &Q)
            .finish_non_exhaustive()
    }
}
impl<'data, B: Backend, const N: usize, const C: usize, const A: usize, const Q: usize>
    SharedContentOwner<'data, B, N, C, A, Q>
{
    /// Install the actual selected backend bundle, without fabricating a scope or connection.
    #[must_use]
    pub const fn new(backend: B) -> Self {
        Self {
            state: RefCell::new(State {
                backend,
                services: None,
            }),
            active: Cell::new(None),
            last_connection: Cell::new(0),
            report: Cell::new(CleanupReport {
                unknown_count: 0,
                failed: false,
            }),
        }
    }
    /// Inspect actual backend or perform explicit device work through the same shared owner.
    /// # Errors
    /// Returns Busy for reentrancy and preserves actual backend failures.
    pub fn with_backend<R>(
        &self,
        operation: impl FnOnce(&mut B) -> Result<R, Error>,
    ) -> Result<R, Error> {
        let mut state = self.state.try_borrow_mut().map_err(|_| Error::Busy)?;
        operation(&mut state.backend)
    }
    /// Last cleanup report; a failed cleanup prevents a new handler on this owner.
    #[must_use]
    pub fn cleanup_report(&self) -> CleanupReport {
        self.report.get()
    }
    pub(crate) fn with<R>(
        &self,
        operation: impl FnOnce(&mut Services<'data, N, C, A, Q>, &mut B) -> Result<R, Error>,
    ) -> Result<R, Error> {
        let mut state = self.state.try_borrow_mut().map_err(|_| Error::Busy)?;
        let State { backend, services } = &mut *state;
        operation(services.as_mut().ok_or(Error::Closed)?, backend)
    }
}
