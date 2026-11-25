#![cfg(feature = "self-test")]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use device_maintenance::{Authority, Capability, Profile, self_test::*};
use device_maintenance_service::*;
use device_protocol::{Frame, Header, Kind, ModuleEntry};
use device_services::ServiceHandler;
use std::cell::Cell;
struct Auth;
impl Authority for Auth {
    fn allows(&self, _: Capability) -> bool {
        true
    }
    fn station_id(&self) -> Option<u32> {
        Some(42)
    }
}
struct ClockSource;
impl Clock for ClockSource {
    fn now_ms(&self) -> Result<u64, Error> {
        Ok(1)
    }
}
struct Port<'a>(&'a Cell<u32>);
impl TestPort for Port<'_> {
    fn supports(&self, t: u16) -> bool {
        t == 1
    }
    fn start(&mut self, _: u16, _: u16) -> Result<Phase, device_maintenance::Error> {
        self.0.set(self.0.get() + 1);
        Ok(Phase::Running)
    }
    fn poll(
        &mut self,
        _: u16,
        out: &mut [Metric; 8],
    ) -> Result<Observation, device_maintenance::Error> {
        out[0] = Metric {
            id: 1,
            value: 5,
            scale_exp: 0,
            unit: 0,
        };
        Ok(Observation::Readings(1))
    }
    fn cancel(&mut self, _: u16) -> Result<(), device_maintenance::Error> {
        Ok(())
    }
}
struct Flags;
impl TestListFlags for Flags {
    fn flags(&self, _: u16) -> Result<u8, Error> {
        Ok(1)
    }
}
struct Verifier;
impl EvidenceVerifier for Verifier {
    fn verify(
        &self,
        _: &ExternalRequest,
        _: u16,
    ) -> Result<VerifiedEvidence, device_maintenance::Error> {
        Err(device_maintenance::Error::Denied)
    }
}
const ENTRY: ModuleEntry = ModuleEntry {
    module: 0x71,
    major: 1,
    minor: 1,
    availability: 1,
    features: 0,
    input_max: 128,
    output_max: 128,
};
fn req(op: u8, b: &[u8]) -> Frame<'_> {
    Frame {
        header: Header {
            kind: Kind::Request,
            module: 0x71,
            opcode: op,
            message_id: 2,
            offset: 0,
            total_length: u16::try_from(b.len()).unwrap(),
        },
        payload: b,
    }
}
#[test]
fn test_should_encode_exact_controlled_list_and_run_job_without_auto_pass() {
    let starts = Cell::new(0);
    let mut port = Port(&starts);
    let criteria = [MetricCriterion {
        id: 1,
        minimum: 0,
        maximum: 10,
        scale_exp: 0,
        unit: 0,
    }];
    let specs = [TestSpec {
        id: 1,
        criterion_rev: 3,
        method: Method::Automatic,
        criteria: &criteria,
        min_timeout_ms: 100,
        max_timeout_ms: 1000,
        dangerous: false,
        evidence_kinds: &[],
        allowed_origins: 0,
    }];
    let trace = CleanupTrace::new();
    let mut observed = SelfTestPortObserver::new(&mut port, &trace).unwrap();
    let mut h = SelfTestHandler::new(
        ENTRY,
        Profile::Development,
        &mut observed,
        &specs,
        &Auth,
        &ClockSource,
        &Flags,
        &Verifier,
    )
    .unwrap();
    let mut out = [0; 128];
    assert_eq!(h.dispatch(req(1, &[0, 8]), &mut out), Ok(10));
    assert_eq!(&out[..10], &[0, 0, 0, 0, 255, 1, 1, 0, 0, 1]);
    assert_eq!(
        h.dispatch(req(2, &[1, 0, 0, 0, 0xe8, 3, 0, 0]), &mut out),
        Ok(8)
    );
    assert_eq!(&out[..8], &[0, 0, 0, 0, 1, 0, 0, 0]);
    assert_eq!(starts.get(), 1);
}
#[test]
fn test_should_encode_exact_manifest_checked_result_only_after_run_admission() {
    let starts = Cell::new(0);
    let mut port = Port(&starts);
    let criteria = [MetricCriterion {
        id: 1,
        minimum: 0,
        maximum: 10,
        scale_exp: 0,
        unit: 0,
    }];
    let specs = [TestSpec {
        id: 1,
        criterion_rev: 3,
        method: Method::Automatic,
        criteria: &criteria,
        min_timeout_ms: 100,
        max_timeout_ms: 1000,
        dangerous: false,
        evidence_kinds: &[],
        allowed_origins: 0,
    }];
    let trace = CleanupTrace::new();
    let mut observed = SelfTestPortObserver::new(&mut port, &trace).unwrap();
    let mut h = SelfTestHandler::new(
        ENTRY,
        Profile::Factory,
        &mut observed,
        &specs,
        &Auth,
        &ClockSource,
        &Flags,
        &Verifier,
    )
    .unwrap();
    let mut out = [0; 128];
    let run = req(2, &[1, 0, 0, 0, 0xe8, 3, 0, 0]);
    h.dispatch(run, &mut out).unwrap();
    assert_eq!(h.dispatch(req(3, &[1, 0, 0, 0]), &mut out), Ok(4));
    assert_eq!(out[0], 4);
    h.response_admitted(run, &[0, 0, 0, 0, 1, 0, 0, 0]).unwrap();
    h.poll().unwrap();
    assert_eq!(h.dispatch(req(3, &[1, 0, 0, 0]), &mut out), Ok(41));
    assert_eq!(&out[..12], &[0, 0, 0, 0, 2, 0, 0, 1, 0, 3, 0, 0]);
    assert_eq!(out[32], 1);
    assert_eq!(&out[33..41], &[1, 0, 5, 0, 0, 0, 0, 0]);
}
struct DynamicAuth(Cell<bool>);
impl Authority for DynamicAuth {
    fn allows(&self, c: Capability) -> bool {
        c != Capability::DangerousSelfTest || self.0.get()
    }
    fn station_id(&self) -> Option<u32> {
        Some(42)
    }
}
#[test]
fn test_should_keep_current_dangerous_job_guard_when_old_terminal_cancel_is_retried() {
    let starts = Cell::new(0);
    let mut port = Port(&starts);
    let criteria = [MetricCriterion {
        id: 1,
        minimum: 0,
        maximum: 10,
        scale_exp: 0,
        unit: 0,
    }];
    let specs = [TestSpec {
        id: 1,
        criterion_rev: 3,
        method: Method::Automatic,
        criteria: &criteria,
        min_timeout_ms: 100,
        max_timeout_ms: 1000,
        dangerous: true,
        evidence_kinds: &[],
        allowed_origins: 0,
    }];
    let auth = DynamicAuth(Cell::new(true));
    let trace = CleanupTrace::new();
    let mut observed = SelfTestPortObserver::new(&mut port, &trace).unwrap();
    let mut h = SelfTestHandler::new(
        ENTRY,
        Profile::Factory,
        &mut observed,
        &specs,
        &auth,
        &ClockSource,
        &Flags,
        &Verifier,
    )
    .unwrap();
    let mut out = [0; 128];
    let run = req(2, &[1, 0, 0, 0, 0xe8, 3, 0, 0]);
    h.dispatch(run, &mut out).unwrap();
    h.response_admitted(run, &[0, 0, 0, 0, 1, 0, 0, 0]).unwrap();
    h.dispatch(req(4, &[1, 0, 0, 0]), &mut out).unwrap();
    h.dispatch(run, &mut out).unwrap();
    h.response_admitted(run, &[0, 0, 0, 0, 2, 0, 0, 0]).unwrap();
    h.dispatch(req(4, &[1, 0, 0, 0]), &mut out).unwrap();
    auth.0.set(false);
    assert!(h.must_end_session());
}
struct FailedCancel;
impl TestPort for FailedCancel {
    fn supports(&self, _: u16) -> bool {
        true
    }
    fn start(&mut self, _: u16, _: u16) -> Result<Phase, device_maintenance::Error> {
        Ok(Phase::Running)
    }
    fn poll(
        &mut self,
        _: u16,
        _: &mut [Metric; 8],
    ) -> Result<Observation, device_maintenance::Error> {
        Ok(Observation::Pending)
    }
    fn cancel(&mut self, _: u16) -> Result<(), device_maintenance::Error> {
        Err(device_maintenance::Error::Backend)
    }
}
#[test]
fn test_should_not_turn_failed_physical_cleanup_into_success_on_repeated_close() {
    let mut port = FailedCancel;
    let criteria = [MetricCriterion {
        id: 1,
        minimum: 0,
        maximum: 10,
        scale_exp: 0,
        unit: 0,
    }];
    let specs = [TestSpec {
        id: 1,
        criterion_rev: 3,
        method: Method::Automatic,
        criteria: &criteria,
        min_timeout_ms: 100,
        max_timeout_ms: 1000,
        dangerous: false,
        evidence_kinds: &[],
        allowed_origins: 0,
    }];
    let trace = CleanupTrace::new();
    let mut observed = SelfTestPortObserver::new(&mut port, &trace).unwrap();
    let mut h = SelfTestHandler::new(
        ENTRY,
        Profile::Factory,
        &mut observed,
        &specs,
        &Auth,
        &ClockSource,
        &Flags,
        &Verifier,
    )
    .unwrap();
    h.dispatch(req(2, &[1, 0, 0, 0, 0xe8, 3, 0, 0]), &mut [0; 128])
        .unwrap();
    assert!(h.connection_closed().is_err());
    assert!(h.connection_closed().is_err());
}
struct ManualPort;
impl TestPort for ManualPort {
    fn supports(&self, t: u16) -> bool {
        t == 1
    }
    fn start(&mut self, _: u16, _: u16) -> Result<Phase, device_maintenance::Error> {
        Ok(Phase::ManualReady)
    }
    fn poll(
        &mut self,
        _: u16,
        _: &mut [Metric; 8],
    ) -> Result<Observation, device_maintenance::Error> {
        Ok(Observation::Pending)
    }
    fn cancel(&mut self, _: u16) -> Result<(), device_maintenance::Error> {
        Ok(())
    }
}
struct Proof;
impl EvidenceVerifier for Proof {
    fn verify(
        &self,
        r: &ExternalRequest,
        rev: u16,
    ) -> Result<VerifiedEvidence, device_maintenance::Error> {
        if r.job != 1 || r.test != 1 || r.station_id != 42 || rev != 3 || r.evidence != [9; 16] {
            return Err(device_maintenance::Error::Denied);
        }
        Ok(VerifiedEvidence {
            station_id: 42,
            origin: Origin::Human,
            kind: 9,
            verdict: Verdict::Pass,
        })
    }
}
#[test]
fn test_should_bind_external_verdict_to_actual_manifest_and_independent_evidence() {
    let mut port = ManualPort;
    let specs = [TestSpec {
        id: 1,
        criterion_rev: 3,
        method: Method::Manual,
        criteria: &[],
        min_timeout_ms: 100,
        max_timeout_ms: 1000,
        dangerous: false,
        evidence_kinds: &[9],
        allowed_origins: 2,
    }];
    let trace = CleanupTrace::new();
    let mut observed = SelfTestPortObserver::new(&mut port, &trace).unwrap();
    let mut h = SelfTestHandler::new(
        ENTRY,
        Profile::Factory,
        &mut observed,
        &specs,
        &Auth,
        &ClockSource,
        &Flags,
        &Proof,
    )
    .unwrap();
    let mut out = [0; 128];
    let run = req(2, &[1, 0, 0, 0, 0xe8, 3, 0, 0]);
    h.dispatch(run, &mut out).unwrap();
    h.response_admitted(run, &[0, 0, 0, 0, 1, 0, 0, 0]).unwrap();
    let mut external = [9; 27];
    external[..11].copy_from_slice(&[1, 0, 0, 0, 1, 0, 1, 42, 0, 0, 0]);
    assert_eq!(h.dispatch(req(5, &external), &mut out), Ok(4));
    assert_eq!(&out[..4], &[0, 0, 0, 0]);
    assert_eq!(h.dispatch(req(3, &[1, 0, 0, 0]), &mut out), Ok(33));
    assert_eq!(out[4], 2);
    assert_eq!(out[11], 1);
    assert_eq!(&out[12..16], &[42, 0, 0, 0]);
    assert_eq!(&out[16..32], &[9; 16]);
    assert_eq!(out[32], 0);
}
struct MovingClock(Cell<u64>);
impl Clock for MovingClock {
    fn now_ms(&self) -> Result<u64, Error> {
        Ok(self.0.get())
    }
}
#[test]
fn test_should_preserve_timeout_cleanup_failure_and_close_wire_actor() {
    let mut port = FailedCancel;
    let criteria = [MetricCriterion {
        id: 1,
        minimum: 0,
        maximum: 10,
        scale_exp: 0,
        unit: 0,
    }];
    let specs = [TestSpec {
        id: 1,
        criterion_rev: 3,
        method: Method::Automatic,
        criteria: &criteria,
        min_timeout_ms: 100,
        max_timeout_ms: 1000,
        dangerous: false,
        evidence_kinds: &[],
        allowed_origins: 0,
    }];
    let clock = MovingClock(Cell::new(1));
    let trace = CleanupTrace::new();
    let mut observed = SelfTestPortObserver::new(&mut port, &trace).unwrap();
    let mut h = SelfTestHandler::new(
        ENTRY,
        Profile::Factory,
        &mut observed,
        &specs,
        &Auth,
        &clock,
        &Flags,
        &Verifier,
    )
    .unwrap();
    h.dispatch(req(2, &[1, 0, 0, 0, 0xe8, 3, 0, 0]), &mut [0; 128])
        .unwrap();
    clock.0.set(1002);
    assert!(h.poll().is_err());
    assert!(h.must_end_session());
    assert!(h.connection_closed().is_err());
    assert!(h.connection_closed().is_err());
}

