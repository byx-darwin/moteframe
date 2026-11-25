//! Concrete pinned upstream GATT integration, entirely scripted in host tests.
#![allow(clippy::unwrap_used, clippy::expect_used)]
#[path = "gatt_fixture/mod.rs"]
mod gatt_support;
use ble_controller::{Controller, HciController, RandomAddress, Version};
use ble_host_trouble::{Resources, build, host::prelude::*};
use device_connection::{FirstFragmentQueue, FirstSlotOutcome};
use device_runtime::Transport;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use gatt_support::{Backend, inject};
use std::{
    future::Future,
    pin::pin,
    task::{Context, Poll, Waker},
};
use wattch5_candidate::gatt::{
    Bridge, ControlProfile, ControlUuids, TroubleTransport, drive_connection,
};

fn uuids() -> ControlUuids {
    ControlUuids {
        service: 0xfff0_u16.into(),
        rx: 0xfff1_u16.into(),
        tx: 0xfff2_u16.into(),
        data: 0xfff3_u16.into(),
    }
}
fn pump(transport: &mut impl Transport) {
    for now in 0..32 {
        transport.poll(now, 2).unwrap();
    }
}
macro_rules! connected {
    ($host:ident, $peripheral:ident, $backend:ident, $server:ident) => {{
        let mut accepting = pin!(async {
            $peripheral
                .advertise(
                    &AdvertisementParameters::default(),
                    Advertisement::ConnectableScannableUndirected {
                        adv_data: &[2, 1, 6],
                        scan_data: &[],
                    },
                )
                .await
                .unwrap()
                .accept()
                .await
                .unwrap()
                .with_attribute_server(&$server)
                .unwrap()
        });
        let mut injected = false;
        let mut found = None;
        for _ in 0..128 {
            let mut cx = Context::from_waker(Waker::noop());
            assert!($host.as_mut().poll(&mut cx).is_pending());
            if let Poll::Ready(conn) = accepting.as_mut().poll(&mut cx) {
                found = Some(conn);
                break;
            }
            if $backend.0.borrow().advertised && !injected {
                $backend.connect();
                injected = true;
            }
        }
        found.expect("actual Host must accept scripted connection")
    }};
}
fn write(backend: &Backend, handle: u16, bytes: &[u8]) {
    let mut pdu = vec![0x12];
    pdu.extend_from_slice(&handle.to_le_bytes());
    pdu.extend_from_slice(bytes);
    inject(backend, &pdu);
}

#[test]
fn test_should_notify_actual_twenty_byte_data_characteristic_and_scope_both_subscriptions() {
    use device_connection::EventPlane;
    use device_runtime::EventTransport;
    let _serial = gatt_support::serialized();
    let backend = Backend::default();
    let controller =
        HciController::new(Controller::new(backend.clone(), Version::new(6, 0xffff, 1)));
    let mut resources = Resources::new();
    let stack = build(
        controller,
        &mut resources,
        RandomAddress::new([1, 2, 3, 4, 5, 0xc0]).unwrap(),
    );
    let mut table: AttributeTable<'_, NoopRawMutex, 9> = AttributeTable::new();
    let mut rx = [0; 20];
    let mut tx = [0; 20];
    let mut data = [0; 20];
    let profile = ControlProfile::add(&mut table, uuids(), &mut rx, &mut tx, &mut data).unwrap();
    assert_eq!(table.len(), 9);
    assert_eq!(table.uuid(profile.data().handle), Some(uuids().data));
    let server = AttributeServer::<NoopRawMutex, DefaultPacketPool, 9, 1>::new(table);
    let mut peripheral = stack.peripheral();
    let mut runner = stack.runner();
    let mut host = pin!(runner.run());
    let conn = connected!(host, peripheral, backend, server);
    let bridge = Bridge::<1, 1>::new().unwrap();
    let mut driver = pin!(drive_connection(&conn, &profile, &bridge));
    let mut transport = TroubleTransport::new(&bridge, host.as_mut(), driver.as_mut());
    pump(&mut transport);
    write(&backend, profile.tx().cccd_handle.unwrap(), &[1, 0]);
    pump(&mut transport);
    assert_eq!(transport.scope().unwrap(), None);
    write(&backend, profile.data().cccd_handle.unwrap(), &[1, 0]);
    pump(&mut transport);
    let scope = transport.scope().unwrap().unwrap();
    assert_eq!(scope.frame_max(), 20);
    {
        let mut queue = transport.event_queue(scope, EventPlane::Data).unwrap();
        assert!(matches!(
            queue
                .with_reserved_slot(|out| {
                    assert_eq!(out.len(), 20);
                    out.fill(0x5a);
                    Ok::<_, ()>((20, ()))
                })
                .unwrap(),
            FirstSlotOutcome::Admitted(())
        ));
    }
    pump(&mut transport);
    let handle = profile.data().handle.to_le_bytes();
    let mut expected = vec![0x1b, handle[0], handle[1]];
    expected.extend_from_slice(&[0x5a; 20]);
    assert!(backend.att(0x1b).contains(&expected));
    write(&backend, profile.data().cccd_handle.unwrap(), &[0, 0]);
    pump(&mut transport);
    assert_eq!(transport.scope().unwrap(), None);
    assert!(transport.event_queue(scope, EventPlane::Data).is_err());
    write(&backend, profile.data().cccd_handle.unwrap(), &[1, 0]);
    pump(&mut transport);
    assert!(transport.scope().unwrap().unwrap().generation() > scope.generation());
    backend.disconnect();
    pump(&mut transport);
    assert_eq!(transport.scope().unwrap(), None);
}
