//! Notification queues must acknowledge admission; no real BLE adapter is exercised.
use device_connection::{ConnectionError, ControlConnection, Receive};
use device_protocol::{Frame, ModuleEntry, ProtocolLimits};
use device_services::{BuildProfile, Config, DeviceInfo, ServiceHandler, SessionError};
#[derive(Debug, Default)]
struct Handler {
    calls: u32,
    admitted: u32,
    closed: u32,
    allowed: bool,
    defer: bool,
}
impl ServiceHandler for Handler {
    fn response_ready(&mut self, _: Frame<'_>) -> Result<bool, SessionError> {
        Ok(!self.defer)
    }
    fn required_features(&self, e: &ModuleEntry, op: u8) -> Option<u32> {
        (e.module == 2 && op == 0x30).then_some(0)
    }
    fn authorized(&self, _: Frame<'_>) -> bool {
        self.allowed
    }
    fn dispatch(&mut self, _: Frame<'_>, out: &mut [u8]) -> Result<usize, SessionError> {
        if out.len() < 14 {
            return Err(SessionError::Capacity);
        }
        self.calls += 1;
        out[..14].fill(0);
        Ok(14)
    }

    fn response_admitted(&mut self, _: Frame<'_>, _: &[u8]) -> Result<(), SessionError> {
        self.admitted += 1;
        Ok(())
    }
    fn connection_closed(&mut self) -> Result<(), SessionError> {
        self.closed += 1;
        Ok(())
    }
}
const MODULES: [ModuleEntry; 2] = [
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
        module: 2,
        major: 1,
        minor: 2,
        availability: 1,
        features: 0,
        input_max: 128,
        output_max: 128,
    },
];
fn connection() -> ControlConnection<'static> {
    ControlConnection::new(Config {
        session: 1,
        capability_rev: 1,
        profile: BuildProfile::Commercial,
        info: DeviceInfo {
            product_id: 5,
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
            data_max: 0,
            idle_ms: 2000,
            fragment_ms: 100,
            response_ms: 10000,
        },
    })
    .unwrap_or_else(|e| panic!("{e:?}"))
}
fn hello(c: &mut ControlConnection<'_>, h: &mut Handler) {
    c.notifications_enabled()
        .unwrap_or_else(|e| panic!("{e:?}"));
    c.receive(
        &[0x10, 0, 1, 0, 1, 0, 0, 0, 8, 0, 1, 1, 128, 0, 0, 0, 3, 0],
        0,
        h,
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    drain(c, h);
}
fn drain(c: &mut ControlConnection<'_>, h: &mut Handler) {
    let mut b = [0; 20];
    while c.response_pending() {
        c.next_frame(h, &mut b).unwrap_or_else(|e| panic!("{e:?}"));
        c.admit_frame(h).unwrap_or_else(|e| panic!("{e:?}"));
    }
}
const ACTION: [u8; 10] = [0x10, 2, 0x30, 0, 2, 0, 0, 0, 0, 0];
#[test]
fn test_should_release_receive_frame_before_dispatching_one_owned_request() {
    let mut c = connection();
    let mut h = Handler {
        allowed: true,
        ..Handler::default()
    };
    hello(&mut c, &mut h);
    assert_eq!(c.receive_staged(&ACTION, 1, &mut h), Ok(Receive::Deferred));
    assert_eq!(h.calls, 0);
    assert!(!c.response_pending());
    assert_eq!(
        c.receive_staged(&ACTION, 1, &mut h),
        Err(ConnectionError::Busy)
    );
    assert_eq!(c.dispatch_received(&mut h), Ok(Receive::ResponseReady));
    assert_eq!(h.calls, 1);
    assert_eq!(
        c.dispatch_received(&mut h),
        Err(ConnectionError::InvalidState)
    );
    drain(&mut c, &mut h);
    assert_eq!(h.admitted, 1);
}
#[test]
fn test_should_retry_notification_bytes_until_admitted_and_block_another_request() {
    let mut c = connection();
    let mut h = Handler {
        allowed: true,
        ..Handler::default()
    };
    hello(&mut c, &mut h);
    assert_eq!(c.receive(&ACTION, 1, &mut h), Ok(Receive::ResponseReady));
    assert_eq!(h.calls, 1);
    assert_eq!(h.admitted, 0);
    let mut a = [0; 20];
    let mut b = [0; 20];
    assert_eq!(c.next_frame(&mut h, &mut a), Ok(20));
    assert_eq!(c.next_frame(&mut h, &mut b), Ok(20));
    assert_eq!(a, b);
    assert_eq!(c.receive(&ACTION, 2, &mut h), Err(ConnectionError::Busy));
    c.admit_frame(&mut h).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(h.admitted, 0);
    assert_eq!(c.next_frame(&mut h, &mut b), Ok(14));
    assert_eq!(b[6], 10);
    c.admit_frame(&mut h).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(h.admitted, 1);
    assert_eq!(c.receive(&ACTION, 3, &mut h), Ok(Receive::ResponseReady));
    drain(&mut c, &mut h);
    assert_eq!(h.calls, 1);
    assert_eq!(h.admitted, 1);
}
#[test]
fn test_should_close_and_clear_private_data_when_scope_is_revoked_while_queue_is_full() {
    let mut c = connection();
    let mut h = Handler {
        allowed: true,
        ..Handler::default()
    };
    hello(&mut c, &mut h);
    c.receive(&ACTION, 1, &mut h)
        .unwrap_or_else(|e| panic!("{e:?}"));
    h.allowed = false;
    let mut out = [0xa5; 20];
    assert!(c.next_frame(&mut h, &mut out).is_err());
    assert!(out.iter().all(|b| *b == 0));
    assert!(!c.response_pending());
    assert_eq!(h.admitted, 0);
    assert_eq!(h.closed, 1);
    assert!(c.notifications_enabled().is_err());
}
#[test]
fn test_should_require_preparation_before_admission_and_close_on_malformed_receive() {
    let mut c = connection();
    let mut h = Handler {
        allowed: true,
        ..Handler::default()
    };
    hello(&mut c, &mut h);
    c.receive(&ACTION, 1, &mut h)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(c.admit_frame(&mut h), Err(ConnectionError::InvalidState));
    drain(&mut c, &mut h);
    assert!(c.receive(&[0; 9], 2, &mut h).is_err());
    assert_eq!(h.closed, 1);
    assert!(!c.response_pending());
}
#[test]
fn test_should_reassemble_multiple_control_fragments_using_negotiated_frame_size() {
    let mut c = connection();
    let mut h = Handler {
        allowed: true,
        ..Handler::default()
    };
    hello(&mut c, &mut h);
    let first = [
        0x10, 2, 0x30, 0, 2, 0, 0, 0, 14, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10,
    ];
    assert_eq!(c.receive(&first, 1, &mut h), Ok(Receive::Pending));
    assert_eq!(h.calls, 0);
    let last = [0x10, 2, 0x30, 0, 2, 0, 10, 0, 14, 0, 11, 12, 13, 14];
    assert_eq!(c.receive(&last, 2, &mut h), Ok(Receive::ResponseReady));
    drain(&mut c, &mut h);
    assert_eq!(h.calls, 1);
    assert_eq!(h.admitted, 1);
}

#[test]
fn test_should_poll_deferred_reply_without_blocking_or_dispatching_another_request() {
    let mut c = connection();
    let mut h = Handler {
        allowed: true,
        ..Handler::default()
    };
    hello(&mut c, &mut h);
    h.defer = true;
    assert_eq!(c.receive(&ACTION, 1, &mut h), Ok(Receive::Deferred));
    assert!(!c.response_pending());
    assert!(c.waiting_for_handler());
    assert_eq!(h.calls, 0);
    assert_eq!(c.receive(&ACTION, 2, &mut h), Err(ConnectionError::Busy));
    assert_eq!(c.poll_pending(2, &mut h), Ok(Receive::Deferred));
    h.defer = false;
    assert_eq!(c.poll_pending(3, &mut h), Ok(Receive::ResponseReady));
    drain(&mut c, &mut h);
    assert_eq!(h.calls, 1);
    assert_eq!(h.admitted, 1);
}
#[test]
fn test_should_end_deferred_work_after_real_response_budget_without_wire_pending() {
    let mut c = connection();
    let mut h = Handler {
        allowed: true,
        ..Handler::default()
    };
    hello(&mut c, &mut h);
    h.defer = true;
    c.receive(&ACTION, 1, &mut h)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(c.poll_pending(10_001, &mut h), Ok(Receive::Deferred));
    assert!(c.poll_pending(10_002, &mut h).is_err());
    assert_eq!(h.closed, 1);
    assert_eq!(h.calls, 0);
    assert_eq!(h.admitted, 0);
}
struct Resources<'a>(&'a core::cell::Cell<bool>);
impl device_services::TrustedAccessProvider for Resources<'_> {
    fn access_snapshot(&self) -> device_services::AccessSnapshot {
        device_services::AccessSnapshot {
            state: device_services::AccessState::Authorized,
            reason: 0,
        }
    }
}
impl device_services::CoreResources for Resources<'_> {
    fn authorized(&self, _: device_services::CoreRequest) -> bool {
        self.0.get()
    }
    fn validate(
        &self,
        _: device_services::CoreRequest,
    ) -> Result<(), device_services::ResourceFailure> {
        Ok(())
    }
    fn grant(
        &mut self,
        r: device_services::GrantRequest,
    ) -> Result<u32, device_services::ResourceFailure> {
        Ok(r.cumulative)
    }
    fn dataset(
        &mut self,
    ) -> Result<device_services::DatasetIdentity, device_services::ResourceFailure> {
        device_services::DatasetIdentity::new([7; 16])
    }
    fn replay_valid(&self, _: device_services::CoreRequest, _: &[u8]) -> bool {
        self.0.get()
    }
}
#[test]
fn test_should_recheck_protected_core_scope_before_every_queued_dataset_fragment() {
    let allowed = core::cell::Cell::new(true);
    let mut resources = Resources(&allowed);
    let mut handler = device_services::CoreResourceHandler::new(MODULES[0], &mut resources)
        .unwrap_or_else(|e| panic!("{e:?}"));
    let mut c = connection();
    c.notifications_enabled()
        .unwrap_or_else(|e| panic!("{e:?}"));
    c.receive(
        &[0x10, 0, 1, 0, 1, 0, 0, 0, 8, 0, 1, 1, 128, 0, 0, 0, 3, 0],
        0,
        &mut handler,
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    let mut out = [0; 20];
    while c.response_pending() {
        c.next_frame(&mut handler, &mut out)
            .unwrap_or_else(|e| panic!("{e:?}"));
        c.admit_frame(&mut handler)
            .unwrap_or_else(|e| panic!("{e:?}"));
    }
    c.receive(&[0x10, 0, 8, 0, 2, 0, 0, 0, 0, 0], 1, &mut handler)
        .unwrap_or_else(|e| panic!("{e:?}"));
    allowed.set(false);
    out.fill(0xa5);
    assert!(c.next_frame(&mut handler, &mut out).is_err());
    assert!(out.iter().all(|b| *b == 0));
    assert!(!c.response_pending());
    assert!(c.notifications_enabled().is_err());
}
