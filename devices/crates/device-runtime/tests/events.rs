//! Actual CONTROL/EventPump scheduling with finite slow persistence slices.
#![allow(clippy::unwrap_used)]
use core::cell::Cell;
use device_connection::{
    EventOffer, EventPlane, EventRoute, EventSource, FirstFragmentQueue, FirstSlotError,
    FirstSlotOutcome, SharedControlChannel, SourceFailure,
};
use device_protocol::{Frame, ModuleEntry, ProtocolLimits};
use device_runtime::{
    Budgets, EventRuntime, EventTransport, OwnedRx, PortError, Scope, Transport, Work,
};
use device_services::{BuildProfile, Config, DeviceInfo, ServiceHandler, SessionError};
struct Handler;
impl ServiceHandler for Handler {
    fn required_features(&self, _: &ModuleEntry, _: u8) -> Option<u32> {
        None
    }
    fn authorized(&self, _: Frame<'_>) -> bool {
        false
    }
    fn dispatch(&mut self, _: Frame<'_>, _: &mut [u8]) -> Result<usize, SessionError> {
        Err(SessionError::InvalidHandlerResponse)
    }
}
struct Source;
impl EventSource for Source {
    type Token = u8;
    fn route(&self) -> Result<Option<EventRoute>, SourceFailure> {
        Ok(None)
    }
    fn prepare(&mut self, _: &mut [u8]) -> Result<EventOffer<u8>, SourceFailure> {
        Err(SourceFailure::NotReady)
    }
    fn authorized(&self, _: u8) -> bool {
        false
    }
    fn must_end_session(&self) -> bool {
        false
    }
    fn admitted(&mut self, _: u8) -> Result<(), SourceFailure> {
        Err(SourceFailure::Invalid)
    }
    fn connection_closed(&mut self) -> Result<(), SourceFailure> {
        Ok(())
    }
}
struct Queue;
impl FirstFragmentQueue for Queue {
    fn with_reserved_slot<T, E>(
        &mut self,
        _: impl FnOnce(&mut [u8]) -> Result<(usize, T), E>,
    ) -> Result<FirstSlotOutcome<T>, FirstSlotError<E>> {
        Ok(FirstSlotOutcome::NoSlot)
    }
}
struct Wire {
    scope: Option<Scope>,
    polls: u32,
    fail: bool,
    revoke_on_rx: bool,
}
impl Transport for Wire {
    type Queue<'a> = Queue;
    fn poll(&mut self, _: u64, budget: u8) -> Result<(), PortError> {
        self.polls += u32::from(budget);
        if self.fail {
            Err(PortError::Fault)
        } else {
            Ok(())
        }
    }
    fn scope(&self) -> Result<Option<Scope>, PortError> {
        Ok(self.scope)
    }
    fn receive(&mut self) -> Result<Option<OwnedRx>, PortError> {
        if self.revoke_on_rx {
            self.revoke_on_rx = false;
            let scope = self.scope.take().unwrap();
            return Ok(Some(
                OwnedRx::new(
                    scope,
                    &[0x10, 0, 1, 0, 1, 0, 0, 0, 8, 0, 1, 1, 128, 0, 0, 2, 3, 0],
                )
                .unwrap(),
            ));
        }
        Ok(None)
    }
    fn queue(&mut self, _: Scope) -> Result<Queue, PortError> {
        Ok(Queue)
    }
    fn close_scope(&mut self, _: Scope) -> Result<(), PortError> {
        self.scope = None;
        Ok(())
    }
}
impl EventTransport for Wire {
    type EventQueue<'a> = Queue;
    fn event_queue(&mut self, _: Scope, _: EventPlane) -> Result<Queue, PortError> {
        Ok(Queue)
    }
}
struct Slice<'a> {
    calls: &'a Cell<u32>,
    remaining: u32,
}
impl Work for Slice<'_> {
    fn step(&mut self, _: u64) -> Result<(), PortError> {
        self.calls.set(self.calls.get() + 1);
        self.remaining = self.remaining.saturating_sub(1);
        Ok(())
    }
}
const MODULES: [ModuleEntry; 1] = [ModuleEntry {
    module: 0,
    major: 1,
    minor: 2,
    availability: 1,
    features: 0,
    input_max: 8,
    output_max: 23,
}];
#[test]
fn test_should_finish_accepted_slow_work_after_ble_failure_and_keep_ui_host_fair() {
    let scope = Scope::new(1, 20).unwrap();
    let channel = SharedControlChannel::new();
    let cfg = Config {
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
        modules: &MODULES,
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
    let rejecting_channel = SharedControlChannel::new();
    assert!(
        EventRuntime::<_, _, 128>::new(
            cfg,
            scope,
            &rejecting_channel,
            Handler,
            Source,
            Budgets::new(1, 255, 2).unwrap()
        )
        .is_err()
    );
    let mut runtime = EventRuntime::<_, _, 128>::new(
        cfg,
        scope,
        &channel,
        Handler,
        Source,
        Budgets::new(1, 1, 2).unwrap(),
    )
    .unwrap();
    let mut wire = Wire {
        scope: Some(scope),
        polls: 0,
        fail: false,
        revoke_on_rx: false,
    };
    let u = Cell::new(0);
    let b = Cell::new(0);
    let mut ui = Slice {
        calls: &u,
        remaining: 0,
    };
    let mut background = Slice {
        calls: &b,
        remaining: 100,
    };
    for now in 0..100 {
        if now == 3 {
            wire.fail = true;
        }
        let r = runtime.step(now, &mut wire, &mut ui, &mut background);
        assert_eq!(r.connection.is_err(), now >= 3);
    }
    assert_eq!(
        (wire.polls, u.get(), b.get(), background.remaining),
        (200, 100, 100, 0)
    );
}
#[test]
fn test_should_reject_overcapacity_rx_without_truncating() {
    let scope = Scope::new(2, 512).unwrap();
    assert!(device_runtime::BoundedRx::<128>::new(scope, &[9; 129]).is_err());
    assert!(device_runtime::BoundedRx::<128>::new(scope, &[9; 128]).is_ok());
    assert!(
        core::mem::size_of::<device_runtime::BoundedRx<128>>() < core::mem::size_of::<OwnedRx>()
    );
}

#[test]
fn test_should_reject_scope_revocation_inside_rx_before_dispatch() {
    let scope = Scope::new(1, 20).unwrap();
    let channel = SharedControlChannel::new();
    let cfg = Config {
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
        modules: &MODULES,
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
    let mut runtime = EventRuntime::<_, _, 128>::new(
        cfg,
        scope,
        &channel,
        Handler,
        Source,
        Budgets::new(1, 1, 2).unwrap(),
    )
    .unwrap();

    let mut wire = Wire {
        scope: Some(scope),
        polls: 0,
        fail: false,
        revoke_on_rx: true,
    };
    let u = Cell::new(0);
    let b = Cell::new(0);
    let mut ui = Slice {
        calls: &u,
        remaining: 0,
    };
    let mut background = Slice {
        calls: &b,
        remaining: 0,
    };
    assert!(
        runtime
            .step(1, &mut wire, &mut ui, &mut background)
            .connection
            .is_err()
    );
    assert_eq!((u.get(), b.get()), (1, 1));
}

#[test]
fn test_should_bound_physical_frame_storage_separately_from_logical_control_capacity() {
    let scope = Scope::new(1, 20).unwrap();
    let cfg = Config {
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
        modules: &MODULES,
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
    let channel = SharedControlChannel::new();
    assert!(
        EventRuntime::<_, _, 128, 20>::new(
            cfg,
            scope,
            &channel,
            Handler,
            Source,
            Budgets::new(1, 1, 2).unwrap()
        )
        .is_ok()
    );
    let channel = SharedControlChannel::new();
    assert!(
        EventRuntime::<_, _, 128, 19>::new(
            cfg,
            scope,
            &channel,
            Handler,
            Source,
            Budgets::new(1, 1, 2).unwrap()
        )
        .is_err()
    );
    assert!(
        core::mem::size_of::<EventRuntime<'_, Handler, Source, 128, 20>>() + 80
            < core::mem::size_of::<EventRuntime<'_, Handler, Source, 128>>()
    );
}
