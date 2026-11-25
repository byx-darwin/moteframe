//! Concrete pinned upstream GATT integration, entirely scripted in host tests.
#![cfg(feature = "runtime-trouble")]
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod gatt_support;
use ble_controller::{Controller, HciController, RandomAddress, Version};
use ble_host_trouble::{Resources, build, host::prelude::*};
use device_connection::{FirstFragmentQueue, FirstSlotOutcome};
use device_runtime::Transport;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use gatt_support::{Backend, count, inject};
use std::{
    cell::Cell,
    future::Future,
    pin::pin,
    task::{Context, Poll, Waker},
};
use wattch5_product::runtime::gatt::{
    Bridge, ControlProfile, ControlUuids, TroubleTransport, drive_connection,
};

fn uuids() -> ControlUuids {
    ControlUuids {
        service: 0xfff0_u16.into(),
        rx: 0xfff1_u16.into(),
        tx: 0xfff2_u16.into(),
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
fn test_should_reject_zero_fifo_capacity() {
    assert!(Bridge::<0, 1>::new().is_err());
    assert!(Bridge::<1, 0>::new().is_err());
}
#[test]
fn test_should_build_real_variable_value_profile_and_check_table_capacity() {
    let mut rx = [0; 20];
    let mut tx = [0; 20];
    let mut table: AttributeTable<'_, NoopRawMutex, 6> = AttributeTable::new();
    let profile = ControlProfile::add(&mut table, uuids(), &mut rx, &mut tx).unwrap();
    assert_eq!(table.len(), 6);
    assert_eq!(table.uuid(profile.rx().handle), Some(uuids().rx));
    table.set(profile.tx(), &[1, 2, 3][..]).unwrap();
    let mut read = [0; 20];
    assert_eq!(table.read(profile.tx().handle, 0, &mut read).unwrap(), 3);
    assert_eq!(&read[..3], &[1, 2, 3]);
    let mut rx = [0; 20];
    let mut tx = [0; 20];
    let mut tiny: AttributeTable<'_, NoopRawMutex, 5> = AttributeTable::new();
    assert!(ControlProfile::add(&mut tiny, uuids(), &mut rx, &mut tx).is_err());
    assert!(tiny.is_empty());
}
fn cycle_subscriptions(
    backend: &Backend,
    cccd: u16,
    first: device_runtime::Scope,
    transport: &mut impl Transport,
) {
    let mut previous = first;
    for _ in 0..5 {
        write(backend, cccd, &[0, 0]);
        pump(transport);
        assert_eq!(transport.scope().unwrap(), None);
        write(backend, cccd, &[1, 0]);
        pump(transport);
        let next = transport.scope().unwrap().unwrap();
        assert!(next.generation() > previous.generation());
        transport.close_scope(previous).unwrap();
        assert_eq!(
            transport.scope().unwrap(),
            Some(next),
            "old close cannot mute new scope"
        );
        previous = next;
    }
}
#[test]
fn test_should_use_actual_cccd_scoped_queue_notifications_rx_rejection_and_disconnect() {
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
    let mut table: AttributeTable<'_, NoopRawMutex, 6> = AttributeTable::new();
    let mut rx = [0; 20];
    let mut tx = [0; 20];
    let profile = ControlProfile::add(&mut table, uuids(), &mut rx, &mut tx).unwrap();
    let server = AttributeServer::<NoopRawMutex, DefaultPacketPool, 6, 1>::new(table);
    let mut peripheral = stack.peripheral();
    let mut runner = stack.runner();
    let mut host = pin!(runner.run());
    let conn = connected!(host, peripheral, backend, server);
    let bridge = Bridge::<1, 1>::new().unwrap();
    let mut driver = pin!(drive_connection(&conn, &profile, &bridge));
    let mut transport = TroubleTransport::new(&bridge, host.as_mut(), driver.as_mut());
    pump(&mut transport);
    assert_eq!(transport.scope().unwrap(), None);
    let cccd = profile.tx().cccd_handle.unwrap();
    write(&backend, cccd, &[1, 0]);
    pump(&mut transport);
    let first = transport.scope().unwrap().unwrap();
    assert_eq!(first.frame_max(), 20);
    write(&backend, cccd, &[1, 0]);
    pump(&mut transport);
    assert_eq!(transport.scope().unwrap(), Some(first));
    let filled = Cell::new(0);
    {
        let mut queue = transport.queue(first).unwrap();
        assert!(matches!(
            queue
                .with_reserved_slot(|out| {
                    out[..3].copy_from_slice(&[1, 2, 3]);
                    Ok::<_, ()>((3, 9))
                })
                .unwrap(),
            FirstSlotOutcome::Admitted(9)
        ));
        assert!(matches!(
            queue
                .with_reserved_slot(|_| {
                    filled.set(1);
                    Ok::<_, ()>((1, ()))
                })
                .unwrap(),
            FirstSlotOutcome::NoSlot
        ));
    }
    assert_eq!(filled.get(), 0);
    pump(&mut transport);
    let h = profile.tx().handle.to_le_bytes();
    assert!(backend.att(0x1b).contains(&vec![0x1b, h[0], h[1], 1, 2, 3]));
    write(
        &backend,
        profile.rx().handle,
        &[0x10, 0, 1, 0, 1, 0, 0, 0, 0, 0],
    );
    pump(&mut transport);
    write(&backend, profile.rx().handle, &[9]);
    pump(&mut transport);
    let errors = backend.att(1);
    let rh = profile.rx().handle.to_le_bytes();
    assert!(errors.contains(&vec![1, 0x12, rh[0], rh[1], 0x11]));
    assert_eq!(transport.receive().unwrap().unwrap().scope(), first);
    assert!(transport.receive().unwrap().is_none());
    transport.close_scope(first).unwrap();
    write(&backend, cccd, &[1, 0]);
    pump(&mut transport);
    assert_eq!(
        transport.scope().unwrap(),
        None,
        "repeated enable cannot reopen muted subscription"
    );
    cycle_subscriptions(&backend, cccd, first, &mut transport);
    backend.disconnect();
    pump(&mut transport);
    assert_eq!(transport.scope().unwrap(), None);
    assert!(transport.receive().unwrap().is_none());
    assert!(count(&backend, 0x13) >= 14);
}

fn request(backend: &Backend, handle: u16, module: u8, opcode: u8, id: u16, payload: &[u8]) {
    let mut frame = vec![0x10, module, opcode, 0];
    frame.extend_from_slice(&id.to_le_bytes());
    frame.extend_from_slice(&[0, 0]);
    frame.extend_from_slice(&u16::try_from(payload.len()).unwrap().to_le_bytes());
    frame.extend_from_slice(payload);
    write(backend, handle, &frame);
}
fn response(backend: &Backend, id: u16, start: usize) -> Vec<u8> {
    let mut peer = device_protocol::Reassembler::<128>::new(20, 128).unwrap();
    for notify in backend.att(0x1b).into_iter().skip(start) {
        let frame = device_protocol::Frame::decode(&notify[3..], 20, 128).unwrap();
        if frame.header.message_id == id {
            peer.push(frame, 0).unwrap();
        }
    }
    peer.message()
        .expect("complete WAP response from real ATT notifications")
        .payload
        .to_vec()
}
#[derive(Debug, Default)]
struct Slice(u32);
impl device_runtime::Work for Slice {
    fn step(&mut self, _: u64) -> Result<(), device_runtime::PortError> {
        self.0 += 1;
        Ok(())
    }
}
fn exercise_product_cycles(backend: &Backend, profile: &ControlProfile, step: &mut impl FnMut()) {
    for cycle in 0..5_u16 {
        write(backend, profile.tx().cccd_handle.unwrap(), &[1, 0]);
        step();
        let id = 1;
        let hello_start = backend.att(0x1b).len();
        request(
            backend,
            profile.rx().handle,
            0,
            1,
            id,
            &[1, 1, 128, 0, 0, 0, 3, 0],
        );
        step();
        let hello = response(backend, id, hello_start);
        assert_eq!(&hello[..4], &[0, 0, 0, 0]);
        assert_eq!(
            device_protocol::HelloResponse::decode(&hello[4..])
                .unwrap()
                .session,
            u32::from(cycle) + 1
        );
        if cycle == 0 {
            let set_start = backend.att(0x1b).len();
            request(
                backend,
                profile.rx().handle,
                4,
                0x11,
                id + 1,
                &[3, 0, 0, 0, 0, 0, 1, 0, 1],
            );
            step();
            assert_eq!(&response(backend, id + 1, set_start)[..4], &[0, 0, 0, 0]);
        }
        let get_start = backend.att(0x1b).len();
        let get_id = if cycle == 0 { 3 } else { 2 };
        request(backend, profile.rx().handle, 4, 0x10, get_id, &[3, 0]);
        step();
        assert_eq!(response(backend, get_id, get_start).last(), Some(&1));
        write(backend, profile.tx().cccd_handle.unwrap(), &[0, 0]);
        step();
    }
}
#[test]
fn test_should_run_real_product_hello_settings_set_get_over_actual_host_gatt() {
    use gatt_support::product::{Access, TimeScope, config, initialized, time_owner};
    use wattch5_product::runtime::{BleAuthority, NoExtensions, ProductFactory};
    let _serial = gatt_support::serialized();
    let memory = gatt_support::medium::Memory::new();
    let owner = initialized(memory.clone());
    let time = time_owner();
    let authority = Access(true);
    let factory = ProductFactory::new(
        &owner,
        &time,
        BleAuthority {
            properties: &authority,
            time: &TimeScope,
        },
        config(),
        &NoExtensions,
    )
    .unwrap();
    let mut runtime = factory.runtime(device_runtime::Budgets::new(2, 2, 2).unwrap());
    let backend = Backend::default();
    let controller =
        HciController::new(Controller::new(backend.clone(), Version::new(6, 0xffff, 1)));
    let mut resources = Resources::new();
    let stack = build(
        controller,
        &mut resources,
        RandomAddress::new([1, 2, 3, 4, 5, 0xc0]).unwrap(),
    );
    let mut rx = [0; 20];
    let mut tx = [0; 20];
    let mut table: AttributeTable<'_, NoopRawMutex, 6> = AttributeTable::new();
    let profile = ControlProfile::add(&mut table, uuids(), &mut rx, &mut tx).unwrap();
    let server = AttributeServer::<NoopRawMutex, DefaultPacketPool, 6, 1>::new(table);
    let mut peripheral = stack.peripheral();
    let mut runner = stack.runner();
    let mut host = pin!(runner.run());
    let conn = connected!(host, peripheral, backend, server);
    let bridge = Bridge::<2, 1>::new().unwrap();
    let mut driver = pin!(drive_connection(&conn, &profile, &bridge));
    let mut transport = TroubleTransport::new(&bridge, host.as_mut(), driver.as_mut());
    let mut ui = Slice::default();
    let mut background = Slice::default();
    let mut now = 0;
    {
        let mut step = || {
            for _ in 0..64 {
                let report = runtime.step(now, &mut transport, &mut ui, &mut background);
                now += 1;
                assert!(report.connection.is_ok(), "{report:?}");
            }
        };
        exercise_product_cycles(&backend, &profile, &mut step);
    }
    runtime.shutdown(&mut transport).unwrap();
    assert!(ui.0 >= 256 && background.0 >= 256);
    drop(runtime);
    drop(owner);
    let reopened = gatt_support::product::open(memory);
    assert_eq!(reopened.revision(), Ok(2));
    let mut value = [0; 1];
    let read = device_services::PropertyBackend::read(&reopened.adapter(4).unwrap(), 3, &mut value)
        .unwrap();
    assert_eq!(read.length, 1);
    assert_eq!(value, [1]);
    assert!(backend.att(0x1b).iter().all(|pdu| pdu.len() <= 23));
    assert!(
        backend.att(0x1b).iter().any(|pdu| pdu.len() < 23),
        "short final WAP fragment must stay short"
    );
}

#[test]
fn test_should_disconnect_instead_of_silently_losing_a_full_rx_write_command() {
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
    let mut rx = [0; 20];
    let mut tx = [0; 20];
    let mut table: AttributeTable<'_, NoopRawMutex, 6> = AttributeTable::new();
    let profile = ControlProfile::add(&mut table, uuids(), &mut rx, &mut tx).unwrap();
    let server = AttributeServer::<NoopRawMutex, DefaultPacketPool, 6, 1>::new(table);
    let mut peripheral = stack.peripheral();
    let mut runner = stack.runner();
    let mut host = pin!(runner.run());
    let conn = connected!(host, peripheral, backend, server);
    let bridge = Bridge::<1, 1>::new().unwrap();
    let mut driver = pin!(drive_connection(&conn, &profile, &bridge));
    let mut transport = TroubleTransport::new(&bridge, host.as_mut(), driver.as_mut());
    write(&backend, profile.tx().cccd_handle.unwrap(), &[1, 0]);
    pump(&mut transport);
    write(&backend, profile.rx().handle, &[1]);
    pump(&mut transport);
    let h = profile.rx().handle.to_le_bytes();
    inject(&backend, &[0x52, h[0], h[1], 2]);
    let mut failed = false;
    for now in 0..64 {
        failed |= transport.poll(now, 2).is_err();
    }
    assert!(failed, "write command overflow cannot claim success");
    assert_eq!(transport.scope().unwrap(), None);
    assert!(transport.receive().unwrap().is_none());
}
#[test]
fn test_should_propagate_actual_host_failure_and_never_repoll_completed_futures() {
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
    let mut rx = [0; 20];
    let mut tx = [0; 20];
    let mut table: AttributeTable<'_, NoopRawMutex, 6> = AttributeTable::new();
    let profile = ControlProfile::add(&mut table, uuids(), &mut rx, &mut tx).unwrap();
    let server = AttributeServer::<NoopRawMutex, DefaultPacketPool, 6, 1>::new(table);
    let mut peripheral = stack.peripheral();
    let mut runner = stack.runner();
    let mut host = pin!(runner.run());
    let conn = connected!(host, peripheral, backend, server);
    let bridge = Bridge::<1, 1>::new().unwrap();
    let mut driver = pin!(drive_connection(&conn, &profile, &bridge));
    let mut transport = TroubleTransport::new(&bridge, host.as_mut(), driver.as_mut());
    write(&backend, profile.tx().cccd_handle.unwrap(), &[1, 0]);
    pump(&mut transport);
    let scope = transport.scope().unwrap().unwrap();
    {
        let mut queue = transport.queue(scope).unwrap();
        queue
            .with_reserved_slot(|out| {
                out[0] = 7;
                Ok::<_, ()>((1, ()))
            })
            .unwrap();
    }
    backend
        .0
        .borrow_mut()
        .events
        .push_back(ble_controller::LinkEvent::Received(
            ble_controller::LinkFrame::new(ble_controller::FrameKind::Control, &[0]).unwrap(),
        ));
    let mut failed = false;
    for now in 0..32 {
        failed |= transport.poll(now, 2).is_err();
    }
    assert!(failed);
    assert_eq!(transport.scope().unwrap(), None);
    assert!(transport.receive().unwrap().is_none());
}

#[test]
fn test_should_mute_and_disconnect_on_actual_upstream_notification_allocation_failure() {
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
    let mut rx = [0; 20];
    let mut tx = [0; 20];
    let mut table: AttributeTable<'_, NoopRawMutex, 6> = AttributeTable::new();
    let profile = ControlProfile::add(&mut table, uuids(), &mut rx, &mut tx).unwrap();
    let server = AttributeServer::<NoopRawMutex, DefaultPacketPool, 6, 1>::new(table);
    let mut peripheral = stack.peripheral();
    let mut runner = stack.runner();
    let mut host = pin!(runner.run());
    let conn = connected!(host, peripheral, backend, server);
    let bridge = Bridge::<1, 1>::new().unwrap();
    let mut driver = pin!(drive_connection(&conn, &profile, &bridge));
    let mut transport = TroubleTransport::new(&bridge, host.as_mut(), driver.as_mut());
    write(&backend, profile.tx().cccd_handle.unwrap(), &[1, 0]);
    pump(&mut transport);
    let scope = transport.scope().unwrap().unwrap();
    {
        let mut queue = transport.queue(scope).unwrap();
        queue
            .with_reserved_slot(|out| {
                out[0] = 9;
                Ok::<_, ()>((1, ()))
            })
            .unwrap();
    }
    // The actual upstream pool now cannot assemble the notification PDU.
    let mut packets = Vec::new();
    while let Some(packet) = DefaultPacketPool::allocate() {
        packets.push(packet);
    }
    let mut uncertain = false;
    for now in 0..8 {
        uncertain |= transport.poll(now, 2) == Err(device_runtime::PortError::Uncertain);
    }
    assert!(uncertain);
    assert_eq!(transport.scope().unwrap(), None);
    assert!(
        !conn.raw().is_connected(),
        "error requests actual upstream disconnect"
    );
    drop(packets);
    // A new driver on the same failed connection must not manufacture a generation.
    let mut replacement = pin!(drive_connection(&conn, &profile, &bridge));
    let mut cx = Context::from_waker(Waker::noop());
    assert_eq!(
        replacement.as_mut().poll(&mut cx),
        Poll::Ready(Err(device_runtime::PortError::InvalidInput))
    );
}

#[test]
fn test_should_retain_pending_actual_notify_future_and_cancel_it_on_subscription_revocation() {
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
    let mut rx = [0; 20];
    let mut tx = [0; 20];
    let mut table: AttributeTable<'_, NoopRawMutex, 6> = AttributeTable::new();
    let profile = ControlProfile::add(&mut table, uuids(), &mut rx, &mut tx).unwrap();
    let server = AttributeServer::<NoopRawMutex, DefaultPacketPool, 6, 1>::new(table);
    let mut peripheral = stack.peripheral();
    let mut runner = stack.runner();
    let mut host = pin!(runner.run());
    let conn = connected!(host, peripheral, backend, server);
    let bridge = Bridge::<1, 1>::new().unwrap();
    let mut driver = pin!(drive_connection(&conn, &profile, &bridge));
    let scope = {
        let mut transport = TroubleTransport::new(&bridge, host.as_mut(), driver.as_mut());
        write(&backend, profile.tx().cccd_handle.unwrap(), &[1, 0]);
        pump(&mut transport);
        transport.scope().unwrap().unwrap()
    };
    {
        // Pause only Host TX in this test so the actual upstream outbound queue
        // fills. The concrete production GATT driver and notify future stay real.
        let mut paused = pin!(std::future::pending::<Result<(), ()>>());
        let mut transport = TroubleTransport::new(&bridge, paused.as_mut(), driver.as_mut());
        for value in 1..=3 {
            let mut queue = transport.queue(scope).unwrap();
            assert!(matches!(
                queue
                    .with_reserved_slot(|out| {
                        out[0] = value;
                        Ok::<_, ()>((1, ()))
                    })
                    .unwrap(),
                FirstSlotOutcome::Admitted(())
            ));
            drop(queue);
            pump(&mut transport);
        }
        assert_eq!(count(&backend, 0x1b), 0);
        // This test directly revokes the actual server's per-connection CCCD.
        // Wire CCCD event handling is exercised separately above. Polling the
        // pending future must observe revocation before calling upstream again.
        let mut values = server.get_client_att_table(conn.raw()).unwrap();
        values
            .write(profile.tx().cccd_handle.unwrap(), 0, &[0, 0])
            .unwrap();
        server.set_client_att_table(conn.raw(), &values.view());
        pump(&mut transport);
        assert_eq!(transport.scope().unwrap(), None);
        values
            .write(profile.tx().cccd_handle.unwrap(), 0, &[1, 0])
            .unwrap();
        server.set_client_att_table(conn.raw(), &values.view());
        pump(&mut transport);
        assert_eq!(
            transport.scope().unwrap(),
            None,
            "actual state alone cannot manufacture a fresh enable event"
        );
    }
    let mut transport = TroubleTransport::new(&bridge, host.as_mut(), driver.as_mut());
    pump(&mut transport);
    let notifications = backend.att(0x1b);
    assert_eq!(
        notifications.len(),
        2,
        "only the two already admitted upstream PDUs can remain"
    );
    assert_eq!(notifications[0].last(), Some(&1));
    assert_eq!(notifications[1].last(), Some(&2));
    assert_eq!(transport.scope().unwrap(), None);
}

#[test]
fn test_should_preserve_three_owned_requests_and_full_invalid_precedence_over_actual_host() {
    use gatt_support::product::{Access, TimeScope, config, initialized, time_owner};
    use wattch5_product::runtime::{BleAuthority, NoExtensions, ProductFactory};
    let _serial = gatt_support::serialized();
    let memory = gatt_support::medium::Memory::new();
    let owner = initialized(memory.clone());
    let time = time_owner();
    let authority = Access(true);
    let factory = ProductFactory::new(
        &owner,
        &time,
        BleAuthority {
            properties: &authority,
            time: &TimeScope,
        },
        config(),
        &NoExtensions,
    )
    .unwrap();
    let mut runtime = factory.runtime(device_runtime::Budgets::new(2, 2, 2).unwrap());
    let backend = Backend::default();
    let controller =
        HciController::new(Controller::new(backend.clone(), Version::new(6, 0xffff, 1)));
    let mut resources = Resources::new();
    let stack = build(
        controller,
        &mut resources,
        RandomAddress::new([1, 2, 3, 4, 5, 0xc0]).unwrap(),
    );
    let mut rx = [0; 20];
    let mut tx = [0; 20];
    let mut table: AttributeTable<'_, NoopRawMutex, 6> = AttributeTable::new();
    let profile = ControlProfile::add(&mut table, uuids(), &mut rx, &mut tx).unwrap();
    let server = AttributeServer::<NoopRawMutex, DefaultPacketPool, 6, 1>::new(table);
    let mut peripheral = stack.peripheral();
    let mut runner = stack.runner();
    let mut host = pin!(runner.run());
    let conn = connected!(host, peripheral, backend, server);
    let bridge = Bridge::<3, 1>::new().unwrap();
    let mut driver = pin!(drive_connection(&conn, &profile, &bridge));
    let mut transport = TroubleTransport::new(&bridge, host.as_mut(), driver.as_mut());
    write(&backend, profile.tx().cccd_handle.unwrap(), &[1, 0]);
    pump(&mut transport);
    let scope = transport.scope().unwrap().unwrap();
    request(
        &backend,
        profile.rx().handle,
        0,
        1,
        1,
        &[1, 1, 128, 0, 0, 0, 3, 0],
    );
    pump(&mut transport);
    request(&backend, profile.rx().handle, 4, 0x10, 2, &[3, 0]);
    pump(&mut transport);
    request(&backend, profile.rx().handle, 4, 0x10, 3, &[3, 0]);
    pump(&mut transport);
    request(&backend, profile.rx().handle, 4, 0x10, 4, &[3, 0]);
    pump(&mut transport);
    write(&backend, profile.rx().handle, &[]);
    pump(&mut transport);
    let h = profile.rx().handle.to_le_bytes();
    let errors = backend.att(1);
    assert_eq!(
        errors,
        vec![
            vec![1, 0x12, h[0], h[1], 0x11],
            vec![1, 0x12, h[0], h[1], 0x0d]
        ]
    );
    let mut ui = Slice::default();
    let mut background = Slice::default();
    for now in 0..128 {
        let report = runtime.step(now, &mut transport, &mut ui, &mut background);
        assert!(report.connection.is_ok(), "{report:?}");
    }
    let mut ids = Vec::new();
    for notify in backend.att(0x1b) {
        let frame = device_protocol::Frame::decode(&notify[3..], 20, 128).unwrap();
        if ids.last() != Some(&frame.header.message_id) {
            ids.push(frame.header.message_id);
        }
    }
    assert_eq!(
        ids,
        vec![1, 2, 3],
        "accepted packet bytes and order survive vendor buffer reuse"
    );
    assert_eq!(&response(&backend, 1, 0)[..4], &[0, 0, 0, 0]);
    assert_eq!(response(&backend, 2, 0).last(), Some(&0));
    assert_eq!(response(&backend, 3, 0).last(), Some(&0));
    assert_eq!(transport.scope().unwrap(), Some(scope));
    assert!(transport.receive().unwrap().is_none());
    runtime.shutdown(&mut transport).unwrap();
}

mod async_support;
#[test]
fn test_should_defer_actual_gatt_set_and_finish_detached_write_before_fresh_hello_get() {
    // A temporary handler, early SET receipt, lost detached write or old-generation
    // response leaks all break this real Host/ATT -> Runtime -> owner pipeline.
    use async_support::{Access, Nor, config, initialized};
    use device_services::PropertyBackend;
    use wattch5_product::runtime::{SettingsFactory, SettingsWork};
    let _serial = gatt_support::serialized();
    let nor = Nor::new();
    let blocked = nor.blocked.clone();
    let memory = nor.memory.clone();
    let owner = initialized(nor);
    let access = Access(Cell::new(true));
    let factory = SettingsFactory::new(&owner, &access, config()).unwrap();
    let mut runtime = factory.runtime(device_runtime::Budgets::new(2, 2, 2).unwrap());
    let backend = Backend::default();
    let controller =
        HciController::new(Controller::new(backend.clone(), Version::new(6, 0xffff, 1)));
    let mut resources = Resources::new();
    let stack = build(
        controller,
        &mut resources,
        RandomAddress::new([1, 2, 3, 4, 5, 0xc0]).unwrap(),
    );
    let mut rx = [0; 20];
    let mut tx = [0; 20];
    let mut table: AttributeTable<'_, NoopRawMutex, 6> = AttributeTable::new();
    let profile = ControlProfile::add(&mut table, uuids(), &mut rx, &mut tx).unwrap();
    let server = AttributeServer::<NoopRawMutex, DefaultPacketPool, 6, 1>::new(table);
    let mut peripheral = stack.peripheral();
    let mut runner = stack.runner();
    let mut host = pin!(runner.run());
    let conn = connected!(host, peripheral, backend, server);
    let bridge = Bridge::<2, 1>::new().unwrap();
    let mut driver = pin!(drive_connection(&conn, &profile, &bridge));
    let mut transport = TroubleTransport::new(&bridge, host.as_mut(), driver.as_mut());
    let mut ui = Slice::default();
    let mut background = SettingsWork::new(&owner);
    let mut now = 0;
    let cccd = profile.tx().cccd_handle.unwrap();
    write(&backend, cccd, &[1, 0]);
    let mut slices = |count| {
        for _ in 0..count {
            let r = runtime.step(now, &mut transport, &mut ui, &mut background);
            assert!(r.connection.is_ok(), "{r:?}");
            assert!(r.background.is_ok(), "{r:?}");
            now += 1;
        }
        transport.scope().unwrap()
    };
    let first = slices(32).unwrap();
    exercise_async_set(&backend, &profile, &owner, &access, &blocked, &mut slices);
    exercise_async_detach(&backend, &profile, &owner, &blocked, first, &mut slices);
    runtime.shutdown(&mut transport).unwrap();
    drop(runtime);
    drop(owner);
    let mut nor = Nor::new();
    nor.memory = memory;
    let restored = async_support::open(nor);
    let mut value = [0; 1];
    assert_eq!(
        restored
            .adapter(4)
            .unwrap()
            .read(3, &mut value)
            .unwrap()
            .revision,
        3
    );
    assert_eq!(value, [0]);
}

fn exercise_async_set(
    backend: &Backend,
    profile: &ControlProfile,
    owner: &async_support::Owner,
    access: &async_support::Access,
    blocked: &Cell<bool>,
    step: &mut impl FnMut(usize) -> Option<device_runtime::Scope>,
) {
    use async_support::dictionary;
    use wattch5_parity_ui_demo::Field;
    use wattch5_product::runtime::AsyncSettings;
    request(
        backend,
        profile.rx().handle,
        0,
        1,
        1,
        &[1, 1, 128, 0, 0, 0, 3, 0],
    );
    step(64);
    assert_eq!(
        device_protocol::HelloResponse::decode(&response(backend, 1, 0)[4..])
            .unwrap()
            .session,
        1
    );
    let start = backend.att(0x1b).len();
    blocked.set(true);
    request(
        backend,
        profile.rx().handle,
        4,
        0x11,
        2,
        &[3, 0, 0, 0, 0, 0, 1, 0, 1],
    );
    step(64);
    assert_eq!(backend.att(0x1b).len(), start);
    assert_eq!(owner.revision(), Ok(1));
    assert!(
        AsyncSettings::new(owner, dictionary())
            .unwrap()
            .begin(Field::Format, 0, access)
            .is_err()
    );
    blocked.set(false);
    step(1800);
    assert_eq!(response(backend, 2, start), vec![0, 0, 0, 0, 2, 0, 0, 0]);
    assert_eq!(owner.revision(), Ok(2));
    let replay_start = backend.att(0x1b).len();
    request(
        backend,
        profile.rx().handle,
        4,
        0x11,
        2,
        &[3, 0, 0, 0, 0, 0, 1, 0, 1],
    );
    step(64);
    assert_eq!(
        response(backend, 2, replay_start),
        vec![0, 0, 0, 0, 2, 0, 0, 0]
    );
    assert_eq!(owner.revision(), Ok(2));
}
fn exercise_async_detach(
    backend: &Backend,
    profile: &ControlProfile,
    owner: &async_support::Owner,
    blocked: &Cell<bool>,
    first: device_runtime::Scope,
    step: &mut impl FnMut(usize) -> Option<device_runtime::Scope>,
) {
    let cccd = profile.tx().cccd_handle.unwrap();
    let detached_start = backend.att(0x1b).len();
    blocked.set(true);
    request(
        backend,
        profile.rx().handle,
        4,
        0x11,
        3,
        &[3, 0, 0, 0, 0, 0, 1, 0, 0],
    );
    step(32);
    assert_eq!(backend.att(0x1b).len(), detached_start);
    write(backend, cccd, &[0, 0]);
    step(32);
    assert_eq!(step(0), None);
    blocked.set(false);
    step(1800);
    assert_eq!(owner.revision(), Ok(3));
    assert_eq!(backend.att(0x1b).len(), detached_start);
    write(backend, cccd, &[1, 0]);
    step(32);
    assert!(step(0).unwrap().generation() > first.generation());
    let fresh_start = backend.att(0x1b).len();
    request(
        backend,
        profile.rx().handle,
        0,
        1,
        1,
        &[1, 1, 128, 0, 0, 0, 3, 0],
    );
    step(64);
    assert_eq!(
        device_protocol::HelloResponse::decode(&response(backend, 1, fresh_start)[4..])
            .unwrap()
            .session,
        2
    );
    request(backend, profile.rx().handle, 4, 0x10, 2, &[3, 0]);
    step(64);
    let get = response(backend, 2, fresh_start);
    assert_eq!(&get[..8], &[0, 0, 0, 0, 3, 0, 0, 0]);
    assert_eq!(get.last(), Some(&0));
}