#[test]
fn test_should_latch_timeout_cleanup_failure_when_result_query_advances_engine() {
    let mut port = FailedCancel;
    let criteria = [MetricCriterion {
        id: 1,
        minimum: 0,
        maximum: 10,
        scale_exp: 0,
        unit: 0,
    }];
    let specs = [TestSpec {
        id: 1,
        criterion_rev: 3,
        method: Method::Automatic,
        criteria: &criteria,
        min_timeout_ms: 100,
        max_timeout_ms: 1000,
        dangerous: false,
        evidence_kinds: &[],
        allowed_origins: 0,
    }];
    let clock = MovingClock(Cell::new(1));
    let trace = CleanupTrace::new();
    let mut observed = SelfTestPortObserver::new(&mut port, &trace).unwrap();
    let mut h = SelfTestHandler::new(
        ENTRY,
        Profile::Factory,
        &mut observed,
        &specs,
        &Auth,
        &clock,
        &Flags,
        &Verifier,
    )
    .unwrap();
    let mut out = [0; 128];
    let run = req(2, &[1, 0, 0, 0, 0xe8, 3, 0, 0]);
    h.dispatch(run, &mut out).unwrap();
    h.response_admitted(run, &out[..8]).unwrap();
    clock.0.set(1002);
    let result = h.dispatch(req(3, &[1, 0, 0, 0]), &mut out);
    assert_eq!(
        result,
        Err(device_services::SessionError::AuthorizationRevoked)
    );
    let ended = h.must_end_session();
    let cleanup = h.connection_closed();
    assert!(ended);
    assert!(cleanup.is_err());
    assert!(h.connection_closed().is_err());
}

