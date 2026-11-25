//! Actual bt-hci command encoders, event decoders and cancellation behavior.
#![allow(clippy::unwrap_used, reason = "Unexpected test setup failure")]
use ble_controller::*;
use bt_hci::{
    cmd::{SyncCmd, controller_baseband::Reset, le::LeReadBufferSize},
    controller::Controller as _,
    data::{AclBroadcastFlag, AclPacket, AclPacketBoundary},
    param::ConnHandle,
};
use std::{
    cell::RefCell,
    collections::VecDeque,
    future::Future,
    pin::pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};
#[derive(Debug, Default)]
struct State {
    events: VecDeque<LinkEvent>,
    sent: Vec<(u32, LinkFrame)>,
    #[cfg(feature = "external-wake")]
    tx_backpressured: bool,
    #[cfg(feature = "external-wake")]
    poll_error: Option<()>,
    adv: usize,
    fail_advertise: bool,
    radio_active: bool,
    stops: usize,
}
#[derive(Clone, Debug, Default)]
struct Backend(Rc<RefCell<State>>);
impl LinkBackend for Backend {
    type Error = ();
    fn reset(&mut self) -> Result<(), ()> {
        self.0.borrow_mut().events.clear();
        self.0.borrow_mut().radio_active = false;
        Ok(())
    }
    fn advertise(&mut self, _: &Advertisement) -> Result<(), ()> {
        let mut state = self.0.borrow_mut();
        state.adv += 1;
        state.radio_active = true;
        if state.fail_advertise {
            Err(())
        } else {
            Ok(())
        }
    }
    fn stop(&mut self) -> Result<(), ()> {
        let mut state = self.0.borrow_mut();
        state.stops += 1;
        state.radio_active = false;
        Ok(())
    }
    fn poll(&mut self) -> Result<Option<LinkEvent>, ()> {
        let mut state = self.0.borrow_mut();
        #[cfg(feature = "external-wake")]
        if let Some(error) = state.poll_error.take() {
            return Err(error);
        }
        Ok(state.events.pop_front())
    }
    fn transmit(&mut self, t: u32, f: LinkFrame) -> Result<TxAdmission, ()> {
        let mut state = self.0.borrow_mut();
        #[cfg(feature = "external-wake")]
        if state.tx_backpressured {
            return Ok(TxAdmission::Busy);
        }
        state.sent.push((t, f));
        Ok(TxAdmission::Committed)
    }
    fn connection_update(&mut self, _: &[u8]) -> Result<(), ()> {
        Ok(())
    }
    fn channel_map(&mut self, _: &[u8]) -> Result<(), ()> {
        Ok(())
    }
}

#[cfg(feature = "external-wake")]
mod external_wake {
    use super::*;
    use bt_hci::{
        cmd::{AsyncCmd, le::LeReadRemoteFeatures, link_control::Disconnect},
        param::DisconnectReason,
    };
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    fn external_hci() -> (HciController<Backend>, Backend) {
        let backend = Backend::default();
        let h = HciController::new_with_external_wake(Controller::new(
            backend.clone(),
            Version::new(8, 0xffff, 1),
        ));
        (h, backend)
    }

    fn connect(backend: &Backend) {
        backend
            .0
            .borrow_mut()
            .events
            .push_back(LinkEvent::Connected(
                Peer::new([7; 6], true),
                ConnectionParameters::new(24, 0, 72).unwrap(),
            ));
    }

    fn external_connected() -> (HciController<Backend>, Backend) {
        let (h, backend) = external_hci();
        run(Reset::new().exec(&h)).unwrap();
        advertise(&h);
        connect(&backend);
        h.service().unwrap();
        let mut buf = h.alloc_buf().unwrap();
        run(h.read(&mut buf)).unwrap();
        (h, backend)
    }

    #[derive(Default)]
    struct WakeCount(AtomicUsize);
    impl std::task::Wake for WakeCount {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn counting_waker() -> (Arc<WakeCount>, Waker) {
        let count = Arc::new(WakeCount::default());
        let waker = Waker::from(count.clone());
        (count, waker)
    }

