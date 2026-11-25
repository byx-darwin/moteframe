//! Session routing tests use only a test-side counter, never hardware effects.
#![allow(clippy::unwrap_used)]
use core::cell::Cell;
use device_protocol::{Frame, Header, Kind, ModuleEntry, ProtocolLimits};
use device_services::{
    BuildProfile, Config, DeviceInfo, ServiceHandler, Session, SessionError, SessionState,
};

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
        module: 4,
        major: 1,
        minor: 0,
        availability: 1,
        features: 0,
        input_max: 128,
        output_max: 128,
    },
];
fn config(modules: &[ModuleEntry]) -> Config<'_> {
    Config {
        session: 1,
        capability_rev: 1,
        profile: BuildProfile::Commercial,
        info: DeviceInfo {
            product_id: 0,
            fw_major: 0,
            fw_minor: 1,
            fw_patch: 0,
            build_id: 0,
        },
        modules,
        limits: ProtocolLimits {
            rx_frame_max: 20,
            tx_frame_max: 20,
            control_max: 128,
            data_max: 0,
            idle_ms: 2000,
            fragment_ms: 100,
            response_ms: 10000,
        },
    }
}
fn request(module: u8, opcode: u8, id: u16, body: &[u8]) -> Frame<'_> {
    Frame {
        header: Header {
            kind: Kind::Request,
            module,
            opcode,
            message_id: id,
            offset: 0,
            total_length: u16::try_from(body.len()).unwrap(),
        },
        payload: body,
    }
}
fn active(modules: &[ModuleEntry]) -> Session<'_> {
    let mut session = Session::new(config(modules)).unwrap();
    session.control_notifications_enabled().unwrap();
    session
        .dispatch(request(0, 1, 1, &[1, 1, 128, 0, 0, 0, 3, 0]), &mut [0; 128])
        .unwrap();
    session
}

#[test]
fn test_should_match_typed_and_dynamic_session_replay_and_revocation() {
    let mut dynamic = active(&MODULES);
    let mut typed = active(&MODULES);
    let mut left = Counter::new();
    let mut right = Counter::new();
    let req = request(4, 0x11, 2, &[9]);
    let mut a = [0xaa; 128];
    let mut b = [0xaa; 128];
    assert_eq!(
        typed.dispatch_with_typed_handler(req, &mut right, &mut b[..5]),
        Err(SessionError::Capacity)
    );
    assert_eq!(right.calls, 0);
    assert_eq!(b, [0xaa; 128]);
    for _ in 0..2 {
        assert_eq!(dynamic.dispatch_with_handler(req, &mut left, &mut a), Ok(5));
        assert_eq!(
            typed.dispatch_with_typed_handler(req, &mut right, &mut b),
            Ok(5)
        );
        assert_eq!(&a[..5], &[0, 0, 0, 0, 1]);
        assert_eq!(a, b);
    }
    assert_eq!((left.calls, right.calls), (1, 1));
    left.allowed.set(false);
    right.allowed.set(false);
    assert_eq!(
        dynamic.dispatch_with_handler(req, &mut left, &mut a),
        Err(SessionError::AuthorizationRevoked)
    );
    assert_eq!(
        typed.dispatch_with_typed_handler(req, &mut right, &mut b),
        Err(SessionError::AuthorizationRevoked)
    );
    assert_eq!(a, [0; 128]);
    assert_eq!(a, b);
    assert_eq!(typed.state(), SessionState::Closed);
}
#[derive(Debug)]
struct Counter {
    calls: u8,
    allowed: Cell<bool>,
    required: u32,
    revoke: bool,
    invalid_length: bool,
}
impl Counter {
    fn new() -> Self {
        Self {
            calls: 0,
            allowed: Cell::new(true),
            required: 0,
            revoke: false,
            invalid_length: false,
        }
    }
}
impl ServiceHandler for Counter {
    fn required_features(&self, module: &ModuleEntry, opcode: u8) -> Option<u32> {
        (module.module == 4 && module.major == 1 && opcode == 0x11).then_some(self.required)
    }
    fn authorized(&self, _request: Frame<'_>) -> bool {
        self.allowed.get()
    }
    fn dispatch(&mut self, _request: Frame<'_>, out: &mut [u8]) -> Result<usize, SessionError> {
        self.calls += 1;
        out[..5].copy_from_slice(&[0, 0, 0, 0, self.calls]);
        if self.revoke {
            self.allowed.set(false);
        }
        Ok(if self.invalid_length {
            out.len() + 1
        } else {
            5
        })
    }
}

#[test]
fn identical_replay_executes_backend_only_once_and_changed_payload_faults() {
    let mut session = active(&MODULES);
    let mut handler = Counter::new();
    let mut out = [0xaa; 128];
    for _ in 0..2 {
        let n = session
            .dispatch_with_handler(request(4, 0x11, 2, &[9]), &mut handler, &mut out)
            .unwrap();
        assert_eq!(&out[..n], &[0, 0, 0, 0, 1]);
    }
    assert_eq!(handler.calls, 1);
    assert_eq!(
        session.dispatch_with_handler(request(4, 0x11, 2, &[8]), &mut handler, &mut out),
        Err(SessionError::RequestConflict)
    );
    assert_eq!(handler.calls, 1);
    assert_eq!(session.state(), SessionState::Faulted);
}

