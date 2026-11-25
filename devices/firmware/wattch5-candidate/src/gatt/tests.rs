//! Candidate actual physical-value FIFO reservation/scoping, independent of logical DATA512.
#![allow(clippy::unwrap_used)]
use super::*;
use device_connection::{EventPlane, FirstFragmentQueue, FirstSlotOutcome};
use device_runtime::PortError;
#[test]
fn test_should_require_both_cccds_and_revoke_exact_old_scope() {
    let bridge = Bridge::<1, 1>::new().unwrap();
    let epoch = bridge.begin().unwrap();
    bridge.subscription(epoch, true).unwrap();
    assert_eq!(bridge.scope().unwrap(), None);
    bridge.data_subscription(epoch, true).unwrap();
    let first = bridge.scope().unwrap().unwrap();
    bridge.incoming(epoch, 0, &[9; 20]).unwrap();
    bridge.data_subscription(epoch, false).unwrap();
    assert_eq!(bridge.scope().unwrap(), None);
    assert!(bridge.receive_event::<128>().unwrap().is_none());
    bridge.data_subscription(epoch, true).unwrap();
    let next = bridge.scope().unwrap().unwrap();
    assert!(next.generation() > first.generation());
    bridge.close(first).unwrap();
    assert_eq!(bridge.scope().unwrap(), Some(next));
}
#[test]
fn test_should_reserve_real_tagged_fifo_before_fill_and_keep_exact_twenty_bytes() {
    let bridge = Bridge::<1, 1>::new().unwrap();
    let epoch = bridge.begin().unwrap();
    bridge.subscription(epoch, true).unwrap();
    bridge.data_subscription(epoch, true).unwrap();
    let scope = bridge.scope().unwrap().unwrap();
    let mut queue = bridge.event_queue(scope, EventPlane::Data).unwrap();
    assert!(matches!(
        queue.with_reserved_slot(|out| {
            assert_eq!(out.len(), 20);
            out.copy_from_slice(&[7; 20]);
            Ok::<_, ()>((20, ()))
        }),
        Ok(FirstSlotOutcome::Admitted(()))
    ));
    let mut called = false;
    assert!(matches!(
        queue.with_reserved_slot(|_| {
            called = true;
            Ok::<_, ()>((1, ()))
        }),
        Ok(FirstSlotOutcome::NoSlot)
    ));
    assert!(!called);
    assert_eq!(
        bridge.data_subscription(epoch, false),
        Err(PortError::Fault)
    );
    drop(queue);
    let frame = bridge.outgoing(epoch).unwrap().unwrap();
    assert_eq!(frame.plane, EventPlane::Data);
    assert_eq!(&frame.bytes[..frame.len], &[7; 20]);
    bridge.data_subscription(epoch, false).unwrap();
    bridge.data_subscription(epoch, true).unwrap();
    assert!(bridge.event_queue(scope, EventPlane::Data).is_err());
}

