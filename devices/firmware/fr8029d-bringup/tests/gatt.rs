//! Exercise the FR profile on the actual Host with a scripted radio boundary.
#![allow(clippy::unwrap_used, reason = "Unexpected fixture or protocol failure")]

use ble_controller::{
    ConnectionParameters, Controller, FrameKind, HciController, LinkBackend, LinkEvent, LinkFrame,
    Peer, RandomAddress, TxAdmission, Version,
};
use ble_host_trouble::{Resources, build, host::prelude::*};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use fr8029d_bringup::gatt::{GattProfile, GattProgress};
use fr8029d_bringup::gatt_runner::{CommittedValue, GattIdentity, GattReport, GattRunError, run};
use moteframe_ble::WriteSlot;
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    future::Future,
    pin::{Pin, pin},
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Wake, Waker},
};

// The pinned Host configuration has one global four-packet pool.
static HOST_FIXTURE: Mutex<()> = Mutex::new(());

#[derive(Debug, Default)]
struct RadioState {
    advertising: bool,
    advertisement: Option<ble_controller::Advertisement>,
    incoming: VecDeque<LinkEvent>,
    replies: VecDeque<Vec<u8>>,
}

#[derive(Clone, Debug, Default)]
struct ScriptedRadio(Rc<RefCell<RadioState>>);

impl LinkBackend for ScriptedRadio {
    type Error = ();

    fn reset(&mut self) -> Result<(), ()> {
        *self.0.borrow_mut() = RadioState::default();
        Ok(())
    }

    fn advertise(&mut self, advertisement: &ble_controller::Advertisement) -> Result<(), ()> {
        let mut state = self.0.borrow_mut();
        state.advertising = true;
        state.advertisement = Some(*advertisement);
        Ok(())
    }

    fn stop(&mut self) -> Result<(), ()> {
        self.0.borrow_mut().advertising = false;
        Ok(())
    }

    fn poll(&mut self) -> Result<Option<LinkEvent>, ()> {
        Ok(self.0.borrow_mut().incoming.pop_front())
    }

    fn transmit(&mut self, token: u32, frame: LinkFrame) -> Result<TxAdmission, ()> {
        let mut state = self.0.borrow_mut();
        if frame.kind() == FrameKind::Start && frame.payload()[2..4] == [4, 0] {
            state.replies.push_back(frame.payload()[4..].to_vec());
        }
        state.incoming.push_back(LinkEvent::Acknowledged(token));
        Ok(TxAdmission::Committed)
    }

    fn connection_update(&mut self, _: &[u8]) -> Result<(), ()> {
        Ok(())
    }

    fn channel_map(&mut self, _: &[u8]) -> Result<(), ()> {
        Ok(())
    }
}

impl ScriptedRadio {
    fn event(&self, event: LinkEvent) {
        self.0.borrow_mut().incoming.push_back(event);
    }

    fn inject_att(&self, request: &[u8]) {
        let mut payload = vec![u8::try_from(request.len()).unwrap(), 0, 4, 0];
        payload.extend_from_slice(request);
        self.event(LinkEvent::Received(
            LinkFrame::new(FrameKind::Start, &payload).unwrap(),
        ));
    }

    fn exchange(&self, pump: &mut impl FnMut(), request: &[u8], response: &[u8]) {
        self.inject_att(request);
        pump_until(pump, || !self.0.borrow().replies.is_empty());
        let actual = self.0.borrow_mut().replies.pop_front().unwrap();
        assert_eq!(actual, response, "ATT request {request:02x?}");
    }
}

fn pump_until(pump: &mut impl FnMut(), done: impl Fn() -> bool) {
    for _ in 0..512 {
        pump();
        if done() {
            return;
        }
    }
    panic!("scripted link stopped making progress");
}