#[test]
fn before_hello_unregistered_unavailable_faulted_and_feature_mismatch_never_dispatch() {
    let mut handler = Counter::new();
    let mut out = [0; 128];
    let mut session = Session::new(config(&MODULES)).unwrap();
    session.control_notifications_enabled().unwrap();
    let n = session
        .dispatch_with_handler(request(4, 0x11, 1, &[]), &mut handler, &mut out)
        .unwrap();
    assert_eq!(&out[..n], &[4, 0, 0, 0]);
    let mut session = active(&MODULES);
    let n = session
        .dispatch_with_handler(request(7, 0x11, 2, &[]), &mut handler, &mut out)
        .unwrap();
    assert_eq!(&out[..n], &[2, 0, 0, 0]);
    for (availability, expected) in [(0, [4, 0, 0, 0]), (2, [14, 0, 0, 0])] {
        let mut modules = MODULES;
        modules[1].availability = availability;
        let mut session = active(&modules);
        let n = session
            .dispatch_with_handler(request(4, 0x11, 2, &[]), &mut handler, &mut out)
            .unwrap();
        assert_eq!(&out[..n], &expected);
    }
    handler.required = 2;
    let mut session = active(&MODULES);
    let n = session
        .dispatch_with_handler(request(4, 0x11, 2, &[]), &mut handler, &mut out)
        .unwrap();
    assert_eq!(&out[..n], &[3, 0, 0, 0]);
    assert_eq!(handler.calls, 0);
}

#[test]
fn valid_feature_and_version_mapping_is_required_before_dispatch() {
    let mut handler = Counter::new();
    let mut out = [0; 128];
    let mut modules = MODULES;
    {
        let mut session = active(&modules);
        assert_eq!(
            session.dispatch_with_handler(request(4, 0x11, 2, &[]), &mut handler, &mut out),
            Ok(5)
        );
    }
    assert_eq!(handler.calls, 1);
    modules[1].major = 2;
    let mut session = active(&modules);
    let n = session
        .dispatch_with_handler(request(4, 0x11, 2, &[]), &mut handler, &mut out)
        .unwrap();
    assert_eq!(&out[..n], &[3, 0, 0, 0]);
    assert_eq!(handler.calls, 1);
}

#[test]
fn insufficient_declared_response_capacity_does_not_consume_or_execute_request() {
    let mut session = active(&MODULES);
    let mut handler = Counter::new();
    let mut out = [0xaa; 128];
    assert_eq!(
        session.dispatch_with_handler(request(4, 0x11, 2, &[]), &mut handler, &mut out[..5]),
        Err(SessionError::Capacity)
    );
    assert_eq!(handler.calls, 0);
    assert_eq!(out, [0xaa; 128]);
    assert_eq!(
        session.dispatch_with_handler(request(4, 0x11, 2, &[]), &mut handler, &mut out),
        Ok(5)
    );
    assert_eq!(handler.calls, 1);
}

#[test]
fn denied_handler_scope_and_default_dispatch_never_invent_authorization() {
    let mut session = active(&MODULES);
    let mut handler = Counter::new();
    handler.allowed.set(false);
    let mut out = [0xaa; 128];
    let n = session
        .dispatch_with_handler(request(4, 0x11, 2, &[]), &mut handler, &mut out)
        .unwrap();
    assert_eq!(&out[..n], &[5, 0, 3, 255]);
    assert_eq!(handler.calls, 0);
    handler.allowed.set(true);
    session
        .dispatch_with_handler(request(4, 0x11, 3, &[]), &mut handler, &mut out)
        .unwrap();
    let n = session
        .dispatch(request(4, 0x11, 3, &[]), &mut out)
        .unwrap();
    assert_eq!(&out[..n], &[5, 0, 3, 255]);
    assert_eq!(handler.calls, 1);
}

#[test]
fn revoked_replay_clears_cache_and_output_without_repeating_effect() {
    let mut session = active(&MODULES);
    let mut handler = Counter::new();
    let mut out = [0xaa; 128];
    session
        .dispatch_with_handler(request(4, 0x11, 2, &[]), &mut handler, &mut out)
        .unwrap();
    handler.allowed.set(false);
    assert_eq!(
        session.dispatch_with_handler(request(4, 0x11, 2, &[]), &mut handler, &mut out),
        Err(SessionError::AuthorizationRevoked)
    );
    assert_eq!(out, [0; 128]);
    assert_eq!(handler.calls, 1);
    assert_eq!(session.state(), SessionState::Closed);
}

#[test]
fn revoke_during_execution_suppresses_reply_and_never_retries_committed_work() {
    let mut session = active(&MODULES);
    let mut handler = Counter::new();
    handler.revoke = true;
    let mut out = [0xaa; 128];
    assert_eq!(
        session.dispatch_with_handler(request(4, 0x11, 2, &[]), &mut handler, &mut out),
        Err(SessionError::AuthorizationRevoked)
    );
    assert_eq!(out, [0; 128]);
    assert_eq!(handler.calls, 1);
    handler.allowed.set(true);
    assert_eq!(
        session.dispatch_with_handler(request(4, 0x11, 2, &[]), &mut handler, &mut out),
        Err(SessionError::Closed)
    );
    assert_eq!(handler.calls, 1);
}

