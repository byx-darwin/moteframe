//! Real upstream Host and GATT processing against a scripted link peer.
#![allow(clippy::unwrap_used, reason = "Unexpected fixture or protocol failure")]
use ble_controller::{
    ConnectionParameters, Controller, FrameKind, HciController, LinkBackend, LinkEvent, LinkFrame,
    Peer, RandomAddress, TxAdmission, Version,
};
use ble_host_trouble::{Resources, build, host::prelude::*};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    future::Future,
    pin::pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};
#[derive(Debug, Default)]
struct State {
    advertised: bool,
    events: VecDeque<LinkEvent>,
    sent: Vec<LinkFrame>,
}
#[derive(Clone, Debug, Default)]
struct Backend(Rc<RefCell<State>>);
impl LinkBackend for Backend {
    type Error = ();
    fn reset(&mut self) -> Result<(), ()> {
        self.0.borrow_mut().events.clear();
        Ok(())
    }
    fn advertise(&mut self, _: &ble_controller::Advertisement) -> Result<(), ()> {
        self.0.borrow_mut().advertised = true;
        Ok(())
    }
    fn stop(&mut self) -> Result<(), ()> {
        Ok(())
    }
    fn poll(&mut self) -> Result<Option<LinkEvent>, ()> {
        Ok(self.0.borrow_mut().events.pop_front())
    }
    fn transmit(&mut self, t: u32, p: LinkFrame) -> Result<TxAdmission, ()> {
        let mut s = self.0.borrow_mut();
        s.sent.push(p);
        s.events.push_back(LinkEvent::Acknowledged(t));
        Ok(TxAdmission::Committed)
    }
    fn connection_update(&mut self, _: &[u8]) -> Result<(), ()> {
        Ok(())
    }
    fn channel_map(&mut self, _: &[u8]) -> Result<(), ()> {
        Ok(())
    }
}
fn inject(b: &Backend, att: &[u8]) {
    let mut payload = vec![u8::try_from(att.len()).unwrap(), 0, 4, 0];
    payload.extend_from_slice(att);
    b.0.borrow_mut().events.push_back(LinkEvent::Received(
        LinkFrame::new(FrameKind::Start, &payload).unwrap(),
    ));
}
fn count(b: &Backend, opcode: u8) -> usize {
    b.0.borrow()
        .sent
        .iter()
        .filter(|f| f.kind() == FrameKind::Start && f.payload().get(4) == Some(&opcode))
        .count()
}
#[test]
#[allow(
    clippy::too_many_lines,
    reason = "One ordered real-Host conversation keeps each phase observable"
)]
fn test_should_run_upstream_host_mtu_read_write_subscription_notification_and_disconnect() {
    let b = Backend::default();
    let h = HciController::new(Controller::new(b.clone(), Version::new(0x08, 0xffff, 1)));
    let mut resources = Resources::new();
    let stack = build(
        h,
        &mut resources,
        RandomAddress::new([1, 2, 3, 4, 5, 0xc0]).unwrap(),
    );
    let mut table: AttributeTable<'_, NoopRawMutex, 12> = AttributeTable::new();
    table
        .add_service(Service::new(0x1800_u16))
        .add_characteristic_ro(0x2a00_u16, b"lab");
    table.add_service(Service::new(0x1801_u16));
    let mut storage = [0; 1];
    let value: Characteristic<u8> = table
        .add_service(Service::new(0xfff0_u16))
        .add_characteristic(
            0xfff1_u16,
            [
                CharacteristicProp::Read,
                CharacteristicProp::Write,
                CharacteristicProp::Notify,
            ],
            0x11,
            &mut storage,
        )
        .build();
    let handle = value.handle.to_le_bytes();
    let cccd = value.cccd_handle.unwrap().to_le_bytes();
    let server =
        AttributeServer::<NoopRawMutex, ble_host_trouble::HostPacketPool, 12, 1>::new(table);
    let mut peripheral = stack.peripheral();
    let mut runner = stack.runner();
    let connected = Cell::new(false);
    let wrote = Cell::new(false);
    let mut run = pin!(runner.run());
    let mut app = pin!(async {
        let acceptor = peripheral
            .advertise(
                &AdvertisementParameters::default(),
                Advertisement::ConnectableScannableUndirected {
                    adv_data: &[2, 1, 6],
                    scan_data: &[],
                },
            )
            .await
            .unwrap();
        let conn = acceptor
            .accept()
            .await
            .unwrap()
            .with_attribute_server(&server)
            .unwrap();
        connected.set(true);
        loop {
            match conn.next().await {
                GattConnectionEvent::Gatt { event } => {
                    let is_value = matches!(&event,GattEvent::Write(w) if w.handle()==value.handle);
                    event.accept().unwrap().send().await;
                    if is_value {
                        assert_eq!(conn.get(&value).unwrap(), 0x42);
                        wrote.set(true);
                        value.notify(&conn, &0xa5, false).await.unwrap();
                    }
                }
                GattConnectionEvent::Disconnected { .. } => break,
                _ => {}
            }
        }
    });
    let mut stage = 0;
    let mut completed = false;
    for _ in 0..512 {
        let mut cx = Context::from_waker(Waker::noop());
        if let Poll::Ready(result) = run.as_mut().poll(&mut cx) {
            panic!("Host exited: {result:?}");
        }
        if app.as_mut().poll(&mut cx).is_ready() {
            completed = true;
            break;
        }
        match stage {
            0 if b.0.borrow().advertised => {
                b.0.borrow_mut().events.push_back(LinkEvent::Connected(
                    Peer::new([7; 6], true),
                    ConnectionParameters::new(24, 0, 72).unwrap(),
                ));
                stage = 1;
            }
            1 if connected.get() => {
                inject(&b, &[2, 64, 0]);
                stage = 2;
            }
            2 if count(&b, 3) == 1 => {
                inject(&b, &[0x0a, handle[0], handle[1]]);
                stage = 3;
            }
            3 if count(&b, 0x0b) == 1 => {
                inject(&b, &[0x12, cccd[0], cccd[1], 1, 0]);
                stage = 4;
            }
            4 if count(&b, 0x13) == 1 => {
                inject(&b, &[0x12, handle[0], handle[1], 0x42]);
                stage = 5;
            }
            5 if count(&b, 0x13) == 2 && count(&b, 0x1b) == 1 => {
                b.0.borrow_mut()
                    .events
                    .push_back(LinkEvent::Disconnected(0x13));
                stage = 6;
            }
            _ => {}
        }
    }
    assert!(completed, "real Host stalled at script stage {stage}");
    assert!(wrote.get());
    let capture = b.0.borrow();
    let data: Vec<&[u8]> = capture
        .sent
        .iter()
        .filter(|f| f.kind() == FrameKind::Start)
        .map(|f| &f.payload()[4..])
        .collect();
    let mtu = if cfg!(feature = "extended-mtu") {
        64
    } else {
        23
    };
    assert!(data.contains(&&[3, mtu, 0][..]));
    assert!(data.contains(&&[0x0b, 0x11][..]));
    assert!(data.contains(&&[0x1b, handle[0], handle[1], 0xa5][..]));
}