#[test]
fn test_should_leave_a_short_attribute_table_untouched() {
    let mut storage = [0; fr8029d_bringup::gatt::VALUE_LEN];
    let mut table: AttributeTable<'_, NoopRawMutex, 11> = AttributeTable::new();
    assert!(GattProfile::add(&mut table, &mut storage).is_err());
    assert_eq!(table.len(), 0);
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "The request and response sequence stays visible as one connection"
)]
fn test_should_discover_read_and_commit_only_complete_write_requests() {
    let _fixture = HOST_FIXTURE.lock().unwrap();
    let radio = ScriptedRadio::default();
    let hci = HciController::new(Controller::new(radio.clone(), Version::new(8, 0xffff, 1)));
    let mut resources = Resources::new();
    let stack = build(
        hci,
        &mut resources,
        RandomAddress::new([1, 2, 3, 4, 5, 0xc0]).unwrap(),
    );
    let mut storage = [0xa5; fr8029d_bringup::gatt::VALUE_LEN];
    let mut table: AttributeTable<'_, NoopRawMutex, 12> = AttributeTable::new();
    let profile = GattProfile::add(&mut table, &mut storage).unwrap();
    let server =
        AttributeServer::<NoopRawMutex, ble_host_trouble::HostPacketPool, 12, 1>::new(table);
    let mut runner = stack.runner();
    let mut peripheral = stack.peripheral();
    let connected = Cell::new(false);
    let disconnected = Cell::new(false);
    let committed = Cell::new(0);
    let delivered = Cell::new(None);
    let mut host = pin!(runner.run());
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
        let connection = acceptor
            .accept()
            .await
            .unwrap()
            .with_attribute_server(&server)
            .unwrap();
        let mut writes = WriteSlot::new();
        connected.set(true);
        loop {
            match profile
                .handle(connection.next().await, &mut writes)
                .await
                .unwrap()
            {
                GattProgress::Continue | GattProgress::SdkSubscribed => {}
                GattProgress::Committed => committed.set(committed.get() + 1),
                GattProgress::Disconnected => break,
            }
        }
        let mut bytes = [0; 8];
        delivered.set(Some((
            writes.take_into(&mut bytes).unwrap().unwrap(),
            bytes,
        )));
        disconnected.set(true);
    });
    let mut pump = || {
        let mut context = Context::from_waker(Waker::noop());
        assert!(host.as_mut().poll(&mut context).is_pending());
        let _ = app.as_mut().poll(&mut context);
    };
    pump_until(&mut pump, || radio.0.borrow().advertising);
    radio.event(LinkEvent::Connected(
        Peer::new([7; 6], true),
        ConnectionParameters::new(24, 0, 72).unwrap(),
    ));
    pump_until(&mut pump, || connected.get());

    let mtu = if cfg!(feature = "ble-gatt-dle") {
        64
    } else {
        23
    };
    radio.exchange(&mut pump, &[0x02, 64, 0], &[0x03, mtu, 0]);
    radio.exchange(
        &mut pump,
        &[0x10, 1, 0, 0xff, 0xff, 0x00, 0x28],
        &[
            0x11, 20, 1, 0, 4, 0, 0x51, 0xb2, 0x35, 0xc7, 0xeb, 0xd6, 0x4e, 0xa6, 0xb0, 0x48, 0xf7,
            0x7f, 0xd0, 0x29, 0x80, 0x2f,
        ],
    );
    radio.exchange(
        &mut pump,
        &[0x08, 1, 0, 0xff, 0xff, 0x03, 0x28],
        &[
            0x09, 21, 2, 0, 0x1a, 3, 0, 0x51, 0xb2, 0x35, 0xc7, 0xeb, 0xd6, 0x4e, 0xa6, 0xb0, 0x48,
            0xf7, 0x7f, 0xd1, 0x29, 0x80, 0x2f,
        ],
    );
    radio.exchange(&mut pump, &[0x0a, 3, 0], &[0x0b, 0, 0, 0, 0, 0, 0, 0, 0]);
    radio.exchange(&mut pump, &[0x12, 3, 0], &[0x01, 0x12, 3, 0, 0x0d]);
    radio.exchange(
        &mut pump,
        &[0x16, 3, 0, 0, 0, 0xff],
        &[0x01, 0x16, 0, 0, 0x06],
    );
    radio.inject_att(&[0x52, 3, 0, 9, 8, 7, 6, 5, 4, 3, 2]);
    radio.exchange(&mut pump, &[0x0a, 3, 0], &[0x0b, 0, 0, 0, 0, 0, 0, 0, 0]);
    assert_eq!(committed.get(), 0);

    radio.exchange(&mut pump, &[0x12, 3, 0, 1, 3, 5, 7, 2, 4, 6, 8], &[0x13]);
    radio.exchange(
        &mut pump,
        &[0x12, 3, 0, 8, 6, 4, 2, 7, 5, 3, 1],
        &[0x01, 0x12, 3, 0, 0x11],
    );
    radio.exchange(&mut pump, &[0x0a, 3, 0], &[0x0b, 1, 3, 5, 7, 2, 4, 6, 8]);
    assert_eq!(committed.get(), 1);

    radio.event(LinkEvent::Disconnected(0x13));
    pump_until(&mut pump, || disconnected.get());
    let (meta, bytes) = delivered.get().unwrap();
    assert_eq!(meta.target, 3);
    assert_eq!(meta.length, 8);
    assert_eq!(bytes, [1, 3, 5, 7, 2, 4, 6, 8]);
}

