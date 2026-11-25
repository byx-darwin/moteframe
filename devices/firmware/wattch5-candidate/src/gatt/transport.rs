use super::{Bridge, Queue};
use core::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};
use device_runtime::{OwnedRx, PortError, Scope, Transport};

/// Synchronous foreground over the real externally pinned Host and GATT futures.
///
/// Construct with `pin!(runner.run())` from the pinned local `ble_host_trouble`
/// stack and `pin!(drive_connection(actual_connection, profile, bridge))`.
/// Futures are external to avoid self-reference, allocation and unsafe pin projection.
/// A no-op waker is appropriate only for a continuously polled foreground; actual
/// upstream time/critical-section drivers remain the platform's responsibility.
/// Each budget unit polls exactly one future, alternating lanes across calls.
pub struct TroubleTransport<'a, H: Future, G: Future, const RX: usize, const TX: usize> {
    bridge: &'a Bridge<RX, TX>,
    host: Pin<&'a mut H>,
    gatt: Pin<&'a mut G>,
    host_turn: bool,
    host_done: bool,
    gatt_done: bool,
}
impl<H: Future, G: Future, const RX: usize, const TX: usize> core::fmt::Debug
    for TroubleTransport<'_, H, G, RX, TX>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TroubleTransport")
            .field("bridge", self.bridge)
            .field("host_done", &self.host_done)
            .field("gatt_done", &self.gatt_done)
            .finish_non_exhaustive()
    }
}
impl<'a, H: Future, G: Future, const RX: usize, const TX: usize>
    TroubleTransport<'a, H, G, RX, TX>
{
    /// Borrow the actual external runner and concrete GATT driver futures.
    #[must_use]
    pub const fn new(
        bridge: &'a Bridge<RX, TX>,
        host: Pin<&'a mut H>,
        gatt: Pin<&'a mut G>,
    ) -> Self {
        Self {
            bridge,
            host,
            gatt,
            host_turn: true,
            host_done: false,
            gatt_done: false,
        }
    }
}
impl<H, G, E, const RX: usize, const TX: usize> Transport for TroubleTransport<'_, H, G, RX, TX>
where
    H: Future<Output = Result<(), E>>,
    G: Future<Output = Result<(), PortError>>,
{
    type Queue<'a>
        = Queue<'a, RX, TX>
    where
        Self: 'a;
    fn poll(&mut self, _: u64, budget: u8) -> Result<(), PortError> {
        if budget == 0 {
            return Err(PortError::InvalidInput);
        }
        let mut cx = Context::from_waker(Waker::noop());
        let mut failure = self.host_done.then_some(PortError::Fault);
        for _ in 0..budget {
            if self.host_turn {
                if !self.host_done && poll_lane(self.host.as_mut(), &mut cx).is_ready() {
                    self.host_done = true;
                    self.bridge.host_failed()?;
                    failure = Some(PortError::Fault);
                }
            } else if !self.gatt_done
                && let Poll::Ready(result) = poll_lane(self.gatt.as_mut(), &mut cx)
            {
                self.gatt_done = true;
                if let Err(error) = result {
                    self.bridge.host_failed()?;
                    failure = Some(error);
                }
            }
            self.host_turn = !self.host_turn;
        }
        failure.map_or(Ok(()), Err)
    }
    fn scope(&self) -> Result<Option<Scope>, PortError> {
        self.bridge.scope()
    }
    fn receive(&mut self) -> Result<Option<OwnedRx>, PortError> {
        self.bridge.receive()
    }
    fn queue(&mut self, expected: Scope) -> Result<Self::Queue<'_>, PortError> {
        self.bridge.queue(expected)
    }
    fn close_scope(&mut self, old: Scope) -> Result<(), PortError> {
        self.bridge.close(old)
    }
}

impl<H, G, E, const RX: usize, const TX: usize> device_runtime::EventTransport
    for TroubleTransport<'_, H, G, RX, TX>
where
    H: Future<Output = Result<(), E>>,
    G: Future<Output = Result<(), PortError>>,
{
    fn receive_event<const N: usize>(
        &mut self,
    ) -> Result<Option<device_runtime::BoundedRx<N>>, PortError> {
        self.bridge.receive_event()
    }
    type EventQueue<'a>
        = Queue<'a, RX, TX>
    where
        Self: 'a;
    fn event_queue(
        &mut self,
        scope: Scope,
        plane: device_connection::EventPlane,
    ) -> Result<Self::EventQueue<'_>, PortError> {
        self.bridge.event_queue(scope, plane)
    }
}

// The two lane futures have disjoint scratch lifetimes. Do not reserve Host
// decoding temporaries on the stack while the independent GATT lane is polled.
#[inline(never)]
fn poll_lane<F: Future>(future: Pin<&mut F>, cx: &mut Context<'_>) -> Poll<F::Output> {
    future.poll(cx)
}