#[test]
fn test_should_latch_cleanup_failure_from_every_engine_deadline_entry() {
    for operation in [2, 3, 4, 5, 6] {
        let mut port = FailedCancel;
        let criteria = [MetricCriterion {
            id: 1,
            minimum: 0,
            maximum: 10,
            scale_exp: 0,
            unit: 0,
        }];
        let specs = [TestSpec {
            id: 1,
            criterion_rev: 3,
            method: Method::Automatic,
            criteria: &criteria,
            min_timeout_ms: 100,
            max_timeout_ms: 1000,
            dangerous: false,
            evidence_kinds: &[],
            allowed_origins: 0,
        }];
        let clock = MovingClock(Cell::new(1));
        let trace = CleanupTrace::new();
        let mut observed = SelfTestPortObserver::new(&mut port, &trace).unwrap();
        let mut h = SelfTestHandler::new(
            ENTRY,
            Profile::Factory,
            &mut observed,
            &specs,
            &Auth,
            &clock,
            &Flags,
            &Verifier,
        )
        .unwrap();
        let mut out = [0; 128];
        let run = req(2, &[1, 0, 0, 0, 0xe8, 3, 0, 0]);
        h.dispatch(run, &mut out).unwrap();
        h.response_admitted(run, &out[..8]).unwrap();
        clock.0.set(1002);
        match operation {
            2 => {
                let _ = h.dispatch(run, &mut out);
            }
            3 | 4 => {
                let _ = h.dispatch(req(operation, &[1, 0, 0, 0]), &mut out);
            }
            5 => {
                let mut external = [9; 27];
                external[..11].copy_from_slice(&[1, 0, 0, 0, 1, 0, 1, 42, 0, 0, 0]);
                let _ = h.dispatch(req(5, &external), &mut out);
            }
            _ => {
                assert!(!h.replay_valid(run, &out[..8]));
            }
        }
        assert!(h.must_end_session(), "deadline entry {operation}");
        assert!(h.connection_closed().is_err(), "deadline entry {operation}");
        assert!(h.connection_closed().is_err(), "deadline entry {operation}");
    }
}