#[test]
fn test_should_report_committed_writes_and_resume_advertising_on_the_real_runner() {
    exercise_runner(false);
}

#[cfg(feature = "ble-gatt-event-driven")]
#[test]
fn externally_serviced_runner_delivers_notifications_and_clears_cccd_on_reconnect() {
    exercise_runner(true);
}

#[cfg(feature = "ble-gatt-event-driven")]
#[test]
fn externally_serviced_runner_re_advertises_if_att_reply_is_pending_at_disconnect() {
    let _fixture = HOST_FIXTURE.lock().unwrap();
    let radio = ScriptedRadio::default();
    let mut resources = Resources::new();
    let reports = RefCell::new(Vec::new());
    let identity = GattIdentity {
        address: RandomAddress::new([1, 2, 3, 4, 5, 0xc0]).unwrap(),
        version: Version::new(8, 0xffff, 1),
        name: "FR29-GATT",
        advertising_data: &[2, 1, 6],
    };
    let mut app = pin!(run_fixture(
        true,
        radio.clone(),
        &mut resources,
        identity,
        |report| reports.borrow_mut().push(report),
    ));
    let mut pump = fixture_pump(app.as_mut(), &radio, true);
    let peer = Peer::new([7; 6], true);
    let parameters = ConnectionParameters::new(24, 0, 72).unwrap();
    pump_until(&mut pump, || reports.borrow().len() == 1);
    radio.event(LinkEvent::Connected(peer, parameters));
    pump_until(&mut pump, || reports.borrow().len() == 2);
    radio.exchange(&mut pump, &[0x12, 10, 0, 1, 0], &[0x13]);
    radio.inject_att(&[0x12, 9, 0, 1, 2, 3, 4, 5, 6, 7, 8]);
    pump_until(&mut pump, || reports.borrow().len() == 3);
    assert!(matches!(reports.borrow()[2], GattReport::Committed(_)));
    assert!(radio.0.borrow().replies.is_empty());

    radio.event(LinkEvent::Disconnected(0x13));
    pump_until(&mut pump, || radio.0.borrow().advertising);
    assert_eq!(
        reports.borrow()[3..],
        [GattReport::Disconnected, GattReport::Advertising]
    );
    radio.event(LinkEvent::Connected(peer, parameters));
    pump_until(&mut pump, || reports.borrow().len() == 6);
    radio.exchange(&mut pump, &[0x0a, 10, 0], &[0x0b, 0, 0]);
    radio.exchange(&mut pump, &[0x12, 10, 0, 1, 0], &[0x13]);
    radio.exchange(&mut pump, &[0x12, 9, 0, 8, 7, 6, 5, 4, 3, 2, 1], &[0x13]);
    pump_until(&mut pump, || !radio.0.borrow().replies.is_empty());
    assert_eq!(
        radio.0.borrow_mut().replies.pop_front().unwrap(),
        [0x1b, 9, 0, 8, 7, 6, 5, 4, 3, 2, 1]
    );
}