#[test]
fn test_should_finish_actual_accepted_property_commit_after_ble_failure_with_fair_ui_and_host() {
    use crate::async_support::{Nor, Owner, initialized};
    use core::{cell::Cell, future::poll_fn, pin::pin, task::Poll};
    use device_connection::{
        EventOffer, EventRoute, EventSource, SharedControlChannel, SourceFailure,
    };
    use device_protocol::{Frame, Header, Kind, ModuleEntry, ProtocolLimits};
    use device_runtime::{Budgets, EventRuntime, Work};
    use device_services::{
        BuildProfile, Config, DeviceInfo, PropertyAccess, PropertyBackend, PropertyOperation,
    };
    struct Access;
    impl PropertyAccess for Access {
        fn allowed(&self, _: u8, _: PropertyOperation, _: Option<u16>) -> bool {
            true
        }
    }
    struct Events;
    impl EventSource for Events {
        type Token = ();
        fn route(&self) -> Result<Option<EventRoute>, SourceFailure> {
            Ok(None)
        }
        fn prepare(&mut self, _: &mut [u8]) -> Result<EventOffer<()>, SourceFailure> {
            Err(SourceFailure::NotReady)
        }
        fn authorized(&self, _: ()) -> bool {
            false
        }
        fn must_end_session(&self) -> bool {
            false
        }
        fn admitted(&mut self, _: ()) -> Result<(), SourceFailure> {
            Err(SourceFailure::Invalid)
        }
        fn connection_closed(&mut self) -> Result<(), SourceFailure> {
            Ok(())
        }
    }
    struct Worker<'a> {
        owner: &'a Owner,
        calls: u32,
    }
    impl Work for Worker<'_> {
        fn step(&mut self, _: u64) -> Result<(), PortError> {
            self.calls += 1;
            self.owner.step().map_err(|_| PortError::Uncertain)
        }
    }
    struct Ui(u32);
    impl Work for Ui {
        fn step(&mut self, _: u64) -> Result<(), PortError> {
            self.0 += 1;
            Ok(())
        }
    }
    let medium = Nor::new();
    let owner = initialized(medium.clone());
    let access = Access;
    let bridge = Bridge::<1, 1>::new().unwrap();
    let epoch = bridge.begin().unwrap();
    bridge.subscription(epoch, true).unwrap();
    bridge.data_subscription(epoch, true).unwrap();
    let scope = bridge.scope().unwrap().unwrap();
    let host_calls = Cell::new(0);
    let gatt_calls = Cell::new(0);
    let fault = Cell::new(false);
    let mut host = pin!(poll_fn(|_| {
        host_calls.set(host_calls.get() + 1);
        Poll::<Result<(), ()>>::Pending
    }));
    let mut gatt = pin!(poll_fn(|_| {
        gatt_calls.set(gatt_calls.get() + 1);
        if fault.get() {
            Poll::Ready(Err(PortError::Fault))
        } else {
            Poll::Pending
        }
    }));
    let mut transport = TroubleTransport::new(&bridge, host.as_mut(), gatt.as_mut());
    let entries = [
        ModuleEntry {
            module: 0,
            major: 1,
            minor: 2,
            availability: 1,
            features: 0,
            input_max: 128,
            output_max: 128,
        },
        ModuleEntry {
            module: 4,
            major: 1,
            minor: 2,
            availability: 1,
            features: 0,
            input_max: 128,
            output_max: 128,
        },
    ];
    let config = Config {
        session: 1,
        capability_rev: 1,
        profile: BuildProfile::Development,
        info: DeviceInfo {
            product_id: 0,
            fw_major: 0,
            fw_minor: 1,
            fw_patch: 0,
            build_id: 1,
        },
        modules: &entries,
        limits: ProtocolLimits {
            rx_frame_max: 20,
            tx_frame_max: 20,
            control_max: 128,
            data_max: 512,
            idle_ms: 2000,
            fragment_ms: 100,
            response_ms: 10000,
        },
    };
    let handler = device_property_store::AsyncPropertyHandler::<_, _, 128, 128>::new(
        &owner,
        4,
        &access,
        (1, 2),
        128,
        128,
    )
    .unwrap();
    let channel = SharedControlChannel::new();
    let mut runtime = EventRuntime::<_, _, 128, 20>::new(
        config,
        scope,
        &channel,
        handler,
        Events,
        Budgets::new(1, 1, 2).unwrap(),
    )
    .unwrap();
    let mut worker = Worker {
        owner: &owner,
        calls: 0,
    };
    let mut ui = Ui(0);
    let mut bytes = [0; 20];
    let hello = Frame {
        header: Header {
            kind: Kind::Request,
            module: 0,
            opcode: 1,
            message_id: 1,
            offset: 0,
            total_length: 8,
        },
        payload: &[1, 1, 128, 0, 0, 2, 3, 0],
    };
    let length = hello.encode(&mut bytes).unwrap();
    bridge.incoming(epoch, 0, &bytes[..length]).unwrap();
    for now in 0..20 {
        assert!(
            runtime
                .step(now, &mut transport, &mut ui, &mut worker)
                .connection
                .is_ok()
        );
        let _ = bridge.outgoing(epoch).unwrap();
    }
    let set = Frame {
        header: Header {
            kind: Kind::Request,
            module: 4,
            opcode: 0x11,
            message_id: 2,
            offset: 0,
            total_length: 9,
        },
        payload: &[1, 0, 1, 0, 0, 0, 1, 0, 1],
    };
    let length = set.encode(&mut bytes).unwrap();
    bridge.incoming(epoch, 0, &bytes[..length]).unwrap();
    assert!(
        runtime
            .step(20, &mut transport, &mut ui, &mut worker)
            .connection
            .is_ok()
    );
    assert_eq!(owner.revision(), Ok(1));
    fault.set(true);
    for now in 21..1500 {
        assert!(
            runtime
                .step(now, &mut transport, &mut ui, &mut worker)
                .connection
                .is_err()
        );
    }
    assert_eq!(owner.revision(), Ok(2));
    assert_eq!(worker.calls, 1500);
    assert_eq!(ui.0, 1500);
    assert_eq!(host_calls.get(), 1500);
    assert!(gatt_calls.get() > 20);
    // A fresh independent recovery reads the durable medium, rather than a RAM-only revision.
    let reopened = Owner::open(
        crate::async_support::engine(medium),
        crate::async_support::CATALOGUE,
        crate::async_support::Schema,
    )
    .unwrap();
    assert_eq!(reopened.revision(), Ok(2));
    let mut actual = [0; 8];
    let value = reopened.adapter(4).unwrap().read(1, &mut actual).unwrap();
    assert_eq!(&actual[..value.length], &[1]);
}
