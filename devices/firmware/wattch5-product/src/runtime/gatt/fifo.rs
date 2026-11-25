use super::CONTROL_MAX;
use core::cell::{RefCell, RefMut};
use device_connection::{FirstFragmentQueue, FirstSlotError, FirstSlotOutcome};
use device_runtime::{OwnedRx, PortError, Scope};

struct Fifo<T, const N: usize> {
    values: [Option<T>; N],
    read: usize,
    len: usize,
}
impl<T, const N: usize> core::fmt::Debug for Fifo<T, N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Fifo")
            .field("length", &self.len)
            .field("capacity", &N)
            .finish_non_exhaustive()
    }
}
impl<T, const N: usize> Fifo<T, N> {
    fn new() -> Self {
        Self {
            values: core::array::from_fn(|_| None),
            read: 0,
            len: 0,
        }
    }
    fn full(&self) -> bool {
        self.len == N
    }
    fn push(&mut self, value: T) -> Result<(), PortError> {
        if N == 0 || self.full() {
            return Err(PortError::Unavailable);
        }
        let index = (self.read + self.len) % N;
        self.values[index] = Some(value);
        self.len += 1;
        Ok(())
    }
    fn front(&self) -> Option<&T> {
        if self.len == 0 {
            return None;
        }
        self.values.get(self.read)?.as_ref()
    }
    fn pop(&mut self) -> Option<T> {
        if self.len == 0 || N == 0 {
            return None;
        }
        let value = self.values[self.read].take();
        self.read = (self.read + 1) % N;
        self.len -= 1;
        value
    }
    fn clear(&mut self) {
        while self.pop().is_some() {}
    }
}
pub(super) struct Frame {
    pub scope: Scope,
    pub bytes: [u8; CONTROL_MAX],
    pub len: usize,
}
impl core::fmt::Debug for Frame {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Frame")
            .field("scope", &self.scope)
            .field("length", &self.len)
            .finish_non_exhaustive()
    }
}
#[derive(Debug)]
struct State<const RX: usize, const TX: usize> {
    scope: Option<Scope>,
    generation: u64,
    epoch: u64,
    connected: bool,
    enabled: bool,
    disconnect: bool,
    rx: Fifo<Frame, RX>,
    tx: Fifo<Frame, TX>,
}
impl<const RX: usize, const TX: usize> State<RX, TX> {
    fn purge(&mut self) {
        self.rx.clear();
        self.tx.clear();
    }
}
/// Bounded FIFO owner for one serialized foreground, retained across connections.
///
/// `RefCell` deliberately makes this boundary !Sync. It is not an ISR mutex.
/// Every borrow is checked; the queue guard spans reservation, fill, publication
/// and the runtime's `response_admitted` callback. No async work holds the guard.
/// Keep this owner alive to preserve monotonically increasing scope generations.
pub struct Bridge<const RX: usize, const TX: usize> {
    state: RefCell<State<RX, TX>>,
}
impl<const RX: usize, const TX: usize> core::fmt::Debug for Bridge<RX, TX> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut debug = f.debug_struct("Bridge");
        if let Ok(s) = self.state.try_borrow() {
            debug
                .field("scope", &s.scope)
                .field("rx_len", &s.rx.len)
                .field("tx_len", &s.tx.len);
        }
        debug.finish_non_exhaustive()
    }
}
impl<const RX: usize, const TX: usize> Bridge<RX, TX> {
    /// Create empty owned FIFOs; this creates no connection or subscription.
    /// # Errors
    /// Both FIFO capacities must be nonzero.
    pub fn new() -> Result<Self, PortError> {
        if RX == 0 || TX == 0 {
            return Err(PortError::InvalidInput);
        }
        Ok(Self {
            state: RefCell::new(State {
                scope: None,
                generation: 0,
                epoch: 0,
                connected: false,
                enabled: false,
                disconnect: false,
                rx: Fifo::new(),
                tx: Fifo::new(),
            }),
        })
    }
    fn borrow(&self) -> Result<RefMut<'_, State<RX, TX>>, PortError> {
        self.state.try_borrow_mut().map_err(|_| PortError::Fault)
    }
    pub(super) fn scope(&self) -> Result<Option<Scope>, PortError> {
        Ok(self.state.try_borrow().map_err(|_| PortError::Fault)?.scope)
    }
    // Keep inflation scratch on the receive branch, outside the caller frame.
    #[inline(never)]
    pub(super) fn receive(&self) -> Result<Option<OwnedRx>, PortError> {
        let mut state = self.borrow()?;
        let Some(front) = state.rx.front() else {
            return Ok(None);
        };
        let bytes = front
            .bytes
            .get(..front.len)
            .ok_or(PortError::InvalidInput)?;
        // The same guard keeps acknowledged input queued until conversion succeeds.
        let owned = OwnedRx::new(front.scope, bytes)?;
        state.rx.pop();
        Ok(Some(owned))
    }
    pub(super) fn queue(&self, expected: Scope) -> Result<Queue<'_, RX, TX>, PortError> {
        let state = self.borrow()?;
        if state.scope != Some(expected) {
            return Err(PortError::InvalidInput);
        }
        Ok(Queue { state, expected })
    }
    pub(super) fn close(&self, old: Scope) -> Result<(), PortError> {
        let mut state = self.borrow()?;
        if state.scope == Some(old) {
            state.scope = None;
            state.purge();
        }
        Ok(())
    }
    pub(super) fn begin(&self) -> Result<u64, PortError> {
        let mut state = self.borrow()?;
        if state.connected {
            return Err(PortError::Unavailable);
        }
        state.epoch = state.epoch.checked_add(1).ok_or(PortError::Fault)?;
        state.connected = true;
        state.enabled = false;
        state.disconnect = false;
        state.scope = None;
        state.purge();
        Ok(state.epoch)
    }
    pub(super) fn end(&self, epoch: u64) -> Result<(), PortError> {
        let mut state = self.borrow()?;
        if state.epoch == epoch {
            state.connected = false;
            state.enabled = false;
            state.scope = None;
            state.purge();
        }
        Ok(())
    }
    pub(super) fn subscription(&self, epoch: u64, enabled: bool) -> Result<(), PortError> {
        let mut state = self.borrow()?;
        if !state.connected || state.epoch != epoch {
            return Err(PortError::Fault);
        }
        if state.enabled == enabled {
            return Ok(());
        }
        state.enabled = enabled;
        state.scope = None;
        state.purge();
        if enabled {
            state.generation = state.generation.checked_add(1).ok_or(PortError::Fault)?;
            state.scope = Some(Scope::new(state.generation, 20)?);
        }
        Ok(())
    }
    pub(super) fn incoming(
        &self,
        epoch: u64,
        offset: usize,
        bytes: &[u8],
    ) -> Result<(), PortError> {
        let mut state = self.borrow()?;
        if !state.connected || state.epoch != epoch {
            return Err(PortError::Fault);
        }
        let scope = state.scope.ok_or(PortError::Unavailable)?;
        if offset != 0 {
            return Err(PortError::InvalidInput);
        }
        if bytes.is_empty()
            || bytes.len() > CONTROL_MAX
            || bytes.len() > usize::from(scope.frame_max())
        {
            return Err(PortError::InvalidInput);
        }
        let mut frame = Frame {
            scope,
            bytes: [0; CONTROL_MAX],
            len: bytes.len(),
        };
        frame.bytes[..bytes.len()].copy_from_slice(bytes);
        state.rx.push(frame)
    }
    pub(super) fn outgoing(&self, epoch: u64) -> Result<Option<Frame>, PortError> {
        let mut state = self.borrow()?;
        if state.epoch != epoch || !state.connected {
            return Err(PortError::Fault);
        }
        Ok(state.tx.pop())
    }
    pub(super) fn disconnect_requested(&self, epoch: u64) -> Result<bool, PortError> {
        let state = self.state.try_borrow().map_err(|_| PortError::Fault)?;
        Ok(state.epoch != epoch || state.disconnect)
    }
    pub(super) fn host_failed(&self) -> Result<(), PortError> {
        let mut state = self.borrow()?;
        state.disconnect = true;
        state.scope = None;
        state.purge();
        Ok(())
    }
}
/// Opaque fixed-profile route: owner, physical connection and CCCD scope generation.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct NotificationTarget {
    owner: usize,
    epoch: u64,
    scope: Scope,
}
impl core::fmt::Debug for NotificationTarget {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("NotificationTarget(..)")
    }
}
/// Checked serialized FIFO reservation, held through application admission.
pub struct Queue<'a, const RX: usize, const TX: usize> {
    state: RefMut<'a, State<RX, TX>>,
    expected: Scope,
}
impl<const RX: usize, const TX: usize> core::fmt::Debug for Queue<'_, RX, TX> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Queue")
            .field("expected", &self.expected)
            .finish_non_exhaustive()
    }
}
impl<const RX: usize, const TX: usize> Queue<'_, RX, TX> {
    fn target(&self) -> Result<NotificationTarget, PortError> {
        if !self.state.connected || !self.state.enabled || self.state.scope.is_none() {
            return Err(PortError::Unavailable);
        }
        if self.state.scope != Some(self.expected) {
            return Err(PortError::InvalidInput);
        }
        // The owner cannot move while its exclusive reservation is live. Target creation
        // stays private; the single profile fixes the control characteristic and bearer.
        Ok(NotificationTarget {
            owner: core::ptr::from_ref(&*self.state).addr(),
            epoch: self.state.epoch,
            scope: self.expected,
        })
    }
}
impl<const RX: usize, const TX: usize> moteframe_ble::NotificationBackend for Queue<'_, RX, TX> {
    type Target = NotificationTarget;
    type Error = PortError;
    fn notification_capacity(&self, target: NotificationTarget) -> Result<usize, PortError> {
        if self.target()? != target {
            return Err(PortError::InvalidInput);
        }
        Ok(usize::from(self.expected.frame_max()).min(CONTROL_MAX))
    }
    fn try_queue_notification(
        &mut self,
        target: NotificationTarget,
        value: &[u8],
    ) -> Result<(), PortError> {
        let capacity = self.notification_capacity(target)?;
        if value.is_empty() || value.len() > capacity {
            return Err(PortError::InvalidInput);
        }
        if self.state.tx.full() {
            return Err(PortError::Unavailable);
        }
        let mut frame = Frame {
            scope: target.scope,
            bytes: [0; CONTROL_MAX],
            len: value.len(),
        };
        frame.bytes[..value.len()].copy_from_slice(value);
        self.state.tx.push(frame)
    }
}
impl<const RX: usize, const TX: usize> FirstFragmentQueue for Queue<'_, RX, TX> {
    fn with_reserved_slot<T, E>(
        &mut self,
        fill: impl FnOnce(&mut [u8]) -> Result<(usize, T), E>,
    ) -> Result<FirstSlotOutcome<T>, FirstSlotError<E>> {
        if self.state.scope != Some(self.expected) {
            return Err(FirstSlotError::Contract);
        }
        if self.state.tx.full() {
            return Ok(FirstSlotOutcome::NoSlot);
        }
        let mut bytes = [0; CONTROL_MAX];
        let (len, value) = fill(&mut bytes).map_err(FirstSlotError::Fill)?;
        if len == 0 || len > CONTROL_MAX || self.state.scope != Some(self.expected) {
            return Err(FirstSlotError::Contract);
        }
        let target = self.target().map_err(|_| FirstSlotError::Contract)?;
        moteframe_ble::CheckedNotifier::new(moteframe_ble::NotificationRef::new(self))
            .try_notify(target, &bytes[..len])
            .map_err(|_| FirstSlotError::Contract)?;
        Ok(FirstSlotOutcome::Admitted(value))
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    extern crate std;
    use super::*;
    use core::cell::Cell;
    #[test]
    fn test_should_bound_each_additional_rx_slot_below_generic_packet_storage() {
        // Additional queue capacity must retain a physical CONTROL value, rather
        // than another public generic packet. Independent of pointer width.
        let extra = core::mem::size_of::<Bridge<3, 1>>() - core::mem::size_of::<Bridge<2, 1>>();
        assert!(
            extra < core::mem::size_of::<OwnedRx>() / 2,
            "RX slot retained {extra} bytes"
        );
    }
    fn active<const RX: usize>() -> (Bridge<RX, 1>, u64, Scope) {
        let bridge = Bridge::new().unwrap();
        let epoch = bridge.begin().unwrap();
        bridge.subscription(epoch, true).unwrap();
        let scope = bridge.scope().unwrap().unwrap();
        (bridge, epoch, scope)
    }
    #[test]
    fn test_should_validate_before_full_without_consuming_the_front() {
        let (bridge, epoch, scope) = active::<2>();
        bridge.incoming(epoch, 0, &[1]).unwrap();
        bridge.incoming(epoch, 0, &[2; 20]).unwrap();
        for (offset, bytes) in [(0, &[][..]), (0, &[3; 21][..]), (1, &[3][..])] {
            assert_eq!(
                bridge.incoming(epoch, offset, bytes),
                Err(PortError::InvalidInput)
            );
            let state = bridge.borrow().unwrap();
            assert_eq!((state.rx.read, state.rx.len), (0, 2));
        }
        assert_eq!(bridge.incoming(epoch, 0, &[3]), Err(PortError::Unavailable));
        for _ in 0..2 {
            assert_eq!(bridge.receive().unwrap().unwrap().scope(), scope);
        }
        assert!(bridge.receive().unwrap().is_none());
        bridge.end(epoch).unwrap();
        assert_eq!(bridge.incoming(epoch, 1, &[]), Err(PortError::Fault));
        let next = bridge.begin().unwrap();
        assert_eq!(bridge.incoming(next, 1, &[]), Err(PortError::Unavailable));
        assert_eq!(bridge.incoming(epoch, 0, &[1]), Err(PortError::Fault));
    }
    #[test]
    fn test_should_purge_only_retired_scopes_and_epochs() {
        let (bridge, epoch, first) = active::<3>();
        for value in 1..=3 {
            bridge.incoming(epoch, 0, &[value]).unwrap();
        }
        bridge.subscription(epoch, true).unwrap();
        assert_eq!(bridge.borrow().unwrap().rx.len, 3);
        bridge.subscription(epoch, false).unwrap();
        assert!(bridge.receive().unwrap().is_none());
        bridge.subscription(epoch, true).unwrap();
        let second = bridge.scope().unwrap().unwrap();
        assert!(second.generation() > first.generation());
        bridge.incoming(epoch, 0, &[4]).unwrap();
        bridge.close(second).unwrap();
        assert!(bridge.receive().unwrap().is_none());
        bridge.subscription(epoch, true).unwrap();
        assert_eq!(bridge.scope().unwrap(), None);
        bridge.subscription(epoch, false).unwrap();
        bridge.subscription(epoch, true).unwrap();
        bridge.incoming(epoch, 0, &[5]).unwrap();
        bridge.end(epoch).unwrap();
        assert!(bridge.receive().unwrap().is_none());
        let next = bridge.begin().unwrap();
        bridge.subscription(next, true).unwrap();
        let current = bridge.scope().unwrap().unwrap();
        bridge.incoming(next, 0, &[6]).unwrap();
        bridge.close(first).unwrap();
        bridge.end(epoch).unwrap();
        assert_eq!(bridge.incoming(epoch, 0, &[7]), Err(PortError::Fault));
        assert_eq!(bridge.subscription(epoch, false), Err(PortError::Fault));
        assert_eq!(bridge.outgoing(epoch).unwrap_err(), PortError::Fault);
        assert!(bridge.disconnect_requested(epoch).unwrap());
        assert_eq!(bridge.receive().unwrap().unwrap().scope(), current);
        bridge.incoming(next, 0, &[8]).unwrap();
        bridge.host_failed().unwrap();
        assert!(bridge.receive().unwrap().is_none());
        assert_eq!(bridge.scope().unwrap(), None);
        assert!(bridge.disconnect_requested(next).unwrap());
        bridge.end(next).unwrap();
        bridge.borrow().unwrap().epoch = u64::MAX;
        assert_eq!(bridge.begin(), Err(PortError::Fault));
        assert!(!bridge.borrow().unwrap().connected);
    }
    #[test]
    fn test_should_serialize_rx_with_fill_publication_and_admission() {
        let (bridge, epoch, scope) = active::<2>();
        bridge.incoming(epoch, 0, &[41]).unwrap();
        bridge.incoming(epoch, 0, &[42]).unwrap();
        let blocked = || {
            assert_eq!(bridge.incoming(epoch, 0, &[43]), Err(PortError::Fault));
            assert_eq!(bridge.receive().unwrap_err(), PortError::Fault);
            assert_eq!(bridge.close(scope), Err(PortError::Fault));
            assert_eq!(bridge.subscription(epoch, false), Err(PortError::Fault));
            assert_eq!(bridge.scope(), Err(PortError::Fault));
        };
        let mut guard = bridge.queue(scope).unwrap();
        assert!(matches!(
            guard.with_reserved_slot(|out| {
                blocked();
                out[0] = 5;
                Ok::<_, ()>((1, ()))
            }),
            Ok(FirstSlotOutcome::Admitted(()))
        ));
        blocked();
        assert_eq!(std::format!("{bridge:?}"), "Bridge { .. }");
        drop(guard);
        assert_eq!(bridge.borrow().unwrap().rx.len, 2);
        assert_eq!(bridge.receive().unwrap().unwrap().scope(), scope);
        assert_eq!(bridge.receive().unwrap().unwrap().scope(), scope);
    }
    #[test]
    fn test_should_own_exact_values_and_wrap_three_rx_slots_in_order() {
        let (bridge, epoch, scope) = active::<3>();
        let mut source = [9; 20];
        bridge.incoming(epoch, 0, &[1]).unwrap();
        bridge.incoming(epoch, 0, &source).unwrap();
        source.fill(99);
        bridge.incoming(epoch, 0, &[2, 3]).unwrap();
        assert_eq!(bridge.incoming(epoch, 0, &[4]), Err(PortError::Unavailable));
        for (expected, wrap) in [
            (&[1][..], true),
            (&[9; 20][..], false),
            (&[2, 3][..], false),
            (&[4][..], false),
        ] {
            {
                let state = bridge.borrow().unwrap();
                let front = state.rx.values[state.rx.read].as_ref().unwrap();
                assert_eq!(front.scope, scope);
                assert_eq!(&front.bytes[..front.len], expected);
            }
            let packet = bridge.receive().unwrap().unwrap();
            assert_eq!(packet.scope(), scope);
            assert!(
                std::format!("{packet:?}").contains(&std::format!("length: {}", expected.len()))
            );
            if wrap {
                bridge.incoming(epoch, 0, &[4]).unwrap();
            }
        }
        assert!(bridge.receive().unwrap().is_none());
        let debug = std::format!(
            "{:?}",
            Frame {
                scope,
                bytes: [199; 20],
                len: 20
            }
        );
        assert!(!debug.contains("199"));
    }
    #[test]
    fn test_should_retain_corrupt_front_until_checked_inflation_succeeds() {
        let (bridge, epoch, scope) = active::<2>();
        bridge.incoming(epoch, 0, &[71]).unwrap();
        bridge.incoming(epoch, 0, &[72]).unwrap();
        for length in [0, 21, usize::MAX] {
            bridge.borrow().unwrap().rx.values[0].as_mut().unwrap().len = length;
            assert_eq!(bridge.receive().unwrap_err(), PortError::InvalidInput);
            let state = bridge.borrow().unwrap();
            assert_eq!((state.rx.read, state.rx.len), (0, 2));
            assert_eq!(state.rx.values[0].as_ref().unwrap().bytes[0], 71);
            assert_eq!(state.rx.values[1].as_ref().unwrap().bytes[0], 72);
        }
        // Receive preserves the original saved scope; Runtime owns stale-scope rejection.
        let saved = Scope::new(scope.generation() + 1, 20).unwrap();
        bridge.borrow().unwrap().scope = Some(saved);
        bridge.borrow().unwrap().rx.values[0].as_mut().unwrap().len = 1;
        assert_eq!(bridge.receive().unwrap().unwrap().scope(), scope);
        assert_eq!(bridge.borrow().unwrap().rx.read, 1);
        assert_eq!(bridge.borrow().unwrap().rx.len, 1);
        assert_eq!(
            bridge.borrow().unwrap().rx.values[1]
                .as_ref()
                .unwrap()
                .bytes[0],
            72
        );
        assert_eq!(bridge.receive().unwrap().unwrap().scope(), scope);
        assert!(bridge.receive().unwrap().is_none());
    }
    #[test]
    fn test_should_keep_scope_and_admission_callback_under_the_same_checked_guard() {
        let bridge = Bridge::<1, 1>::new().unwrap();
        let epoch = bridge.begin().unwrap();
        bridge.subscription(epoch, true).unwrap();
        let scope = bridge.scope().unwrap().unwrap();
        let mut guard = bridge.queue(scope).unwrap();
        let result = guard.with_reserved_slot(|out| {
            assert_eq!(bridge.subscription(epoch, false), Err(PortError::Fault));
            out[0] = 7;
            Ok::<_, ()>((1, ()))
        });
        assert!(matches!(result, Ok(FirstSlotOutcome::Admitted(()))));
        // Mirrors Runtime retaining its guard through response_admitted.
        assert_eq!(bridge.close(scope), Err(PortError::Fault));
        drop(guard);
        bridge.close(scope).unwrap();
        bridge.subscription(epoch, true).unwrap();
        assert_eq!(bridge.scope().unwrap(), None);
        bridge.subscription(epoch, false).unwrap();
        bridge.subscription(epoch, true).unwrap();
        assert!(bridge.scope().unwrap().unwrap().generation() > scope.generation());
    }
    #[test]
    fn test_should_not_consume_fill_on_full_or_fill_error_or_invalid_length() {
        let bridge = Bridge::<1, 1>::new().unwrap();
        let epoch = bridge.begin().unwrap();
        bridge.subscription(epoch, true).unwrap();
        let scope = bridge.scope().unwrap().unwrap();
        let mut guard = bridge.queue(scope).unwrap();
        assert!(matches!(
            guard.with_reserved_slot::<(), _>(|_| Err(9)),
            Err(FirstSlotError::Fill(9))
        ));
        assert!(matches!(
            guard.with_reserved_slot(|_| Ok::<_, ()>((21, ()))),
            Err(FirstSlotError::Contract)
        ));
        assert!(matches!(
            guard.with_reserved_slot(|_| Ok::<_, ()>((0, ()))),
            Err(FirstSlotError::Contract)
        ));
        assert!(matches!(
            guard.with_reserved_slot(|_| Ok::<_, ()>((1, 5))),
            Ok(FirstSlotOutcome::Admitted(5))
        ));
        let consumed = Cell::new(false);
        assert!(matches!(
            guard.with_reserved_slot(|_| {
                consumed.set(true);
                Ok::<_, ()>((1, ()))
            }),
            Ok(FirstSlotOutcome::NoSlot)
        ));
        assert!(!consumed.get());
    }
    #[test]
    fn test_should_preserve_new_scope_when_an_old_driver_finishes() {
        let bridge = Bridge::<1, 1>::new().unwrap();
        let old = bridge.begin().unwrap();
        bridge.subscription(old, true).unwrap();
        let first = bridge.scope().unwrap().unwrap();
        bridge.end(old).unwrap();
        let next = bridge.begin().unwrap();
        bridge.subscription(next, true).unwrap();
        let current = bridge.scope().unwrap().unwrap();
        bridge.end(old).unwrap();
        bridge.close(first).unwrap();
        assert_eq!(bridge.scope().unwrap(), Some(current));
        assert_eq!(bridge.subscription(old, false), Err(PortError::Fault));
    }
    #[test]
    fn test_should_fail_closed_without_wrapping_subscription_generation() {
        let bridge = Bridge::<1, 1>::new().unwrap();
        let epoch = bridge.begin().unwrap();
        bridge.borrow().unwrap().generation = u64::MAX;
        assert_eq!(bridge.subscription(epoch, true), Err(PortError::Fault));
        assert_eq!(bridge.scope().unwrap(), None);
    }
    #[test]
    fn test_should_revalidate_complete_public_notification_target_before_queue_admission() {
        use moteframe_ble::{CheckedNotifier, NotificationBackend, NotificationRef, NotifyError};
        let (bridge, epoch, scope) = active::<1>();
        let mut q = bridge.queue(scope).unwrap();
        let target = q.target().unwrap();
        assert_eq!(q.notification_capacity(target), Ok(20));
        let stale = NotificationTarget {
            owner: target.owner.wrapping_add(1),
            ..target
        };
        assert_eq!(q.notification_capacity(stale), Err(PortError::InvalidInput));
        q.state.enabled = false;
        assert!(matches!(
            CheckedNotifier::new(NotificationRef::new(&mut q)).try_notify(target, &[1]),
            Err(NotifyError::Backend(PortError::Unavailable))
        ));
        assert_eq!(q.state.tx.len, 0);
        q.state.enabled = true;
        CheckedNotifier::new(NotificationRef::new(&mut q))
            .try_notify(target, &[1, 2])
            .unwrap();
        assert_eq!(q.state.tx.len, 1);
        assert!(matches!(
            CheckedNotifier::new(NotificationRef::new(&mut q)).try_notify(target, &[3]),
            Err(NotifyError::Backend(PortError::Unavailable))
        ));
        drop(q);
        let frame = bridge.outgoing(epoch).unwrap().unwrap();
        assert_eq!(&frame.bytes[..frame.len], &[1, 2]);
    }
}