#[test]
fn exhausted_reply_or_notify_pool_recovers_and_accepts_next_client() {
    use ble_host_trouble::{HostPacketPool, host::PacketPool};

    let _fixture = HOST_FIXTURE.lock().unwrap();
    for fail_notify in [false, true] {
        let radio = ScriptedRadio::default();
        let mut resources = Resources::new();
        let reports = RefCell::new(Vec::new());
        let held = RefCell::new(Vec::new());
        let injected = Cell::new(false);
        let identity = GattIdentity {
            address: RandomAddress::new([1, 2, 3, 4, 5, 0xc0]).unwrap(),
            version: Version::new(8, 0xffff, 1),
            name: "FR29-GATT",
            advertising_data: &[2, 1, 6],
        };
        let external = cfg!(feature = "ble-gatt-event-driven");
        let mut app = pin!(run_fixture(
            external,
            radio.clone(),
            &mut resources,
            identity,
            |event| {
                if fail_notify
                    && matches!(event, GattReport::Committed(_))
                    && !injected.replace(true)
                {
                    while let Some(packet) = HostPacketPool::allocate() {
                        held.borrow_mut().push(packet);
                    }
                }
                reports.borrow_mut().push(event);
            }
        ));
        // Model periodic connection-event ticks, including empty radio events.
        let mut pump = fixture_pump(app.as_mut(), &radio, false);
        let connected = LinkEvent::Connected(
            Peer::new([7; 6], true),
            ConnectionParameters::new(24, 0, 72).unwrap(),
        );
        pump_until(&mut pump, || reports.borrow().len() == 1);
        radio.event(connected);
        pump_until(&mut pump, || reports.borrow().len() == 2);
        radio.exchange(&mut pump, &[0x12, 10, 0, 1, 0], &[0x13]);
        if !fail_notify {
            // Leave room for the incoming ATT request but not its reply.
            for _ in 1..HostPacketPool::capacity() {
                held.borrow_mut().push(HostPacketPool::allocate().unwrap());
            }
        }
        radio.inject_att(&[0x12, 9, 0, 42]);
        pump_until(&mut pump, || {
            reports.borrow().last() == Some(&GattReport::Advertising)
        });
        assert!(!held.borrow().is_empty());
        held.borrow_mut().clear();
        radio.0.borrow_mut().replies.clear();
        radio.event(connected);
        pump_until(&mut pump, || {
            reports.borrow().last() == Some(&GattReport::Connected)
        });
        radio.exchange(&mut pump, &[0x0a, 10, 0], &[0x0b, 0, 0]);
        radio.exchange(&mut pump, &[0x12, 10, 0, 1, 0], &[0x13]);
        radio.exchange(&mut pump, &[0x12, 9, 0, 43], &[0x13]);
        pump_until(&mut pump, || !radio.0.borrow().replies.is_empty());
        assert_eq!(
            radio.0.borrow_mut().replies.pop_front().unwrap(),
            [0x1b, 9, 0, 43]
        );
    }
}

async fn run_fixture(
    event_driven: bool,
    radio: ScriptedRadio,
    resources: &mut Resources,
    identity: GattIdentity,
    report: impl FnMut(GattReport),
) -> GattRunError<()> {
    #[cfg(feature = "ble-gatt-event-driven")]
    if event_driven {
        return fr8029d_bringup::gatt_runner::run_event_driven(radio, resources, identity, report)
            .await;
    }
    let _ = event_driven;
    run(radio, resources, identity, report).await
}

fn fixture_pump<'a>(
    mut app: Pin<&'a mut impl Future<Output = GattRunError<()>>>,
    radio: &'a ScriptedRadio,
    event_driven: bool,
) -> impl FnMut() + 'a {
    struct Ready(AtomicBool);
    impl Wake for Ready {
        fn wake(self: Arc<Self>) {
            self.0.store(true, Ordering::Release);
        }
    }
    let ready = Arc::new(Ready(AtomicBool::new(true)));
    let waker = Waker::from(Arc::clone(&ready));
    move || {
        if event_driven
            && !ready.0.swap(false, Ordering::AcqRel)
            && radio.0.borrow().incoming.is_empty()
        {
            return;
        }
        let mut context = Context::from_waker(if event_driven { &waker } else { Waker::noop() });
        if let std::task::Poll::Ready(error) = app.as_mut().poll(&mut context) {
            panic!("board runner exited with {error:?}");
        }
    }
}

fn expected_commit(value: &[u8]) -> GattReport {
    let mut bytes = [0; fr8029d_bringup::gatt::VALUE_LEN];
    bytes[..value.len()].copy_from_slice(value);
    GattReport::Committed(CommittedValue {
        handle: 9,
        length: value.len(),
        bytes,
    })
}

