//! One lexically scoped CONTROL and DATA connection with fair foreground work.
use crate::{Budgets, PortError, Progress, RuntimeError, Scope, Transport, Work};
use device_connection::{
    BodyStorage, BoundedControlConnection, CopiedBody, EventPlane, EventPump, EventSource,
    FirstFragmentQueue, FirstSlotError, FirstSlotOutcome, PumpError, RetainedBody,
    RetainedEventSource, SharedControlChannel,
};
use device_services::{Config, ServiceHandler};

/// Exact owned value for event runtimes whose real CONTROL limit is smaller.
/// Capacity failure never truncates a GATT value or changes its original scope.
pub struct BoundedRx<const N: usize> {
    scope: Scope,
    bytes: [u8; N],
    length: usize,
}
impl<const N: usize> core::fmt::Debug for BoundedRx<N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BoundedRx")
            .field("scope", &self.scope)
            .field("length", &self.length)
            .finish_non_exhaustive()
    }
}
impl<const N: usize> BoundedRx<N> {
    /// Copy one actual value with its exact original transport generation.
    /// # Errors
    /// Empty, over-capacity or over-scope values fail before publication.
    pub fn new(scope: Scope, bytes: &[u8]) -> Result<Self, PortError> {
        if bytes.is_empty() || bytes.len() > N || bytes.len() > usize::from(scope.frame_max()) {
            return Err(PortError::InvalidInput);
        }
        let mut value = Self {
            scope,
            bytes: [0; N],
            length: bytes.len(),
        };
        value.bytes[..bytes.len()].copy_from_slice(bytes);
        Ok(value)
    }
    /// Actual scope captured with this value.
    #[must_use]
    pub const fn scope(&self) -> Scope {
        self.scope
    }
    fn bytes(&self) -> &[u8] {
        &self.bytes[..self.length]
    }
}
/// Transport with a real separately subscribed DATA notification FIFO.
pub trait EventTransport: Transport {
    /// Guard type retains the same scope/publication boundary as CONTROL.
    type EventQueue<'a>: FirstFragmentQueue
    where
        Self: 'a;
    /// Receive directly into bounded ownership. Adapters may override to avoid
    /// constructing the old generic512-byte packet on a fixed20-byte carrier.
    /// # Errors
    /// Over-capacity values fail explicitly; they are never truncated.
    fn receive_event<const N: usize>(&mut self) -> Result<Option<BoundedRx<N>>, PortError> {
        self.receive()?
            .map(|p| BoundedRx::new(p.scope(), p.bytes()))
            .transpose()
    }
    /// Select the actual event characteristic. No reservation occurs here.
    /// # Errors
    /// Stale/unsubscribed scopes must fail without publication.
    fn event_queue(
        &mut self,
        scope: Scope,
        plane: EventPlane,
    ) -> Result<Self::EventQueue<'_>, PortError>;
}
/// Either existing CONTROL failure or an event-pump failure, including cleanup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventRuntimeError {
    /// Transport, protocol, clock or scoped actor failure.
    Control(RuntimeError),
    /// Event publication/authorization or cleanup failed.
    Event(PumpError),
}
/// All independent lane results from one bounded foreground pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventStepReport {
    /// Actual FIFO admission progress, never peer receipt.
    pub connection: Result<Progress, EventRuntimeError>,
    /// One UI slice, also serviced after terminal BLE failure.
    pub ui: Result<(), PortError>,
    /// One boot-owned persistence slice, also serviced after terminal BLE failure.
    pub background: Result<(), PortError>,
}
/// A single subscription's owners. Construct fresh external channel/data owners
/// and service cells for every subscription; this actor never reopens them.
/// Boot-owned stores and accepted work outlive this lexical scope.
/// `N` bounds logical CONTROL bodies; `F` bounds one owned physical value.
/// `F` defaults to `N` for existing callers; smaller physical carriers can use
/// less pending-frame storage without reducing logical message capacity.
pub struct EventRuntime<
    'a,
    H: ServiceHandler,
    S: EventSource,
    const N: usize = 512,
    const F: usize = N,
    B: BodyStorage<S> = CopiedBody,