#[test]
fn test_should_refuse_a_new_selftest_view_after_the_real_port_cleanup_failed() {
    let mut port = FailedCancel;
    let criteria = [MetricCriterion {
        id: 1,
        minimum: 0,
        maximum: 10,
        scale_exp: 0,
        unit: 0,
    }];
    let specs = [TestSpec {
        id: 1,
        criterion_rev: 3,
        method: Method::Automatic,
        criteria: &criteria,
        min_timeout_ms: 100,
        max_timeout_ms: 1000,
        dangerous: false,
        evidence_kinds: &[],
        allowed_origins: 0,
    }];
    let trace = CleanupTrace::new();
    let mut observed = SelfTestPortObserver::new(&mut port, &trace).unwrap();
    {
        let mut handler = SelfTestHandler::new(
            ENTRY,
            Profile::Factory,
            &mut observed,
            &specs,
            &Auth,
            &ClockSource,
            &Flags,
            &Verifier,
        )
        .unwrap();
        handler
            .dispatch(req(2, &[1, 0, 0, 0, 0xe8, 3, 0, 0]), &mut [0; 128])
            .unwrap();
        assert!(handler.connection_closed().is_err());
    }
    assert!(
        SelfTestHandler::new(
            ENTRY,
            Profile::Factory,
            &mut observed,
            &specs,
            &Auth,
            &ClockSource,
            &Flags,
            &Verifier
        )
        .is_err()
    );
}