fn check_sdk_service(radio: &ScriptedRadio, pump: &mut impl FnMut()) {
    radio.exchange(
        pump,
        &[0x10, 11, 0, 0xff, 0xff, 0x00, 0x28],
        &[0x11, 6, 11, 0, 0xff, 0xff, 0xf0, 0xff],
    );
    radio.exchange(
        pump,
        &[0x08, 11, 0, 18, 0, 0x03, 0x28],
        &[0x09, 7, 12, 0, 0x1a, 13, 0, 0xf3, 0xff],
    );
    radio.exchange(
        pump,
        &[0x08, 15, 0, 18, 0, 0x03, 0x28],
        &[
            0x09, 21, 16, 0, 2, 17, 0, 0xba, 0x5c, 0xff, 0xf4, 0x04, 0xa3, 0x40, 0x71, 0xa0, 0xb5,
            0x35, 0x85, 0x3e, 0xb0, 0x83, 0x07,
        ],
    );
    radio.exchange(
        pump,
        &[0x04, 14, 0, 15, 0],
        &[0x05, 1, 14, 0, 2, 0x29, 15, 0, 1, 0x29],
    );
    radio.exchange(pump, &[0x04, 18, 0, 18, 0], &[0x05, 1, 18, 0, 1, 0x29]);
    radio.exchange(pump, &[0x0a, 15, 0], b"\x0bCharacteristic 1\0");
    radio.exchange(pump, &[0x0a, 18, 0], b"\x0bCharacteristic 2\0");
    radio.exchange(pump, &[0x0a, 17, 0], &[0x0b, 0x11, 0x22, 0x33, 0x44, 0x55]);
    radio.exchange(pump, &[0x12, 17, 0, 0xee], &[0x01, 0x12, 17, 0, 3]);
    radio.exchange(pump, &[0x0a, 13, 0], &[0x0b, 0, 1, 2, 3, 4]);
}

fn subscribe_sdk(radio: &ScriptedRadio, pump: &mut impl FnMut()) {
    for (cccd, error) in [
        (&[1][..], 0x0d),
        (&[1, 0, 0][..], 0x0d),
        (&[2, 0][..], 0x13),
    ] {
        let mut request = vec![0x12, 14, 0];
        request.extend_from_slice(cccd);
        radio.exchange(pump, &request, &[0x01, 0x12, 14, 0, error]);
    }
    radio.exchange(pump, &[0x12, 14, 0, 1, 0], &[0x13]);
    pump_until(pump, || !radio.0.borrow().replies.is_empty());
    assert_eq!(
        radio.0.borrow_mut().replies.pop_front().unwrap(),
        [0x1b, 13, 0, 1, 2, 3, 4, 5]
    );
}