    #[test]
    fn external_reader_waits_for_board_service_to_stage_an_event() {
        let (h, backend) = external_hci();
        run(Reset::new().exec(&h)).unwrap();
        advertise(&h);
        let borrowed = h.borrowed();
        let mut buf = borrowed.alloc_buf().unwrap();
        let (counter, waker) = counting_waker();
        let mut read = pin!(borrowed.read(&mut buf));
        let mut cx = Context::from_waker(&waker);

        assert!(read.as_mut().poll(&mut cx).is_pending());
        h.service().unwrap();
        run(LeReadBufferSize::new().exec(&borrowed)).unwrap();
        assert!(matches!(
            run(LeReadRemoteFeatures::new(ConnHandle::new(1)).exec(&borrowed)),
            Err(bt_hci::cmd::Error::Hci(bt_hci::param::Error::UNKNOWN_CMD))
        ));
        assert_eq!(counter.0.load(Ordering::Relaxed), 0);

        connect(&backend);
        assert!(read.as_mut().poll(&mut cx).is_pending());
        h.service().unwrap();
        h.service().unwrap();
        assert_eq!(counter.0.load(Ordering::Relaxed), 1);
        assert!(matches!(read.as_mut().poll(&mut cx), Poll::Ready(Ok(
        bt_hci::ControllerToHostPacket::Event(event)
    )) if event.kind == bt_hci::event::EventKind::Le));
        run(Disconnect::new(
            ConnHandle::new(1),
            DisconnectReason::RemoteUserTerminatedConn,
        )
        .exec(&borrowed))
        .unwrap();
        h.service().unwrap();
        assert_eq!(backend.0.borrow().sent[0].1.payload(), &[2, 0x13]);
    }

    #[test]
    fn external_reader_services_bounded_batches_through_masked_events() {
        let (h, backend) = external_connected();
        h.command(0x2001, &[0; 8]).unwrap();
        let parameters = ConnectionParameters::new(24, 0, 72).unwrap();
        let frame = LinkFrame::new(FrameKind::Start, &[0, 0, 4, 0]).unwrap();
        backend
            .0
            .borrow_mut()
            .events
            .extend([LinkEvent::ParametersUpdated(parameters); 16]);
        backend
            .0
            .borrow_mut()
            .events
            .push_back(LinkEvent::Received(frame));
        let (counter, waker) = counting_waker();
        let mut cx = Context::from_waker(&waker);
        let mut buf = h.alloc_buf().unwrap();
        let mut read = pin!(h.read(&mut buf));
        assert!(read.as_mut().poll(&mut cx).is_pending());

        for (remaining, wake_count) in [(9, 1), (1, 2)] {
            h.service().unwrap();
            assert_eq!(backend.0.borrow().events.len(), remaining);
            assert!(read.as_mut().poll(&mut cx).is_pending());
            assert_eq!(counter.0.load(Ordering::Relaxed), wake_count);
        }
        h.service().unwrap();
        assert_eq!(counter.0.load(Ordering::Relaxed), 3);
        assert!(matches!(read.as_mut().poll(&mut cx), Poll::Ready(Ok(
        bt_hci::ControllerToHostPacket::Acl(packet)
    )) if packet.data() == frame.payload()));
    }

