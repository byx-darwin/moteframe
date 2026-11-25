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
                if !self.host_done && self.host.as_mut().poll(&mut cx).is_ready() {
                    self.host_done = true;
                    self.bridge.host_failed()?;
                    failure = Some(PortError::Fault);
                }
            } else if !self.gatt_done
                && let Poll::Ready(result) = self.gatt.as_mut().poll(&mut cx)
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

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use core::{cell::Cell, future::poll_fn, pin::pin};
    #[test]
    fn test_should_bound_and_alternate_actual_future_poll_lanes() {
        let host_calls = Cell::new(0);
        let gatt_calls = Cell::new(0);
        let mut host = pin!(poll_fn(|_| {
            host_calls.set(host_calls.get() + 1);
            Poll::<Result<(), ()>>::Pending
        }));
        let mut gatt = pin!(poll_fn(|_| {
            gatt_calls.set(gatt_calls.get() + 1);
            Poll::<Result<(), PortError>>::Pending
        }));
        let bridge = Bridge::<1, 1>::new().unwrap();
        let mut transport = TroubleTransport::new(&bridge, host.as_mut(), gatt.as_mut());
        assert_eq!(transport.poll(0, 0), Err(PortError::InvalidInput));
        for now in 0..9 {
            transport.poll(now, 1).unwrap();
        }
        assert_eq!((host_calls.get(), gatt_calls.get()), (5, 4));
        transport.poll(9, 3).unwrap();
        assert_eq!((host_calls.get(), gatt_calls.get()), (6, 6));
    }
}