fn exercise_runner(event_driven: bool) {
    let _fixture = HOST_FIXTURE.lock().unwrap();
    let radio = ScriptedRadio::default();
    let mut resources = Resources::new();
    let reports = RefCell::new(Vec::new());
    let identity = GattIdentity {
        address: RandomAddress::new([1, 2, 3, 4, 5, 0xc0]).unwrap(),
        version: Version::new(8, 0xffff, 1),
        name: "FR29-GATT",
        advertising_data: b"\x02\x01\x06\x0a\x09FR29-GATT",
    };
    let mut app = pin!(run_fixture(
        event_driven,
        radio.clone(),
        &mut resources,
        identity,
        |event| {
            reports.borrow_mut().push(event);
        }
    ));
    let mut pump = fixture_pump(app.as_mut(), &radio, event_driven);
    let peer = Peer::new([7; 6], true);
    let parameters = ConnectionParameters::new(24, 0, 72).unwrap();
    pump_until(&mut pump, || reports.borrow().len() == 1);
    check_advertisement(&radio);
    radio.event(LinkEvent::Connected(peer, parameters));
    pump_until(&mut pump, || reports.borrow().len() == 2);
    check_sdk_service(&radio, &mut pump);
    subscribe_sdk(&radio, &mut pump);
    let mut sdk_write = vec![0x12, 13, 0];
    sdk_write.extend(0..20);
    radio.exchange(&mut pump, &sdk_write, &[0x13]);
    radio.exchange(&mut pump, &[0x0a, 13, 0], &[0x0b, 0, 1, 2, 3, 4]);
    radio.exchange(&mut pump, &[0x0a, 10, 0], &[0x0b, 0, 0]);
    radio.exchange(&mut pump, &[0x0a, 3, 0], b"\x0bFR29-GATT");
    radio.exchange(&mut pump, &[0x0a, 9, 0], &[0x0b, 0, 0, 0, 0, 0, 0, 0, 0]);
    radio.exchange(&mut pump, &[0x12, 9, 0], &[0x01, 0x12, 9, 0, 0x0d]);
    assert_eq!(reports.borrow().len(), 2);
    radio.exchange(&mut pump, &[0x12, 9, 0, 1, 2, 3, 4, 5, 6, 7, 8], &[0x13]);
    // The first write did not queue an echo. Discover and enable the CCCD.
    radio.exchange(
        &mut pump,
        &[0x04, 10, 0, 10, 0],
        &[0x05, 1, 10, 0, 0x02, 0x29],
    );
    radio.exchange(&mut pump, &[0x12, 10, 0, 1, 0], &[0x13]);
    radio.exchange(&mut pump, &[0x12, 9, 0], &[0x01, 0x12, 9, 0, 0x0d]);
    radio.exchange(&mut pump, &[0x12, 9, 0, 9, 8, 7, 6, 5, 4, 3, 2], &[0x13]);
    pump_until(&mut pump, || !radio.0.borrow().replies.is_empty());
    assert_eq!(
        radio.0.borrow_mut().replies.pop_front().unwrap(),
        [0x1b, 9, 0, 9, 8, 7, 6, 5, 4, 3, 2]
    );
    radio.exchange(&mut pump, &[0x12, 14, 0, 0, 0], &[0x13]);
    radio.exchange(&mut pump, &[0x0a, 10, 0], &[0x0b, 1, 0]);
    subscribe_sdk(&radio, &mut pump);
    radio.exchange(&mut pump, &[0x12, 10, 0, 0, 0], &[0x13]);
    radio.exchange(&mut pump, &[0x12, 9, 0, 1, 1, 2, 3, 5, 8, 13, 21], &[0x13]);
    radio.exchange(&mut pump, &[0x12, 10, 0, 1, 0], &[0x13]);
    radio.event(LinkEvent::Disconnected(0x13));
    pump_until(&mut pump, || reports.borrow().len() == 7);
    radio.event(LinkEvent::Connected(peer, parameters));
    pump_until(&mut pump, || reports.borrow().len() == 8);
    radio.exchange(&mut pump, &[0x0a, 14, 0], &[0x0b, 0, 0]);
    subscribe_sdk(&radio, &mut pump);
    radio.exchange(&mut pump, &[0x12, 14, 0, 0, 0], &[0x13]);
    radio.exchange(&mut pump, &[0x0a, 10, 0], &[0x0b, 0, 0]);
    radio.exchange(&mut pump, &[0x0a, 9, 0], &[0x0b, 1, 1, 2, 3, 5, 8, 13, 21]);
    radio.exchange(&mut pump, &[0x12, 9, 0, 0, 1, 2, 3, 4, 5, 6, 7], &[0x13]);
    for _ in 0..16 {
        pump();
    }
    assert!(radio.0.borrow().replies.is_empty());
    assert_eq!(
        reports.borrow().as_slice(),
        &[
            GattReport::Advertising,
            GattReport::Connected,
            expected_commit(&[1, 2, 3, 4, 5, 6, 7, 8]),
            expected_commit(&[9, 8, 7, 6, 5, 4, 3, 2]),
            expected_commit(&[1, 1, 2, 3, 5, 8, 13, 21]),
            GattReport::Disconnected,
            GattReport::Advertising,
            GattReport::Connected,
            expected_commit(&[0, 1, 2, 3, 4, 5, 6, 7]),
        ]
    );
    // Fill one complete ATT MTU, then shrink the value to prove that no old tail is returned.
    radio.exchange(&mut pump, &[0x12, 10, 0, 1, 0], &[0x13]);
    for value in [&b"12345678abcdefghij90"[..], &[0x81][..]] {
        let mut request = vec![0x12, 9, 0];
        request.extend_from_slice(value);
        radio.exchange(&mut pump, &request, &[0x13]);
        pump_until(&mut pump, || !radio.0.borrow().replies.is_empty());
        let mut notify = vec![0x1b, 9, 0];
        notify.extend_from_slice(value);
        assert_eq!(radio.0.borrow_mut().replies.pop_front().unwrap(), notify);
        assert_eq!(reports.borrow().last(), Some(&expected_commit(value)));
        let mut read = vec![0x0b];
        read.extend_from_slice(value);
        radio.exchange(&mut pump, &[0x0a, 9, 0], &read);
    }
}