> {
    scope: Scope,
    control: BoundedControlConnection<'a, N>,
    handler: H,
    events: EventPump<'a, S, B>,
    pending: Option<BoundedRx<F>>,
    budgets: Budgets,
    last_now: Option<u64>,
    stopped: Option<EventRuntimeError>,
}
impl<H: ServiceHandler, S: EventSource, const N: usize, const F: usize, B: BodyStorage<S>>
    core::fmt::Debug for EventRuntime<'_, H, S, N, F, B>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("EventRuntime")
            .field("scope", &self.scope)
            .field("stopped", &self.stopped)
            .finish_non_exhaustive()
    }
}
impl<'a, H: ServiceHandler, S: EventSource, const N: usize, const F: usize>
    EventRuntime<'a, H, S, N, F>
{
    /// Bind the compatibility sender owning a full logical body copy.
    /// # Errors
    /// Invalid channel, catalogue, scope or limits reject construction.
    pub fn new(
        config: Config<'a>,
        scope: Scope,
        channel: &'a SharedControlChannel,
        handler: H,
        source: S,
        budgets: Budgets,
    ) -> Result<Self, EventRuntimeError> {
        Self::with_storage(
            config,
            scope,
            channel,
            handler,
            source,
            budgets,
            CopiedBody::default(),
        )
    }
}
impl<'a, H: ServiceHandler, S: RetainedEventSource, const N: usize, const F: usize>
    EventRuntime<'a, H, S, N, F, RetainedBody>
{
    /// Bind a retained source without duplicating its complete logical body.
    /// # Errors
    /// Same channel, catalogue, scope and limits failures as `new`.
    pub fn new_retained(
        config: Config<'a>,
        scope: Scope,
        channel: &'a SharedControlChannel,
        handler: H,
        source: S,
        budgets: Budgets,
    ) -> Result<Self, EventRuntimeError> {
        Self::with_storage(
            config,
            scope,
            channel,
            handler,
            source,
            budgets,
            RetainedBody,
        )
    }
}
impl<'a, H: ServiceHandler, S: EventSource, const N: usize, const F: usize, B: BodyStorage<S>>
    EventRuntime<'a, H, S, N, F, B>
{
    /// Bind the same real configuration/channel to CONTROL and event arbitration.
    /// # Errors
    /// Invalid limits, duplicate channel use or unavailable notifications reject construction.
    fn with_storage(
        config: Config<'a>,
        scope: Scope,
        channel: &'a SharedControlChannel,
        handler: H,
        source: S,
        budgets: Budgets,
        body: B,
    ) -> Result<Self, EventRuntimeError> {
        if usize::from(config.limits.rx_frame_max) > F
            || config.limits.rx_frame_max > scope.frame_max()
            || config.limits.tx_frame_max > scope.frame_max()
            || budgets.tx > u8::MAX - 2
        {
            return Err(EventRuntimeError::Control(
                RuntimeError::InvalidConfiguration,
            ));
        }
        let (mut control, token) = BoundedControlConnection::with_events(config, channel)
            .map_err(|e| EventRuntimeError::Control(RuntimeError::Connection(e)))?;
        control
            .notifications_enabled()
            .map_err(|e| EventRuntimeError::Control(RuntimeError::Connection(e)))?;
        let events =
            EventPump::with_storage(token, source, body).map_err(EventRuntimeError::Event)?;
        Ok(Self {
            scope,
            control,
            handler,
            events,
            pending: None,
            budgets,
            last_now: None,
            stopped: None,
        })
    }
    /// Service bounded transport, CONTROL, event fragments and independent workers.
    /// Terminal connection failure never prevents storage/UI progress on later calls.
    pub fn step(
        &mut self,
        now: u64,
        transport: &mut impl EventTransport,
        ui: &mut impl Work,
        background: &mut impl Work,
    ) -> EventStepReport {
        let safe = self.last_now.map_or(now, |last| last.max(now));
        let regressed = safe != now;
        self.last_now = Some(safe);
        let polled = transport
            .poll(safe, self.budgets.transport)
            .map_err(|e| EventRuntimeError::Control(RuntimeError::Port(e)));
        let mut result = if let Some(error) = self.stopped {
            Err(error)
        } else if regressed {
            Err(EventRuntimeError::Control(RuntimeError::ClockRegression))
        } else {
            polled.and_then(|()| self.connection_step(safe, transport))
        };
        if let Err(error) = result {
            self.stopped = Some(error);
            if let Err(cleanup) = self.shutdown(transport) {
                result = Err(cleanup);
            }
        }
        EventStepReport {
            connection: result,
            ui: ui.step(safe),
            background: background.step(safe),
        }
    }
    /// Mute queues and close both real actors; repeat retries fallible cleanup.
    /// # Errors
    /// Every failure is surfaced; accepted boot-owned storage is not cancelled.
    pub fn shutdown(
        &mut self,
        transport: &mut impl EventTransport,
    ) -> Result<(), EventRuntimeError> {
        self.pending = None;
        let queue = transport
            .close_scope(self.scope)
            .map_err(|e| EventRuntimeError::Control(RuntimeError::Port(e)));
        let control = self
            .control
            .close(&mut self.handler)
            .map_err(|e| EventRuntimeError::Control(RuntimeError::Connection(e)));
        let events = self.events.close().map_err(EventRuntimeError::Event);
        queue.and(control).and(events)
    }
    // Receive packets and CONTROL dispatch have temporary ownership separate
    // from event preparation; keep those frames out of the later DATA path.
    #[inline(never)]
    fn receive_control(
        &mut self,
        now: u64,
        transport: &mut impl EventTransport,
        progress: &mut Progress,
    ) -> Result<bool, EventRuntimeError> {
        let wrap = |e| EventRuntimeError::Control(RuntimeError::Connection(e));
        let mut staged = false;
        for _ in 0..self.budgets.rx {
            if transport
                .scope()
                .map_err(|e| EventRuntimeError::Control(RuntimeError::Port(e)))?
                != Some(self.scope)
            {
                return Err(EventRuntimeError::Control(RuntimeError::StaleScope));
            }
            let packet = if let Some(packet) = self.pending.take() {
                packet
            } else {
                let Some(packet) = transport
                    .receive_event()
                    .map_err(|e| EventRuntimeError::Control(RuntimeError::Port(e)))?
                else {
                    break;
                };
                progress.received += 1;
                packet
            };
            if transport
                .scope()
                .map_err(|e| EventRuntimeError::Control(RuntimeError::Port(e)))?
                != Some(self.scope)
            {
                return Err(EventRuntimeError::Control(RuntimeError::StaleScope));
            }
            if packet.scope() != self.scope {
                progress.stale += 1;
                continue;
            }
            match self
                .control
                .receive_staged(packet.bytes(), now, &mut self.handler)
            {
                Ok(device_connection::Receive::Deferred) => staged = true,
                Ok(_) => (),
                Err(device_connection::ConnectionError::Busy) => {
                    self.pending = Some(packet);
                    break;
                }
                Err(e) => return Err(wrap(e)),
            }
        }
        Ok(staged)
    }
    // Keep request/event scratch out of the caller's frame while it polls the
    // Host and independent UI/storage workers. These phases execute serially.
    #[inline(never)]
    fn connection_step(
        &mut self,
        now: u64,
        transport: &mut impl EventTransport,
    ) -> Result<Progress, EventRuntimeError> {
        let wrap = |e| EventRuntimeError::Control(RuntimeError::Connection(e));
        if transport
            .scope()
            .map_err(|e| EventRuntimeError::Control(RuntimeError::Port(e)))?
            != Some(self.scope)
        {
            return Err(EventRuntimeError::Control(RuntimeError::StaleScope));
        }
        self.control.tick(now, &mut self.handler).map_err(wrap)?;
        if self.control.waiting_for_handler() {
            self.control
                .poll_pending(now, &mut self.handler)
                .map_err(wrap)?;
        }
        let mut progress = Progress::default();
        if self.receive_control(now, transport, &mut progress)? {
            self.control
                .dispatch_received(&mut self.handler)
                .map_err(wrap)?;
        }
        self.flush_control(transport, &mut progress)?;
        self.flush_events(now, transport, &mut progress)?;
        Ok(progress)
    }
    // Each phase releases its temporary queue/packet storage before the next.
    #[inline(never)]
    fn flush_control(
        &mut self,
        transport: &mut impl EventTransport,
        progress: &mut Progress,
    ) -> Result<(), EventRuntimeError> {
        let wrap = |e| EventRuntimeError::Control(RuntimeError::Connection(e));
        for _ in 0..self.budgets.tx {
            if !self.control.response_pending() {
                break;
            }
            let mut queue = transport
                .queue(self.scope)
                .map_err(|e| EventRuntimeError::Control(RuntimeError::Port(e)))?;
            match queue.with_reserved_slot(|bytes| {
                self.control
                    .next_frame(&mut self.handler, bytes)
                    .map(|n| (n, ()))
            }) {
                Ok(FirstSlotOutcome::NoSlot) => break,
                Ok(FirstSlotOutcome::Admitted(())) => {
                    self.control.admit_frame(&mut self.handler).map_err(wrap)?;
                    progress.transmitted += 1;
                }
                Err(FirstSlotError::Fill(e)) => return Err(wrap(e)),
                Err(FirstSlotError::Transport | FirstSlotError::Contract) => {
                    return Err(EventRuntimeError::Control(RuntimeError::Publication));
                }
            }
        }
        Ok(())
    }
    #[inline(never)]
    fn flush_events(
        &mut self,
        now: u64,
        transport: &mut impl EventTransport,
        progress: &mut Progress,
    ) -> Result<(), EventRuntimeError> {
        for plane in [EventPlane::Control, EventPlane::Data] {
            let mut queue = transport
                .event_queue(self.scope, plane)
                .map_err(|e| EventRuntimeError::Control(RuntimeError::Port(e)))?;
            if self
                .events
                .flush(now, plane, &mut queue)
                .map_err(EventRuntimeError::Event)?
            {
                progress.transmitted += 1;
            }
        }
        Ok(())
    }
}
