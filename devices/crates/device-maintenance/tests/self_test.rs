//! Real metrics, controlled criteria and verified station evidence tests.
#![cfg(feature = "self-test")]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use device_maintenance::{Authority, Capability, Error, Profile, self_test::*};
use std::cell::Cell;
struct Auth(Cell<bool>);
impl Authority for Auth {
    fn allows(&self, _: Capability) -> bool {
        self.0.get()
    }
    fn station_id(&self) -> Option<u32> {
        Some(42)
    }
}
struct Port<'a> {
    value: i32,
    manual: bool,
    cancels: &'a Cell<u32>,
    count: usize,
}
impl TestPort for Port<'_> {
    fn supports(&self, test: u16) -> bool {
        test == 1
    }
    fn start(&mut self, _: u16, _: u16) -> Result<Phase, Error> {
        Ok(if self.manual {
            Phase::ManualReady
        } else {
            Phase::Running
        })
    }
    fn poll(&mut self, _: u16, out: &mut [Metric; 8]) -> Result<Observation, Error> {
        out[0] = Metric {
            id: 1,
            value: self.value,
            scale_exp: 0,
            unit: 0,
        };
        Ok(Observation::Readings(self.count))
    }
    fn cancel(&mut self, _: u16) -> Result<(), Error> {
        self.cancels.set(self.cancels.get() + 1);
        Ok(())
    }
}
fn criterion() -> [MetricCriterion; 1] {
    [MetricCriterion {
        id: 1,
        minimum: 10,
        maximum: 20,
        scale_exp: 0,
        unit: 0,
    }]
}
fn spec(c: &[MetricCriterion]) -> TestSpec<'_> {
    TestSpec {
        id: 1,
        criterion_rev: 3,
        method: Method::Automatic,
        criteria: c,
        min_timeout_ms: 100,
        max_timeout_ms: 1000,
        dangerous: false,
        evidence_kinds: &[],
        allowed_origins: 0,
    }
}
fn port(c: &Cell<u32>) -> Port<'_> {
    Port {
        value: 15,
        manual: false,
        cancels: c,
        count: 1,
    }
}
struct Verify {
    valid: bool,
    origin: Origin,
    verdict: Verdict,
}
impl EvidenceVerifier for Verify {
    fn verify(&self, _: &ExternalRequest, _: u16) -> Result<VerifiedEvidence, Error> {
        if !self.valid {
            return Err(Error::Denied);
        }
        Ok(VerifiedEvidence {
            station_id: 42,
            origin: self.origin,
            kind: 9,
            verdict: self.verdict,
        })
    }
}
fn request(job: u32) -> ExternalRequest {
    ExternalRequest {
        job,
        test: 1,
        result: Verdict::Pass,
        station_id: 42,
        evidence: [7; 16],
    }
}
#[test]
fn real_metrics_need_manifest_criteria_and_preserve_bound_version() {
    let c = criterion();
    let specs = [spec(&c)];
    let cancelled = Cell::new(0);
    let mut p = port(&cancelled);
    let auth = Auth(Cell::new(true));
    let mut e = SelfTestEngine::new(Profile::Development, Some(&mut p), &specs, &auth).unwrap();
    let job = e.run(1, 0, 500, 100).unwrap();
    assert_eq!(job, 1);
    assert_eq!(e.result(job, 100).unwrap().state, JobState::Running);
    e.poll(101).unwrap();
    let result = e.result(job, 101).unwrap();
    assert_eq!(result.state, JobState::Passed);
    assert_eq!(result.criterion_rev, 3);
    assert_eq!(result.origin, Origin::Automatic);
    assert_eq!(
        result.values(),
        &[Metric {
            id: 1,
            value: 15,
            scale_exp: 0,
            unit: 0
        }]
    );
    assert_eq!(result.evidence, [0; 16]);
    assert_eq!(e.cancel(job, 102), Ok(()));
    assert_eq!(e.result(job, 102).unwrap(), result);
}
#[test]
fn missing_thresholds_backend_and_commercial_profile_never_auto_pass() {
    let c = criterion();
    let specs = [spec(&c)];
    let cancelled = Cell::new(0);
    let mut p = port(&cancelled);
    let auth = Auth(Cell::new(true));
    assert_eq!(
        SelfTestEngine::new(Profile::Commercial, Some(&mut p), &specs, &auth).err(),
        Some(Error::Commercial)
    );
    assert_eq!(
        SelfTestEngine::new(Profile::Factory, None, &specs, &auth).err(),
        Some(Error::Unsupported)
    );
    let bad = [spec(&[])];
    assert_eq!(
        SelfTestEngine::new(Profile::Factory, Some(&mut p), &bad, &auth).err(),
        Some(Error::CriterionUndefined)
    );
}
#[test]
fn failed_metrics_and_missing_valid_samples_are_not_pass() {
    for (value, count, want) in [
        (99, 1, JobState::Failed),
        (15, 0, JobState::Unsupported),
        (15, 9, JobState::Failed),
    ] {
        let c = criterion();
        let specs = [spec(&c)];
        let cancelled = Cell::new(0);
        let mut p = port(&cancelled);
        p.value = value;
        p.count = count;
        let auth = Auth(Cell::new(true));
        let mut e = SelfTestEngine::new(Profile::Factory, Some(&mut p), &specs, &auth).unwrap();
        let job = e.run(1, 0, 500, 0).unwrap();
        let _ = e.poll(1);
        assert_eq!(e.result(job, 1).unwrap().state, want);
    }
}
#[test]
fn timeout_cancel_and_terminal_retention_are_idempotent_and_ids_never_reused() {
    let c = criterion();
    let specs = [spec(&c)];
    let cancelled = Cell::new(0);
    let mut p = port(&cancelled);
    let auth = Auth(Cell::new(true));
    let mut e = SelfTestEngine::new(Profile::Factory, Some(&mut p), &specs, &auth).unwrap();
    let first = e.run(1, 0, 100, 0).unwrap();
    assert_eq!(e.run(1, 0, 100, 0), Err(Error::Busy));
    e.poll(100).unwrap();
    assert_eq!(e.result(first, 100).unwrap().state, JobState::TimedOut);
    e.cancel(first, 100).unwrap();
    assert_eq!(cancelled.get(), 1);
    for expected in 2..=10 {
        let job = e.run(1, 0, 100, 100).unwrap();
        assert_eq!(job, expected);
        e.cancel(job, 100).unwrap();
        assert_eq!(e.result(job, 100).unwrap().state, JobState::Cancelled);
    }
    assert_eq!(e.result(first, 100).err(), Some(Error::NotFound));
    assert_eq!(e.result(2, 100).err(), Some(Error::NotFound));
    assert_eq!(e.result(3, 100).unwrap().state, JobState::Cancelled);
}
#[test]
fn manual_observation_never_passes_without_verified_station_evidence() {
    let specs = [TestSpec {
        method: Method::Manual,
        criteria: &[],
        evidence_kinds: &[9],
        allowed_origins: 2,
        ..spec(&[])
    }];
    let cancelled = Cell::new(0);
    let mut p = port(&cancelled);
    p.manual = true;
    let auth = Auth(Cell::new(true));
    let mut e = SelfTestEngine::new(Profile::Factory, Some(&mut p), &specs, &auth).unwrap();
    let job = e.run(1, 0, 500, 0).unwrap();
    assert_eq!(e.result(job, 0).unwrap().state, JobState::WaitManual);
    let invalid = Verify {
        valid: false,
        origin: Origin::Human,
        verdict: Verdict::Pass,
    };
    assert_eq!(
        e.external_result(request(job), &invalid, 1),
        Err(Error::Denied)
    );
    assert_eq!(e.result(job, 1).unwrap().state, JobState::WaitManual);
    let valid = Verify {
        valid: true,
        origin: Origin::Human,
        verdict: Verdict::Pass,
    };
    e.external_result(request(job), &valid, 2).unwrap();
    let result = e.result(job, 2).unwrap();
    assert_eq!(
        (result.state, result.origin, result.station_id),
        (JobState::Passed, Origin::Human, 42)
    );
    assert_eq!(result.evidence, [7; 16]);
    assert_eq!(e.external_result(request(job), &valid, 3), Ok(()));
    assert_eq!(
        e.external_result(
            ExternalRequest {
                evidence: [8; 16],
                ..request(job)
            },
            &valid,
            3
        ),
        Err(Error::Conflict)
    );
}
#[test]
fn phone_verdict_station_and_origin_claims_cannot_replace_evidence() {
    let specs = [TestSpec {
        method: Method::Manual,
        criteria: &[],
        evidence_kinds: &[9],
        allowed_origins: 2,
        ..spec(&[])
    }];
    let cancelled = Cell::new(0);
    let mut p = port(&cancelled);
    p.manual = true;
    let auth = Auth(Cell::new(true));
    let mut e = SelfTestEngine::new(Profile::Factory, Some(&mut p), &specs, &auth).unwrap();
    let job = e.run(1, 0, 500, 0).unwrap();
    let verifier = Verify {
        valid: true,
        origin: Origin::Human,
        verdict: Verdict::Fail,
    };
    assert_eq!(
        e.external_result(request(job), &verifier, 1),
        Err(Error::Conflict)
    );
    let verifier = Verify {
        valid: true,
        origin: Origin::Instrument,
        verdict: Verdict::Pass,
    };
    assert_eq!(
        e.external_result(request(job), &verifier, 1),
        Err(Error::Denied)
    );
    let verifier = Verify {
        valid: true,
        origin: Origin::Human,
        verdict: Verdict::Pass,
    };
    assert_eq!(
        e.external_result(
            ExternalRequest {
                station_id: 99,
                ..request(job)
            },
            &verifier,
            1
        ),
        Err(Error::Denied)
    );
    assert_eq!(
        e.external_result(
            ExternalRequest {
                evidence: [0; 16],
                ..request(job)
            },
            &verifier,
            1
        ),
        Err(Error::Invalid)
    );
    assert_eq!(e.result(job, 1).unwrap().state, JobState::WaitManual);
}
#[test]
fn cancelled_and_timed_out_manual_jobs_cannot_be_overwritten() {
    let specs = [TestSpec {
        method: Method::Manual,
        criteria: &[],
        evidence_kinds: &[9],
        allowed_origins: 2,
        ..spec(&[])
    }];
    let cancelled = Cell::new(0);
    let mut p = port(&cancelled);
    p.manual = true;
    let auth = Auth(Cell::new(true));
    let mut e = SelfTestEngine::new(Profile::Factory, Some(&mut p), &specs, &auth).unwrap();
    let valid = Verify {
        valid: true,
        origin: Origin::Human,
        verdict: Verdict::Pass,
    };
    let first = e.run(1, 0, 100, 0).unwrap();
    e.cancel(first, 1).unwrap();
    assert_eq!(
        e.external_result(request(first), &valid, 2),
        Err(Error::InvalidState)
    );
    let second = e.run(1, 0, 100, 2).unwrap();
    assert_eq!(
        e.external_result(request(second), &valid, 102),
        Err(Error::InvalidState)
    );
    assert_eq!(e.result(second, 102).unwrap().state, JobState::TimedOut);
}
#[test]
fn revocation_clears_cached_sensitive_results_and_stops_active_job() {
    let c = criterion();
    let specs = [spec(&c)];
    let cancelled = Cell::new(0);
    let mut p = port(&cancelled);
    let auth = Auth(Cell::new(true));
    let mut e = SelfTestEngine::new(Profile::Factory, Some(&mut p), &specs, &auth).unwrap();
    let job = e.run(1, 0, 500, 0).unwrap();
    auth.0.set(false);
    assert_eq!(e.result(job, 1).err(), Some(Error::Denied));
    assert_eq!(cancelled.get(), 1);
    auth.0.set(true);
    assert_eq!(e.result(job, 2).err(), Some(Error::Closed));
}
#[test]
fn debug_never_dumps_metric_values_or_evidence() {
    let c = criterion();
    let specs = [spec(&c)];
    let cancelled = Cell::new(0);
    let mut p = port(&cancelled);
    let auth = Auth(Cell::new(true));
    let mut e = SelfTestEngine::new(Profile::Factory, Some(&mut p), &specs, &auth).unwrap();
    let job = e.run(1, 0, 500, 0).unwrap();
    e.poll(1).unwrap();
    let result = e.result(job, 1).unwrap();
    assert!(!format!("{result:?}").contains("value: 15"));
    assert!(!format!("{e:?}").contains("values:"));
    assert!(!format!("{:?}", request(job)).contains("[7, 7"));
}

#[test]
fn hybrid_external_pass_cannot_bypass_required_automatic_metrics() {
    let criteria = criterion();
    let specs = [TestSpec {
        method: Method::Hybrid,
        evidence_kinds: &[9],
        allowed_origins: 2,
        ..spec(&criteria)
    }];
    let cancelled = Cell::new(0);
    let mut p = port(&cancelled);
    p.manual = true;
    let auth = Auth(Cell::new(true));
    let mut e = SelfTestEngine::new(Profile::Factory, Some(&mut p), &specs, &auth).unwrap();
    let job = e.run(1, 0, 500, 0).unwrap();
    let proof = Verify {
        valid: true,
        origin: Origin::Human,
        verdict: Verdict::Pass,
    };
    assert_eq!(
        e.external_result(request(job), &proof, 1),
        Err(Error::InvalidState)
    );
    e.poll(2).unwrap();
    assert_eq!(e.result(job, 2).unwrap().state, JobState::WaitManual);
    e.external_result(request(job), &proof, 3).unwrap();
    assert_eq!(e.result(job, 3).unwrap().state, JobState::Passed);
}