#[cfg(feature = "ble-gatt-event-driven")]
#[test]
fn event_driven_runner_drops_invalid_responses_and_reconnects() {
    use fr8029d_bringup::{
        gatt::VALUE_LEN,
        gatt_runner::{GattApplication, run_event_driven_with_application},
    };

    thread_local! {
        static BOOTED: Cell<bool> = const { Cell::new(false) };
        static POLLED: Cell<bool> = const { Cell::new(false) };
        static DISCONNECTED: Cell<bool> = const { Cell::new(false) };
    }
    struct TestApplication;
    impl GattApplication for TestApplication {
        type Response = [u8; VALUE_LEN + 1];

        fn on_boot(&self) {
            BOOTED.set(true);
        }

        fn poll(&self) {
            POLLED.set(true);
        }

        fn on_disconnect(&self) {
            DISCONNECTED.set(true);
        }

        fn response<'a>(&self, value: &'a [u8], response: &'a mut Self::Response) -> &'a [u8] {
            assert!(BOOTED.get() && POLLED.get());
            response.fill(0x5a);
            match value {
                [0xad, 1, 0, 0] => &response[..VALUE_LEN],
                [0xad, 2, 0, 0] => response,
                [0xad, 3, 0, 0] => &[],
                _ => panic!("unexpected application write"),
            }
        }
    }

    let _fixture = HOST_FIXTURE.lock().unwrap();
    let radio = ScriptedRadio::default();
    let mut resources = Resources::new();
    let identity = GattIdentity {
        address: RandomAddress::new([1, 2, 3, 4, 5, 0xc0]).unwrap(),
        version: Version::new(8, 0xffff, 1),
        name: "FR29-GATT",
        advertising_data: b"\x02\x01\x06\x0a\x09FR29-GATT",
    };
    let mut app = pin!(run_event_driven_with_application(
        radio.clone(),
        &mut resources,
        identity,
        |_| {},
        None,
        TestApplication,
    ));
    let mut pump = fixture_pump(app.as_mut(), &radio, true);
    pump_until(&mut pump, || radio.0.borrow().advertising);
    assert!(BOOTED.get());
    for _ in 0..2 {
        DISCONNECTED.set(false);
        radio.event(LinkEvent::Connected(
            Peer::new([7; 6], true),
            ConnectionParameters::new(24, 0, 72).unwrap(),
        ));
        radio.exchange(&mut pump, &[0x12, 10, 0, 1, 0], &[0x13]);
        for op in [2, 3] {
            radio.exchange(&mut pump, &[0x12, 9, 0, 0xad, op, 0, 0], &[0x13]);
            // The accepted write remains readable and there is no invalid notification.
            radio.exchange(&mut pump, &[0x0a, 9, 0], &[0x0b, 0xad, op, 0, 0]);
        }
        radio.exchange(&mut pump, &[0x12, 9, 0, 0xad, 1, 0, 0], &[0x13]);
        pump_until(&mut pump, || !radio.0.borrow().replies.is_empty());
        let mut notification = vec![0x1b, 9, 0];
        notification.extend([0x5a; VALUE_LEN]);
        assert_eq!(
            radio.0.borrow_mut().replies.pop_front().unwrap(),
            notification
        );
        assert!(!DISCONNECTED.get());
        radio.event(LinkEvent::Disconnected(0x13));
        pump_until(&mut pump, || {
            DISCONNECTED.get() && radio.0.borrow().advertising
        });
    }
}

#[test]
fn test_should_reject_an_oversized_advertisement_without_starting_radio() {
    let radio = ScriptedRadio::default();
    let mut resources = Resources::new();
    let identity = GattIdentity {
        address: RandomAddress::new([1, 2, 3, 4, 5, 0xc0]).unwrap(),
        version: Version::new(8, 0xffff, 1),
        name: "FR29-GATT",
        // Valid leading flags so the Host would otherwise silently truncate.
        advertising_data: b"\x02\x01\x06\x1c\x09abcdefghijklmnopqrstuvwxyz!",
    };
    let mut app = pin!(run(radio.clone(), &mut resources, identity, |_| {
        panic!("invalid identity must not report an active application")
    }));
    let mut context = Context::from_waker(Waker::noop());
    assert!(matches!(
        app.as_mut().poll(&mut context),
        std::task::Poll::Ready(GattRunError::Host(_))
    ));
    assert!(radio.0.borrow().advertisement.is_none());
}