#[test]
fn test_should_propagate_malformed_link_control_through_the_actual_host_runner() {
    let backend = Backend::default();
    let controller =
        HciController::new(Controller::new(backend.clone(), Version::new(6, 0xffff, 1)));
    let mut resources = Resources::new();
    let stack = build(
        controller,
        &mut resources,
        RandomAddress::new([1, 2, 3, 4, 5, 0xc0]).unwrap(),
    );
    let mut runner = stack.runner();
    let mut peripheral = stack.peripheral();
    let mut run = pin!(runner.run());
    let mut advertise = pin!(async {
        let acceptor = peripheral
            .advertise(
                &AdvertisementParameters::default(),
                Advertisement::ConnectableScannableUndirected {
                    adv_data: &[2, 1, 6],
                    scan_data: &[],
                },
            )
            .await
            .unwrap();
        core::future::pending::<()>().await;
        drop(acceptor);
    });
    let mut injected = false;
    let mut faulted = false;
    for _ in 0..64 {
        let mut cx = Context::from_waker(Waker::noop());
        if let Poll::Ready(result) = run.as_mut().poll(&mut cx) {
            assert!(matches!(
                result,
                Err(BleHostError::Controller(
                    ble_controller::HciError::Controller(ble_controller::ControllerError::Protocol)
                ))
            ));
            faulted = true;
            break;
        }
        assert!(advertise.as_mut().poll(&mut cx).is_pending());
        if backend.0.borrow().advertised && !injected {
            let mut state = backend.0.borrow_mut();
            state.events.push_back(LinkEvent::Connected(
                Peer::new([7; 6], true),
                ConnectionParameters::new(24, 0, 72).unwrap(),
            ));
            state.events.push_back(LinkEvent::Received(
                LinkFrame::new(FrameKind::Control, &[0]).unwrap(),
            ));
            injected = true;
        }
    }
    assert!(
        faulted,
        "malformed control did not reach the actual Host error boundary"
    );
}
