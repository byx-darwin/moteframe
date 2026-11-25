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
    pub plane: device_connection::EventPlane,
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
    control_enabled: bool,
    data_enabled: bool,
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
                control_enabled: false,
                data_enabled: false,
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
    #[inline(never)]
    pub(super) fn receive_event<const N: usize>(
        &self,
    ) -> Result<Option<device_runtime::BoundedRx<N>>, PortError> {
        let mut state = self.borrow()?;
        let Some(front) = state.rx.front() else {
            return Ok(None);
        };
        let bytes = front
            .bytes
            .get(..front.len)
            .ok_or(PortError::InvalidInput)?;
        let packet = device_runtime::BoundedRx::new(front.scope, bytes)?;
        state.rx.pop();
        Ok(Some(packet))
    }
    pub(super) fn queue(&self, expected: Scope) -> Result<Queue<'_, RX, TX>, PortError> {
        let state = self.borrow()?;
        if state.scope != Some(expected) {
            return Err(PortError::InvalidInput);
        }
        Ok(Queue {
            state,
            expected,
            plane: device_connection::EventPlane::Control,
        })
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
        state.control_enabled = false;
        state.data_enabled = false;
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
            state.control_enabled = false;
            state.data_enabled = false;
            state.scope = None;
            state.purge();
        }
        Ok(())
    }
    pub(super) fn subscription(&self, epoch: u64, enabled: bool) -> Result<(), PortError> {
        self.set_subscription(epoch, device_connection::EventPlane::Control, enabled)
    }
    pub(super) fn data_subscription(&self, epoch: u64, enabled: bool) -> Result<(), PortError> {
        self.set_subscription(epoch, device_connection::EventPlane::Data, enabled)
    }
    fn set_subscription(
        &self,
        epoch: u64,
        plane: device_connection::EventPlane,
        enabled: bool,
    ) -> Result<(), PortError> {
        let mut state = self.borrow()?;
        if !state.connected || state.epoch != epoch {
            return Err(PortError::Fault);
        }
        match plane {
            device_connection::EventPlane::Control => state.control_enabled = enabled,
            device_connection::EventPlane::Data => state.data_enabled = enabled,
        }
        let enabled = state.control_enabled && state.data_enabled;
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
    pub(super) fn event_queue(
        &self,
        expected: Scope,
        plane: device_connection::EventPlane,
    ) -> Result<Queue<'_, RX, TX>, PortError> {
        let mut queue = self.queue(expected)?;
        queue.plane = plane;
        Ok(queue)
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
            plane: device_connection::EventPlane::Control,
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
/// Checked serialized FIFO reservation, held through application admission.
pub struct Queue<'a, const RX: usize, const TX: usize> {
    state: RefMut<'a, State<RX, TX>>,
    expected: Scope,
    plane: device_connection::EventPlane,
}
impl<const RX: usize, const TX: usize> core::fmt::Debug for Queue<'_, RX, TX> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Queue")
            .field("expected", &self.expected)
            .finish_non_exhaustive()
    }
}
/// Opaque identity for the actual single-profile CONTROL/DATA notification route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NotificationTarget {
    owner: usize,
    epoch: u64,
    scope: Scope,
    plane: device_connection::EventPlane,
}
impl<const RX: usize, const TX: usize> Queue<'_, RX, TX> {
    fn target(&self) -> Result<NotificationTarget, PortError> {
        if !self.state.connected || !self.state.enabled || self.state.scope.is_none() {
            return Err(PortError::Unavailable);
        }
        if self.state.scope != Some(self.expected) {
            return Err(PortError::InvalidInput);
        }
        let subscribed = match self.plane {
            device_connection::EventPlane::Control => self.state.control_enabled,
            device_connection::EventPlane::Data => self.state.data_enabled,
        };
        if !subscribed {
            return Err(PortError::Unavailable);
        }
        Ok(NotificationTarget {
            owner: core::ptr::from_ref(&*self.state).addr(),
            epoch: self.state.epoch,
            scope: self.expected,
            plane: self.plane,
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
            plane: target.plane,
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
        // Keep the same exclusive FIFO guard through checked publication and admission.
        moteframe_ble::CheckedNotifier::new(moteframe_ble::NotificationRef::new(self))
            .try_notify(target, &bytes[..len])
            .map_err(|_| FirstSlotError::Contract)?;
        Ok(FirstSlotOutcome::Admitted(value))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod moteframe_tests {
    use super::*;
    use device_connection::EventPlane;
    use moteframe_ble::{CheckedNotifier, NotificationBackend, NotificationRef, NotifyError};
    fn connected() -> (Bridge<1, 1>, u64, Scope) {
        let bridge = Bridge::new().unwrap();
        let epoch = bridge.begin().unwrap();
        bridge.subscription(epoch, true).unwrap();
        bridge.data_subscription(epoch, true).unwrap();
        let scope = bridge.scope().unwrap().unwrap();
        (bridge, epoch, scope)
    }
    #[test]
    fn test_should_publish_public_notifications_to_the_actual_control_and_data_planes() {
        let (bridge, epoch, scope) = connected();
        for plane in [EventPlane::Control, EventPlane::Data] {
            let mut queue = bridge.event_queue(scope, plane).unwrap();
            let target = queue.target().unwrap();
            assert_eq!(queue.notification_capacity(target), Ok(20));
            CheckedNotifier::new(NotificationRef::new(&mut queue))
                .try_notify(target, &[1, 2, 3])
                .unwrap();
            drop(queue);
            let frame = bridge.outgoing(epoch).unwrap().unwrap();
            assert_eq!(frame.plane, plane);
            assert_eq!(&frame.bytes[..frame.len], &[1, 2, 3]);
        }
    }
    #[test]
    fn test_should_reject_stale_plane_owner_epoch_and_scope_without_publication() {
        let (bridge, epoch, scope) = connected();
        let (other, _, other_scope) = connected();
        let mut queue = bridge.queue(scope).unwrap();
        let control = queue.target().unwrap();
        queue.plane = EventPlane::Data;
        assert_eq!(
            queue.notification_capacity(control),
            Err(PortError::InvalidInput)
        );
        let data = queue.target().unwrap();
        let foreign = other.queue(other_scope).unwrap().target().unwrap();
        for stale in [
            foreign,
            NotificationTarget {
                epoch: data.epoch + 1,
                ..data
            },
            NotificationTarget {
                scope: Scope::new(scope.generation() + 1, 20).unwrap(),
                ..data
            },
        ] {
            assert!(
                CheckedNotifier::new(NotificationRef::new(&mut queue))
                    .try_notify(stale, &[1])
                    .is_err()
            );
        }
        assert!(queue.state.tx.front().is_none());
        drop(queue);
        bridge.data_subscription(epoch, false).unwrap();
        assert!(bridge.queue(scope).is_err());
    }
    #[test]
    fn test_should_keep_first_fragment_capacity_and_admission_transactional() {
        let (bridge, epoch, scope) = connected();
        let mut queue = bridge.queue(scope).unwrap();
        let target = queue.target().unwrap();
        CheckedNotifier::new(NotificationRef::new(&mut queue))
            .try_notify(target, &[7])
            .unwrap();
        assert_eq!(
            CheckedNotifier::new(NotificationRef::new(&mut queue)).try_notify(target, &[8]),
            Err(NotifyError::Backend(PortError::Unavailable))
        );
        let mut filled = false;
        let result = queue
            .with_reserved_slot(|_| {
                filled = true;
                Ok::<_, ()>((1, ()))
            })
            .unwrap();
        assert!(matches!(result, FirstSlotOutcome::NoSlot));
        assert!(!filled);
        drop(queue);
        let frame = bridge.outgoing(epoch).unwrap().unwrap();
        assert_eq!(&frame.bytes[..frame.len], &[7]);
    }
}