fn check_advertisement(radio: &ScriptedRadio) {
    let advertisement = radio.0.borrow().advertisement.unwrap();
    assert_eq!(advertisement.address.bytes(), [1, 2, 3, 4, 5, 0xc0]);
    let expected_interval = if cfg!(all(
        feature = "ble-gatt-mac-probe",
        not(feature = "ble-gatt-xip")
    )) {
        48
    } else {
        256
    };
    assert_eq!(advertisement.parameters.min_interval(), expected_interval);
    assert_eq!(advertisement.parameters.max_interval(), expected_interval);
    assert_eq!(
        advertisement.data.as_slice(),
        b"\x02\x01\x06\x0a\x09FR29-GATT"
    );
}

#[cfg(all(feature = "ota-v1", feature = "ble-gatt-event-driven"))]
#[path = "common/ota_medium.rs"]
mod ota_medium;

#[cfg(all(feature = "ota-v1", feature = "ble-gatt-event-driven"))]
#[test]
fn ota_characteristic_rejects_oversize_and_publishes_only_after_finish() {
    use fr8029d_bringup::{
        gatt_runner::run_event_driven_ota,
        ota::{MAGIC, MARKER, Receiver},
    };
    use ota_medium::{Nor, begin, crc32, image};

    let _fixture = HOST_FIXTURE.lock().unwrap();
    let radio = ScriptedRadio::default();
    let mut resources = Resources::new();
    let connected = Cell::new(false);
    let data = image(0x3a000);
    let mut nor = Nor::new();
    {
        let mut rx = Receiver::new(&mut nor, fr8029d_bringup::ota::manifest(&data).unwrap());
        let mut receive = |frame: Option<&[u8]>| {
            if let Some(frame) = frame {
                rx.receive(frame).is_ok()
            } else {
                rx.abort();
                true
            }
        };
        let identity = GattIdentity {
            address: RandomAddress::new([1, 2, 3, 4, 5, 0xc0]).unwrap(),
            version: Version::new(8, 0xffff, 1),
            name: "FR29-OTA",
            advertising_data: &[2, 1, 6],
        };
        let mut app = pin!(run_event_driven_ota(
            radio.clone(),
            &mut resources,
            identity,
            |event| connected.set(event == GattReport::Connected),
            &mut receive
        ));
        let mut pump = fixture_pump(app.as_mut(), &radio, false);
        pump_until(&mut pump, || radio.0.borrow().advertising);
        radio.event(LinkEvent::Connected(
            Peer::new([7; 6], true),
            ConnectionParameters::new(24, 0, 72).unwrap(),
        ));
        pump_until(&mut pump, || connected.get());
        let uuid = 0x2f8029e0_7ff7_48b0_a64e_d6ebc735b251_u128.to_le_bytes();
        radio.exchange(
            &mut pump,
            &[0x10, 19, 0, 0xff, 0xff, 0, 0x28],
            &[&[0x11, 20, 19, 0, 0xff, 0xff][..], &uuid].concat(),
        );
        let write = [0x12, 21, 0];
        radio.exchange(
            &mut pump,
            &[&write[..], &begin(0x3a001, 0)].concat(),
            &[1, 0x12, 21, 0, 0x13],
        );
        radio.exchange(
            &mut pump,
            &[&write[..], &begin(0x3a000, crc32(&data))].concat(),
            &[0x13],
        );
        for (i, chunk) in data.chunks(15).enumerate() {
            radio.exchange(
                &mut pump,
                &[
                    &write[..],
                    &[2],
                    &u32::try_from(i * 15).unwrap().to_le_bytes(),
                    chunk,
                ]
                .concat(),
                &[0x13],
            );
        }
        radio.exchange(&mut pump, &[0x12, 21, 0, 3], &[0x13]);
    }
    assert_eq!(nor.word(MARKER), MAGIC);
    nor.check_neighbors();
}
