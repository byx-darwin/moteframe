//! Serialized CONTROL lifecycle and fair foreground scheduling.
use crate::{OwnedRx, PortError, Scope, SessionFactory, Transport, Work};
use device_connection::{
    ConnectionError, ControlConnection, FirstFragmentQueue, FirstSlotError, FirstSlotOutcome,
};
use device_services::ServiceHandler;

/// Finite per-step units. Each RX/TX call processes at most one GATT value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budgets {
    pub(crate) rx: u8,
    pub(crate) tx: u8,
    pub(crate) transport: u8,
}
impl Budgets {
    /// Select positive finite RX/TX and Host/Controller work limits.
    /// # Errors
    /// Any zero budget would starve a lane and is rejected.
    pub const fn new(rx: u8, tx: u8, transport: u8) -> Result<Self, PortError> {
        if rx == 0 || tx == 0 || transport == 0 {
            return Err(PortError::InvalidInput);
        }
        Ok(Self { rx, tx, transport })
    }
}
/// Connection lane failure, separate from the independent UI/background results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeError {
    /// A real platform or factory port failed.
    Port(PortError),
    /// Existing protocol/handler actor failed.
    Connection(ConnectionError),
    /// Actual time regressed; the previous actor is closed.
    ClockRegression,
    /// A retired generation was reused or the same generation changed capacity.
    StaleScope,
    /// Factory reused an application session identity or exceeded actual frame limits.
    InvalidConfiguration,
    /// A queue commit was uncertain or its serialized-scope contract failed.
    Publication,
    /// Failed cleanup retains old actor ownership until explicit retry.
    CleanupBlocked,
}
/// Bounded connection-lane accounting, never a claim of peer receipt or durable completion.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    /// Incoming values consumed, including discarded stale values.
    pub received: u8,
    /// Actual queue-admitted fragments.
    pub transmitted: u8,
    /// Stale RX values discarded without dispatch.
    pub stale: u8,
}
/// All three lane results; one failure never skips the other scheduled slices.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StepReport {
    /// Actual transport/protocol progress or failure.
    pub connection: Result<Progress, RuntimeError>,
    /// One UI/render slice result.
    pub ui: Result<(), PortError>,
    /// One background/storage slice result.
    pub background: Result<(), PortError>,
}
struct Actor<'a, F: SessionFactory + 'a> {
    scope: Scope,
    control: Option<ControlConnection<'a>>,
    handler: F::Handler<'a>,
}
/// Bounded cooperative owner of CONTROL actors and one retained Busy RX value.
/// Persistent owners remain in the externally borrowed factory. Call shutdown before destruction:
/// Drop cannot run fallible backend cleanup. No implicit executor, trust or DATA owner is created.
pub struct Runtime<'a, F: SessionFactory + 'a> {
    factory: &'a F,
    actor: Option<Actor<'a, F>>,
    pending: Option<OwnedRx>,
    opening: Option<Scope>,
    budgets: Budgets,
    last_now: Option<u64>,
    last_generation: u64,
    last_session: u32,
    blocked: bool,
}
impl<F: SessionFactory> core::fmt::Debug for Runtime<'_, F> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Runtime")
            .field("active", &self.actor.is_some())
            .field("pending_rx", &self.pending.is_some())
            .field("cleanup_blocked", &self.blocked)
            .finish_non_exhaustive()
    }
}
impl<'a, F: SessionFactory + 'a> Runtime<'a, F> {
    /// Create a closed runtime around external stable owners; no backend work occurs here.
    #[must_use]
    pub const fn new(factory: &'a F, budgets: Budgets) -> Self {
        Self {
            factory,
            actor: None,
            pending: None,
            opening: None,
            budgets,
            last_now: None,
            last_generation: 0,
            last_session: 0,
            blocked: false,
        }
    }
    /// Service bounded transport/CONTROL, then one UI slice and one background slice.
    /// All failures are returned in `StepReport`. Regressing clocks use the last valid time for
    /// executor/UI/background servicing and retire CONTROL before further protocol work.
    pub fn step(
        &mut self,
        now_ms: u64,
        transport: &mut impl Transport,
        ui: &mut impl Work,
        background: &mut impl Work,
    ) -> StepReport {
        let regressed = self.last_now.is_some_and(|last| now_ms < last);
        let safe_now = self.last_now.map_or(now_ms, |last| last.max(now_ms));
        self.last_now = Some(safe_now);
        let polled = transport
            .poll(safe_now, self.budgets.transport)
            .map_err(RuntimeError::Port);
        let result = if self.blocked {
            Err(RuntimeError::CleanupBlocked)
        } else if regressed {
            Err(RuntimeError::ClockRegression)
        } else {
            polled.and_then(|()| self.connection_step(safe_now, transport))
        };
        let connection = match result {
            Err(error @ RuntimeError::Connection(ConnectionError::CleanupFailed(_)))
                if !self.blocked =>
            {
                self.pending = None;
                self.blocked = true;
                if let Some(actor) = self.actor.as_ref() {
                    let _transport_cleanup = transport.close_scope(actor.scope);
                }
                Err(error)
            }
            Err(error) if !self.blocked => self.retire(transport).and(Err(error)),
            other => other,
        };
        StepReport {
            connection,
            ui: ui.step(safe_now),
            background: background.step(safe_now),
        }
    }
    /// Close actual old scoped transport and invoke all handler cleanup before dropping the actor.
    /// # Errors
    /// Failed/uncertain cleanup retains ownership and blocks new sessions until explicit retry.
    pub fn shutdown(&mut self, transport: &mut impl Transport) -> Result<(), RuntimeError> {
        self.retire(transport)
    }
    /// Retry retained fallible cleanup; never opens a fresh actor by itself.
    /// # Errors
    /// Actual transport/handler cleanup errors remain explicit and keep reuse blocked.
    pub fn retry_cleanup(&mut self, transport: &mut impl Transport) -> Result<(), RuntimeError> {
        self.retire(transport)
    }
    fn retire(&mut self, transport: &mut impl Transport) -> Result<(), RuntimeError> {
        self.pending = None;
        let Some(actor) = self.actor.as_mut() else {
            if let Some(scope) = self.opening {
                let result = transport.close_scope(scope).map_err(RuntimeError::Port);
                self.blocked = result.is_err();
                result?;
                self.opening = None;
            }
            self.blocked = false;
            return Ok(());
        };
        let transport_result = transport
            .close_scope(actor.scope)
            .map_err(RuntimeError::Port);
        let handler_result = if let Some(control) = actor.control.as_mut() {
            control
                .close(&mut actor.handler)
                .map_err(RuntimeError::Connection)
        } else {
            actor
                .handler
                .connection_closed()
                .map_err(|error| RuntimeError::Connection(ConnectionError::CleanupFailed(error)))
        };
        self.blocked = transport_result.is_err() || handler_result.is_err();
        handler_result.and(transport_result)?;
        self.actor = None;
        Ok(())
    }
    #[inline(never)]
    fn synchronize(&mut self, transport: &mut impl Transport) -> Result<(), RuntimeError> {
        let actual = transport.scope().map_err(RuntimeError::Port)?;
        if self
            .actor
            .as_ref()
            .is_some_and(|actor| Some(actor.scope) != actual)
        {
            self.retire(transport)?;
        }
        let Some(scope) = actual else {
            return Ok(());
        };
        if self.actor.is_some() {
            return Ok(());
        }
        if scope.generation() <= self.last_generation {
            return Err(RuntimeError::StaleScope);
        }
        self.last_generation = scope.generation();
        self.opening = Some(scope);
        let factory: &'a F = self.factory;
        let (config, handler) = factory.open(scope).map_err(RuntimeError::Port)?;
        self.actor = Some(Actor {
            scope,
            control: None,
            handler,
        });
        self.opening = None;
        if config.session <= self.last_session
            || config.limits.rx_frame_max > scope.frame_max()
            || config.limits.tx_frame_max > scope.frame_max()
            || config.limits.data_max != 0
        {
            return Err(RuntimeError::InvalidConfiguration);
        }
        self.last_session = config.session;
        let mut created = ControlConnection::new(config);
        match created.as_mut() {
            Ok(control) => control
                .notifications_enabled()
                .map_err(RuntimeError::Connection)?,
            Err(error) => return Err(RuntimeError::Connection(*error)),
        }
        match created {
            Ok(control) => {
                if let Some(actor) = self.actor.as_mut() {
                    actor.control = Some(control);
                }
            }
            Err(error) => return Err(RuntimeError::Connection(error)),
        }
        Ok(())
    }
    fn connection_step(
        &mut self,
        now: u64,
        transport: &mut impl Transport,
    ) -> Result<Progress, RuntimeError> {
        self.synchronize(transport)?;
        let mut progress = Progress::default();
        let Some(actor) = self.actor.as_mut() else {
            return Ok(progress);
        };
        let control = actor
            .control
            .as_mut()
            .ok_or(RuntimeError::InvalidConfiguration)?;
        control
            .tick(now, &mut actor.handler)
            .map_err(RuntimeError::Connection)?;
        if control.waiting_for_handler() {
            control
                .poll_pending(now, &mut actor.handler)
                .map_err(RuntimeError::Connection)?;
        }
        for _ in 0..self.budgets.rx {
            if transport.scope().map_err(RuntimeError::Port)? != Some(actor.scope) {
                return Err(RuntimeError::StaleScope);
            }
            let packet = if let Some(packet) = self.pending.take() {
                packet
            } else {
                let Some(packet) = transport.receive().map_err(RuntimeError::Port)? else {
                    break;
                };
                progress.received += 1;
                packet
            };
            if transport.scope().map_err(RuntimeError::Port)? != Some(actor.scope) {
                return Err(RuntimeError::StaleScope);
            }
            if packet.scope() != actor.scope {
                progress.stale += 1;
                continue;
            }
            match control.receive(packet.bytes(), now, &mut actor.handler) {
                Ok(_) => (),
                Err(ConnectionError::Busy) => {
                    self.pending = Some(packet);
                    break;
                }
                Err(error) => return Err(RuntimeError::Connection(error)),
            }
        }
        for _ in 0..self.budgets.tx {
            if !control.response_pending() {
                break;
            }
            let mut queue = transport.queue(actor.scope).map_err(RuntimeError::Port)?;
            let offered = queue.with_reserved_slot(|bytes| {
                control
                    .next_frame(&mut actor.handler, bytes)
                    .map(|length| (length, ()))
            });
            match offered {
                Ok(FirstSlotOutcome::NoSlot) => break,
                Ok(FirstSlotOutcome::Admitted(())) => {
                    control
                        .admit_frame(&mut actor.handler)
                        .map_err(RuntimeError::Connection)?;
                    progress.transmitted += 1;
                }
                Err(FirstSlotError::Fill(error)) => return Err(RuntimeError::Connection(error)),
                Err(FirstSlotError::Transport | FirstSlotError::Contract) => {
                    return Err(RuntimeError::Publication);
                }
            }
        }
        Ok(progress)
    }
}