#[test]
fn invalid_handler_response_faults_instead_of_caching_success() {
    let mut session = active(&MODULES);
    let mut handler = Counter::new();
    handler.invalid_length = true;
    let mut out = [0xaa; 128];
    assert_eq!(
        session.dispatch_with_handler(request(4, 0x11, 2, &[]), &mut handler, &mut out),
        Err(SessionError::InvalidHandlerResponse)
    );
    assert_eq!(session.state(), SessionState::Faulted);
    assert_eq!(out, [0xaa; 128]);
}
#[derive(Debug)]
struct ClosedResource {
    closed: Cell<bool>,
}
impl ServiceHandler for ClosedResource {
    fn must_end_session(&self) -> bool {
        self.closed.get()
    }
    fn required_features(&self, _: &ModuleEntry, _: u8) -> Option<u32> {
        Some(0)
    }
    fn authorized(&self, _: Frame<'_>) -> bool {
        self.closed.set(true);
        false
    }
    fn dispatch(&mut self, _: Frame<'_>, _: &mut [u8]) -> Result<usize, SessionError> {
        Err(SessionError::InvalidHandlerResponse)
    }
}
#[test]
fn test_should_close_new_request_immediately_if_owned_resource_revocation_ended_handler() {
    let mut session = active(&MODULES);
    let mut handler = ClosedResource {
        closed: Cell::new(false),
    };
    let mut out = [0xa5; 128];
    assert_eq!(
        session.dispatch_with_handler(request(4, 0x11, 2, &[]), &mut handler, &mut out),
        Err(SessionError::AuthorizationRevoked)
    );
    assert_eq!(session.state(), SessionState::Closed);
    assert!(out.iter().all(|b| *b == 0));
    let mut session = active(&MODULES);
    let mut handler = ClosedResource {
        closed: Cell::new(true),
    };
    assert_eq!(
        session.dispatch_with_handler(request(0, 2, 2, &[]), &mut handler, &mut out),
        Err(SessionError::AuthorizationRevoked)
    );
    assert_eq!(session.state(), SessionState::Closed);
}
#[derive(Debug)]
struct DeferredHandler {
    ready: Cell<bool>,
    prepared: Cell<u32>,
    calls: u32,
}
impl ServiceHandler for DeferredHandler {
    fn response_ready(&mut self, _: Frame<'_>) -> Result<bool, SessionError> {
        self.prepared.set(self.prepared.get() + 1);
        Ok(self.ready.get())
    }
    fn required_features(&self, e: &ModuleEntry, op: u8) -> Option<u32> {
        (e.module == 4 && op == 0x11).then_some(0)
    }
    fn authorized(&self, _: Frame<'_>) -> bool {
        true
    }
    fn dispatch(&mut self, _: Frame<'_>, out: &mut [u8]) -> Result<usize, SessionError> {
        self.calls += 1;
        out[..4].fill(0);
        Ok(4)
    }
}
#[test]
fn test_should_hold_one_exact_request_until_nonblocking_response_gate_is_ready() {
    let mut s = active(&MODULES);
    let mut h = DeferredHandler {
        ready: Cell::new(false),
        prepared: Cell::new(0),
        calls: 0,
    };
    let mut out = [0xa5; 128];
    assert_eq!(
        s.dispatch_with_handler(request(4, 0x11, 2, &[7]), &mut h, &mut out),
        Err(SessionError::Deferred)
    );
    assert_eq!(h.calls, 0);
    assert!(out.iter().all(|b| *b == 0));
    h.ready.set(true);
    assert_eq!(
        s.dispatch_with_handler(request(4, 0x11, 2, &[7]), &mut h, &mut out),
        Ok(4)
    );
    assert_eq!(h.calls, 1);
    assert_eq!(h.prepared.get(), 2);
    assert_eq!(
        s.dispatch_with_handler(request(4, 0x11, 2, &[7]), &mut h, &mut out),
        Ok(4)
    );
    assert_eq!(h.calls, 1);
    assert_eq!(h.prepared.get(), 2);
}
#[test]
fn test_should_reject_any_other_identity_while_a_response_is_deferred() {
    let mut s = active(&MODULES);
    let mut h = DeferredHandler {
        ready: Cell::new(false),
        prepared: Cell::new(0),
        calls: 0,
    };
    let mut out = [0; 128];
    assert_eq!(
        s.dispatch_with_handler(request(4, 0x11, 2, &[7]), &mut h, &mut out),
        Err(SessionError::Deferred)
    );
    assert_eq!(
        s.dispatch_with_handler(request(4, 0x11, 3, &[7]), &mut h, &mut out),
        Err(SessionError::RequestConflict)
    );
    assert_eq!(s.state(), SessionState::Faulted);
    assert_eq!(h.calls, 0);
}