    #[test]
    fn external_writer_waits_for_real_ack_at_acl_credit_limit() {
        let (h, backend) = external_connected();
        let packet = AclPacket::new(
            ConnHandle::new(1),
            AclPacketBoundary::FirstNonFlushable,
            AclBroadcastFlag::PointToPoint,
            &[0, 0, 4, 0],
        );
        for _ in 0..4 {
            run(h.write_acl_data(&packet)).unwrap();
        }
        let (counter, waker) = counting_waker();
        let mut cx = Context::from_waker(&waker);
        let mut write = pin!(h.write_acl_data(&packet));
        assert!(write.as_mut().poll(&mut cx).is_pending());
        assert!(matches!(
            run(h.write_acl_data(&packet)),
            Err(HciError::Busy)
        ));
        backend.0.borrow_mut().tx_backpressured = true;
        h.service().unwrap();
        assert!(backend.0.borrow().sent.is_empty());
        backend.0.borrow_mut().tx_backpressured = false;
        h.service().unwrap();

        let token = backend.0.borrow().sent[0].0;
        backend
            .0
            .borrow_mut()
            .events
            .push_back(LinkEvent::Acknowledged(token + 1));
        h.service().unwrap();
        assert_eq!(counter.0.load(Ordering::Relaxed), 0);

        backend
            .0
            .borrow_mut()
            .events
            .push_back(LinkEvent::Acknowledged(token));
        h.service().unwrap();
        assert_eq!(counter.0.load(Ordering::Relaxed), 1);
        assert!(matches!(write.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
        assert_eq!(backend.0.borrow().sent.len(), 2);
    }

    #[test]
    fn external_waiter_cancellation_releases_wakers_and_disconnect_invalidates_write() {
        let (h, backend) = external_connected();
        let packet = AclPacket::new(
            ConnHandle::new(1),
            AclPacketBoundary::FirstNonFlushable,
            AclBroadcastFlag::PointToPoint,
            &[0, 0, 4, 0],
        );
        for _ in 0..4 {
            run(h.write_acl_data(&packet)).unwrap();
        }
        let (cancelled, old_waker) = counting_waker();
        let mut buf = h.alloc_buf().unwrap();
        {
            let mut read = pin!(h.read(&mut buf));
            let mut write = pin!(h.write_acl_data(&packet));
            let mut cx = Context::from_waker(&old_waker);
            assert!(read.as_mut().poll(&mut cx).is_pending());
            assert!(write.as_mut().poll(&mut cx).is_pending());
        }
        assert_eq!(Arc::strong_count(&cancelled), 2);

        let (counter, waker) = counting_waker();
        let mut write = pin!(h.write_acl_data(&packet));
        let mut cx = Context::from_waker(&waker);
        assert!(write.as_mut().poll(&mut cx).is_pending());
        backend
            .0
            .borrow_mut()
            .events
            .push_back(LinkEvent::Disconnected(0x13));
        h.service().unwrap();
        assert_eq!(counter.0.load(Ordering::Relaxed), 1);
        assert_eq!(cancelled.0.load(Ordering::Relaxed), 0);
        h.command(0x200a, &[1]).unwrap();
        connect(&backend);
        h.service().unwrap();
        assert!(backend.0.borrow().sent.is_empty());
        let next = AclPacket::new(
            ConnHandle::new(2),
            AclPacketBoundary::FirstNonFlushable,
            AclBroadcastFlag::PointToPoint,
            &[0, 0, 4, 0],
        );
        for _ in 0..4 {
            run(h.write_acl_data(&next)).unwrap();
        }
        let (next_counter, next_waker) = counting_waker();
        let mut next_cx = Context::from_waker(&next_waker);
        let mut next_write = pin!(h.write_acl_data(&next));
        assert!(next_write.as_mut().poll(&mut next_cx).is_pending());
        assert!(matches!(
            write.as_mut().poll(&mut cx),
            Poll::Ready(Err(HciError::Controller(ControllerError::State)))
        ));
        h.service().unwrap();
        let token = backend.0.borrow().sent[0].0;
        backend
            .0
            .borrow_mut()
            .events
            .push_back(LinkEvent::Acknowledged(token));
        h.service().unwrap();
        assert_eq!(next_counter.0.load(Ordering::Relaxed), 1);
        assert!(matches!(
            next_write.as_mut().poll(&mut next_cx),
            Poll::Ready(Ok(()))
        ));
    }

    #[test]
    fn external_reader_waits_for_reset_then_observes_service_fault() {
        let (h, backend) = external_hci();
        let (counter, waker) = counting_waker();
        let mut cx = Context::from_waker(&waker);
        let mut buf = h.alloc_buf().unwrap();
        let mut read = pin!(h.read(&mut buf));
        assert!(read.as_mut().poll(&mut cx).is_pending());
        h.service().unwrap();
        run(Reset::new().exec(&h)).unwrap();
        assert_eq!(counter.0.load(Ordering::Relaxed), 0);
        backend.0.borrow_mut().poll_error = Some(());
        assert!(matches!(
            h.service(),
            Err(HciError::Controller(ControllerError::Backend(())))
        ));
        assert!(matches!(
            h.service(),
            Err(HciError::Controller(ControllerError::Faulted))
        ));
        assert_eq!(counter.0.load(Ordering::Relaxed), 1);
        assert!(matches!(
            read.as_mut().poll(&mut cx),
            Poll::Ready(Err(HciError::Controller(ControllerError::Faulted)))
        ));
    }
}

fn run<F: Future>(f: F) -> F::Output {
    let mut f = pin!(f);
    match f.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(v) => v,
        Poll::Pending => panic!("expected immediate readiness"),
    }
}
fn setup() -> (HciController<Backend>, Backend) {
    let b = Backend::default();
    let h = HciController::new(Controller::new(b.clone(), Version::new(0xff, 0xffff, 1)));
    run(Reset::new().exec(&h)).unwrap();
    (h, b)
}
fn configure(h: &HciController<Backend>) {
    h.command(0x0c01, &u64::MAX.to_le_bytes()).unwrap();
    h.command(0x2001, &u64::MAX.to_le_bytes()).unwrap();
    h.command(0x2005, &[1, 2, 3, 4, 5, 0xc0]).unwrap();
    h.command(0x2006, &[160, 0, 160, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 7, 0])
        .unwrap();
}
fn advertise(h: &HciController<Backend>) {
    configure(h);
    h.command(0x200a, &[1]).unwrap();
}
#[test]
fn test_should_use_real_typed_startup_commands_and_reject_unsupported_before_side_effects() {
    let (h, b) = setup();
    let r = run(LeReadBufferSize::new().exec(&h)).unwrap();
    let len = r.le_acl_data_packet_length;
    assert_eq!(len, 27);
    assert_eq!(r.total_num_le_acl_data_packets, 4);
    assert!(h.command(0x2006, &[0; 15]).is_err());
    assert!(h.command(0x2041, &[]).is_err());
    assert_eq!(b.0.borrow().adv, 0);
    advertise(&h);
    assert_eq!(b.0.borrow().adv, 1);
    assert!(h.command(0x2005, &[6, 5, 4, 3, 2, 0xc0]).is_err());
}
#[test]
fn test_should_deliver_real_hci_connection_acl_and_ack_credit_events() {
    let (h, b) = setup();
    advertise(&h);
    b.0.borrow_mut().events.push_back(LinkEvent::Connected(
        Peer::new([7; 6], true),
        ConnectionParameters::new(24, 0, 72).unwrap(),
    ));
    let mut buf = h.alloc_buf().unwrap();
    let connection = run(h.read(&mut buf)).unwrap();
    assert!(matches!(
        connection,
        bt_hci::ControllerToHostPacket::Event(_)
    ));
    let frame = LinkFrame::new(FrameKind::Start, &[1, 0, 4, 0, 0x0a]).unwrap();
    b.0.borrow_mut()
        .events
        .push_back(LinkEvent::Received(frame));
    match run(h.read(&mut buf)).unwrap() {
        bt_hci::ControllerToHostPacket::Acl(p) => {
            assert_eq!(p.handle().into_inner(), 1);
            assert_eq!(p.boundary_flag(), AclPacketBoundary::FirstFlushable);
            assert_eq!(p.data(), frame.payload());
        }
        _ => panic!("expected ACL"),
    }
    let packet = AclPacket::new(
        ConnHandle::new(1),
        AclPacketBoundary::FirstNonFlushable,
        AclBroadcastFlag::PointToPoint,
        frame.payload(),
    );
    run(h.write_acl_data(&packet)).unwrap();
    h.service().unwrap();
    assert_eq!(b.0.borrow().sent.len(), 1);
    h.command(0x0c01, &[0; 8]).unwrap(); // Credit accounting cannot be masked away.
    let token = b.0.borrow().sent[0].0;
    b.0.borrow_mut()
        .events
        .push_back(LinkEvent::Acknowledged(token));
    let event = run(h.read(&mut buf)).unwrap();
    assert!(
        matches!(event,bt_hci::ControllerToHostPacket::Event(e) if e.kind==bt_hci::event::EventKind::NumberOfCompletedPackets)
    );
}
#[test]
fn test_should_release_cancelled_read_and_reject_invalid_acl_without_ownership_transfer() {
    let (h, _) = setup();
    let mut a = h.alloc_buf().unwrap();
    let mut b = h.alloc_buf().unwrap();
    {
        let mut pending = pin!(h.read(&mut a));
        assert!(
            pending
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
        assert!(run(h.read(&mut b)).is_err());
    }
    let mut pending = pin!(h.read(&mut b));
    assert!(
        pending
            .as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    let packet = AclPacket::new(
        ConnHandle::new(1),
        AclPacketBoundary::Complete,
        AclBroadcastFlag::PointToPoint,
        &[0; 4],
    );
    assert!(run(h.write_acl_data(&packet)).is_err());
}

#[test]
fn test_should_wait_for_host_reset_when_rx_future_is_polled_first() {
    let h = HciController::new(Controller::new(
        Backend::default(),
        Version::new(8, 0xffff, 1),
    ));
    let mut buf = h.alloc_buf().unwrap();
    let mut read = pin!(h.read(&mut buf));
    assert!(
        read.as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
    run(Reset::new().exec(&h)).unwrap();
    assert!(
        read.as_mut()
            .poll(&mut Context::from_waker(Waker::noop()))
            .is_pending()
    );
}

#[test]
fn test_should_keep_partial_advertising_failure_latched_even_for_disable() {
    let (h, b) = setup();
    configure(&h);
    b.0.borrow_mut().fail_advertise = true;
    assert!(h.command(0x200a, &[1]).is_err());
    assert!(b.0.borrow().radio_active);
    assert!(matches!(
        h.command(0x200a, &[0]),
        Err(bt_hci::cmd::Error::Io(HciError::Controller(
            ControllerError::Faulted
        )))
    ));
    assert!(h.command(0x2005, &[6, 5, 4, 3, 2, 0xc0]).is_err());
    assert_eq!(b.0.borrow().stops, 0);
    run(Reset::new().exec(&h)).unwrap();
    assert!(!b.0.borrow().radio_active);
    assert!(h.command(0x200a, &[0]).is_ok());
}
std::thread_local! {static REENTER: RefCell<Option<Rc<HciController<Backend>>>>=const {RefCell::new(None)};}
struct DropReentry;
impl std::task::Wake for DropReentry {
    fn wake(self: std::sync::Arc<Self>) {
        REENTER.with(|slot| {
            if let Some(h) = slot.borrow().as_ref() {
                assert!(h.command(0x2002, &[]).is_ok());
            }
        });
    }
}
impl Drop for DropReentry {
    fn drop(&mut self) {
        REENTER.with(|slot| {
            if let Some(h) = slot.borrow().as_ref() {
                assert!(h.command(0x2002, &[]).is_ok());
            }
        });
    }
}
#[test]
fn test_should_replace_cancelled_reader_waker_without_borrow_during_destructor_reentry() {
    let (h, _) = setup();
    let h = Rc::new(h);
    REENTER.with(|slot| *slot.borrow_mut() = Some(h.clone()));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut a = h.alloc_buf().unwrap();
        let w = Waker::from(std::sync::Arc::new(DropReentry));
        {
            let mut first = pin!(h.read(&mut a));
            assert!(
                first
                    .as_mut()
                    .poll(&mut Context::from_waker(&w))
                    .is_pending()
            );
        }
        drop(w); // The adapter now owns the last safe Wake reference.
        let mut b = h.alloc_buf().unwrap();
        let mut second = pin!(h.read(&mut b));
        assert!(
            second
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
    }));
    REENTER.with(|slot| {
        slot.borrow_mut().take();
    });
    assert!(result.is_ok());
}

#[test]
fn test_should_replace_cancelled_writer_waker_without_borrow_during_destructor_reentry() {
    let (h, backend) = setup();
    advertise(&h);
    backend
        .0
        .borrow_mut()
        .events
        .push_back(LinkEvent::Connected(
            Peer::new([1; 6], true),
            ConnectionParameters::new(24, 0, 72).unwrap(),
        ));
    let mut buf = h.alloc_buf().unwrap();
    run(h.read(&mut buf)).unwrap();
    let h = Rc::new(h);
    REENTER.with(|slot| *slot.borrow_mut() = Some(h.clone()));
    let packet = AclPacket::new(
        ConnHandle::new(1),
        AclPacketBoundary::FirstNonFlushable,
        AclBroadcastFlag::PointToPoint,
        &[0, 0, 4, 0],
    );
    for _ in 0..4 {
        run(h.write_acl_data(&packet)).unwrap();
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let w = Waker::from(std::sync::Arc::new(DropReentry));
        {
            let mut first = pin!(h.write_acl_data(&packet));
            assert!(
                first
                    .as_mut()
                    .poll(&mut Context::from_waker(&w))
                    .is_pending()
            );
        }
        drop(w);
        let mut second = pin!(h.write_acl_data(&packet));
        assert!(
            second
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending()
        );
    }));
    REENTER.with(|slot| {
        slot.borrow_mut().take();
    });
    assert!(result.is_ok());
}
