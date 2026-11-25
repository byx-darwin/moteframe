#![cfg(feature = "heart-rate")]
use device_health::{
    Backend, BackendError, Error, HealthStart, JobId, JobManager, JobState, Module, Outcome, Owner,
    Reason, ResourceSet, ResultInfo, ResultSource, StopProgress, Timestamp, TriggerSource,
};

// Orthogonal test fault switches; this is not a production lifecycle representation.
#[allow(clippy::struct_excessive_bools)]
struct Fixture {
    available: bool,
    fault_start: bool,
    fault_stop: bool,
    stopping: bool,
    resources: ResourceSet,
    starts: usize,
    stops: usize,
    acknowledgements: usize,
    context: u32,
    completion: Option<Outcome>,
    sample: Option<ResultInfo>,
    data: [u8; 4],
}
impl Default for Fixture {
    fn default() -> Self {
        Self {
            available: true,
            fault_start: false,
            fault_stop: false,
            stopping: false,
            resources: ResourceSet::from_bits(1),
            starts: 0,
            stops: 0,
            acknowledgements: 0,
            context: 0,
            completion: None,
            sample: None,
            data: [60, 0, 100, 1],
        }
    }
}
impl Backend for Fixture {
    fn has_pending_result(&self, _: JobId) -> Result<bool, BackendError> {
        Ok(self.sample.is_some())
    }
    fn resources(&self, _: HealthStart) -> Result<ResourceSet, BackendError> {
        if self.available {
            Ok(self.resources)
        } else {
            Err(BackendError::Unavailable)
        }
    }
    fn context_is_valid(&self, id: u32) -> bool {
        id == self.context
    }
    fn start(&mut self, _: JobId, _: HealthStart) -> Result<(), BackendError> {
        self.starts += 1;
        if self.fault_start {
            Err(BackendError::Fault)
        } else {
            Ok(())
        }
    }
    fn stop(&mut self, _: JobId) -> Result<StopProgress, BackendError> {
        self.stops += 1;
        if self.fault_stop {
            return Err(BackendError::Fault);
        }
        Ok(if self.stopping {
            StopProgress::Stopping
        } else {
            StopProgress::Stopped
        })
    }
    fn completion(&mut self, _: JobId) -> Result<Option<Outcome>, BackendError> {
        Ok(self.completion)
    }
    fn result(&mut self, _: JobId, output: &mut [u8]) -> Result<Option<ResultInfo>, BackendError> {
        if let Some(info) = self.sample {
            if output.len() < self.data.len() {
                return Err(BackendError::Capacity);
            }
            output[..4].copy_from_slice(&self.data);
            Ok(Some(info))
        } else {
            Ok(None)
        }
    }
    fn acknowledge_result(&mut self, _: JobId, _: u64) -> Result<(), BackendError> {
        self.acknowledgements += 1;
        self.sample = None;
        Ok(())
    }
}
fn hr() -> Module {
    Module::from_wire(0x20).unwrap_or_else(|e| panic!("{e:?}"))
}
fn start() -> HealthStart {
    HealthStart::decode(0x20, &[1, 0x20, 0, 0, 0, 0, 0, 0]).unwrap_or_else(|e| panic!("{e:?}"))
}
fn manager<const N: usize>(first: u32) -> JobManager<N> {
    JobManager::new(first).unwrap_or_else(|e| panic!("{e:?}"))
}
fn begin<const N: usize>(m: &mut JobManager<N>, b: &mut Fixture) -> JobId {
    m.start(start(), Owner::Application, TriggerSource::App, b)
        .unwrap_or_else(|e| panic!("{e:?}"))
}
fn sample(sequence: u64) -> ResultInfo {
    ResultInfo {
        sample_id: sequence,
        record_type: 0x20,
        schema: 1,
        timestamp: Timestamp::unknown(),
        length: 4,
        source: ResultSource::DeviceAlgorithm,
    }
}
#[test]
fn test_should_not_allocate_a_successful_job_without_real_backend() {
    let mut m = manager::<2>(1);
    let mut b = Fixture {
        available: false,
        ..Fixture::default()
    };
    assert_eq!(
        m.start(start(), Owner::Application, TriggerSource::App, &mut b),
        Err(Error::Unavailable)
    );
    assert_eq!(b.starts, 0);
    b.available = true;
    let job = begin(&mut m, &mut b);
    assert_eq!(job.get(), 1);
    assert_eq!(m.query(hr(), job).map(|j| j.state), Ok(JobState::Accepted));
}
#[test]
fn test_should_validate_context_before_backend_start_and_keep_ids_monotonic() {
    let mut m = manager::<1>(7);
    let mut b = Fixture::default();
    let request =
        HealthStart::decode(0x20, &[1, 0x20, 0, 9, 0, 0, 0, 0]).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        m.start(request, Owner::Application, TriggerSource::Sport, &mut b),
        Err(Error::InvalidField)
    );
    assert_eq!(b.starts, 0);
    b.context = 9;
    let a = m
        .start(request, Owner::Application, TriggerSource::Sport, &mut b)
        .unwrap_or_else(|e| panic!("{e:?}"));
    m.confirm_start_response(hr(), a)
        .unwrap_or_else(|e| panic!("{e:?}"));
    m.stop(hr(), a, &mut b).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        m.start(start(), Owner::Application, TriggerSource::App, &mut b),
        Err(Error::Capacity)
    );
    m.confirm_end_event(hr(), a)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(begin(&mut m, &mut b).get(), 8);
    assert_eq!(m.query(hr(), a).map(|j| j.state), Ok(JobState::Cancelled));
}
#[test]
fn test_should_hold_resources_until_real_stop_and_bind_jobs_to_module() {
    let mut m = manager::<3>(1);
    let mut b = Fixture {
        stopping: true,
        ..Fixture::default()
    };
    let a = begin(&mut m, &mut b);
    assert_eq!(
        m.start(start(), Owner::Application, TriggerSource::App, &mut b),
        Err(Error::Busy)
    );
    m.stop(hr(), a, &mut b).unwrap_or_else(|e| panic!("{e:?}"));
    m.stop(hr(), a, &mut b).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(b.stops, 1);
    assert_eq!(
        m.start(start(), Owner::Application, TriggerSource::App, &mut b),
        Err(Error::Busy)
    );
    b.completion = Some(Outcome::Cancelled(Reason::UserCancelled));
    assert_eq!(m.poll_completion(hr(), a, &mut b), Ok(JobState::Cancelled));
    assert!(
        m.start(start(), Owner::Application, TriggerSource::App, &mut b)
            .is_ok()
    );
    let unknown = JobId::new(200).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(m.stop(hr(), unknown, &mut b), Err(Error::NotFound));
    #[cfg(feature = "blood-pressure")]
    assert_eq!(
        m.query(
            Module::from_wire(0x25).unwrap_or_else(|e| panic!("{e:?}")),
            a
        ),
        Err(Error::NotFound)
    );
}
#[test]
fn test_should_gate_all_events_on_start_response_and_ack_one_terminal_event() {
    let mut m = manager::<1>(1);
    let mut b = Fixture::default();
    let a = begin(&mut m, &mut b);
    m.stop(hr(), a, &mut b).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(m.pending_end(hr(), a), Ok(None));
    assert_eq!(m.confirm_end_event(hr(), a), Err(Error::InvalidState));
    m.confirm_start_response(hr(), a)
        .unwrap_or_else(|e| panic!("{e:?}"));
    let end = m.pending_end(hr(), a).unwrap_or_else(|e| panic!("{e:?}"));
    assert!(end.is_some());
    assert_eq!(m.pending_end(hr(), a), Ok(end));
    m.confirm_end_event(hr(), a)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(m.pending_end(hr(), a), Ok(None));
    m.stop(hr(), a, &mut b).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(b.stops, 1);
    assert_eq!(m.pending_end(hr(), a), Ok(None));
}
#[test]
fn test_should_never_reuse_job_ids_or_return_job_after_failed_start() {
    let mut m = manager::<2>(u32::MAX);
    let mut b = Fixture::default();
    assert_eq!(begin(&mut m, &mut b).get(), u32::MAX);
    b.resources = ResourceSet::from_bits(0);
    assert_eq!(
        m.start(start(), Owner::Application, TriggerSource::App, &mut b),
        Err(Error::Exhausted)
    );
    assert!(JobManager::<1>::new(0).is_err());
    let mut m = manager::<2>(5);
    b.fault_start = true;
    assert_eq!(
        m.start(start(), Owner::Application, TriggerSource::App, &mut b),
        Err(Error::Backend)
    );
    b.fault_start = false;
    assert_eq!(begin(&mut m, &mut b).get(), 6);
}
#[test]
fn test_should_retry_real_result_without_retransmission_count_or_fake_zero() {
    let mut m = manager::<1>(1);
    let mut b = Fixture::default();
    let a = begin(&mut m, &mut b);
    let mut output = [0; 8];
    b.sample = Some(sample(1));
    assert_eq!(
        m.fetch_result(hr(), a, &mut b, &mut output),
        Err(Error::InvalidState)
    );
    m.confirm_start_response(hr(), a)
        .unwrap_or_else(|e| panic!("{e:?}"));
    let value = m
        .fetch_result(hr(), a, &mut b, &mut output)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        value.map(|v| v.latest.payload().to_vec()),
        Some(vec![60, 0, 100, 1])
    );
    assert!(
        m.fetch_result(hr(), a, &mut b, &mut output)
            .unwrap_or_else(|e| panic!("{e:?}"))
            .is_some()
    );
    assert_eq!(b.acknowledgements, 0);
    m.confirm_result(hr(), a, 1, &mut b)
        .unwrap_or_else(|e| panic!("{e:?}"));
    m.confirm_result(hr(), a, 1, &mut b)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(b.acknowledgements, 1);
    assert_eq!(m.fetch_result(hr(), a, &mut b, &mut output), Ok(None));
    b.sample = Some(sample(1));
    assert_eq!(m.fetch_result(hr(), a, &mut b, &mut output), Ok(None));
    b.sample = Some(sample(2));
    b.sample
        .as_mut()
        .unwrap_or_else(|| panic!("missing"))
        .length = 99;
    assert_eq!(
        m.fetch_result(hr(), a, &mut b, &mut output),
        Err(Error::InvalidLength)
    );
}
#[test]
fn test_should_mute_disconnect_events_and_not_release_resources_on_stop_failure() {
    let mut m = manager::<3>(1);
    let mut b = Fixture {
        fault_stop: true,
        ..Fixture::default()
    };
    let a = begin(&mut m, &mut b);
    m.confirm_start_response(hr(), a)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(m.disconnect(&mut b), Err(Error::Backend));
    assert_eq!(m.pending_end(hr(), a), Ok(None));
    assert_eq!(
        m.start(start(), Owner::Application, TriggerSource::App, &mut b),
        Err(Error::Busy)
    );
    b.fault_stop = false;
    m.disconnect(&mut b).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(m.query(hr(), a).map(|v| v.state), Ok(JobState::Cancelled));
    assert_eq!(m.pending_end(hr(), a), Ok(None));
}
#[test]
fn test_should_preserve_device_owned_jobs_on_disconnect() {
    let mut m = manager::<2>(1);
    let mut b = Fixture::default();
    let job = m
        .start(start(), Owner::Device, TriggerSource::Timer, &mut b)
        .unwrap_or_else(|e| panic!("{e:?}"));
    m.disconnect(&mut b).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(b.stops, 0);
    assert_eq!(m.query(hr(), job).map(|v| v.state), Ok(JobState::Accepted));
}
#[test]
fn test_should_reject_backend_mismatch_and_preserve_pending_result_until_admitted() {
    let mut m = manager::<1>(1);
    let mut b = Fixture::default();
    let job = begin(&mut m, &mut b);
    m.confirm_start_response(hr(), job)
        .unwrap_or_else(|e| panic!("{e:?}"));
    let mut output = [0; 4];
    let mut mismatch = sample(1);
    mismatch.record_type = 0x2501;
    b.sample = Some(mismatch);
    assert_eq!(
        m.fetch_result(hr(), job, &mut b, &mut output),
        Err(Error::InvalidField)
    );
    b.sample = Some(sample(1));
    m.fetch_result(hr(), job, &mut b, &mut output)
        .unwrap_or_else(|e| panic!("{e:?}"));
    b.completion = Some(Outcome::Completed);
    assert_eq!(
        m.poll_completion(hr(), job, &mut b),
        Err(Error::InvalidState)
    );
    m.confirm_result(hr(), job, 1, &mut b)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        m.poll_completion(hr(), job, &mut b),
        Ok(JobState::Completed)
    );
}
#[test]
fn test_should_attempt_all_application_cleanup_even_when_first_stop_fails() {
    let mut m = manager::<2>(1);
    let mut b = Fixture {
        resources: ResourceSet::from_bits(0),
        fault_stop: true,
        ..Fixture::default()
    };
    let first = begin(&mut m, &mut b);
    let second = begin(&mut m, &mut b);
    assert_eq!(m.disconnect(&mut b), Err(Error::Backend));
    assert_eq!(b.stops, 2);
    for id in [first, second] {
        assert_eq!(m.pending_end(hr(), id), Ok(None));
        assert_eq!(m.confirm_start_response(hr(), id), Err(Error::InvalidState));
        assert_eq!(m.query(hr(), id).map(|v| v.state), Ok(JobState::Accepted));
    }
}
#[test]
fn test_should_preserve_failed_terminal_outcome_and_admit_end_once() {
    let mut m = manager::<1>(1);
    let mut b = Fixture::default();
    let id = begin(&mut m, &mut b);
    m.confirm_start_response(hr(), id)
        .unwrap_or_else(|e| panic!("{e:?}"));
    b.completion = Some(Outcome::Failed(Reason::Timeout));
    assert_eq!(m.poll_completion(hr(), id, &mut b), Ok(JobState::Failed));
    let event = m.pending_end(hr(), id).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        event.map(|v| v.outcome),
        Some(Outcome::Failed(Reason::Timeout))
    );
    assert_eq!(m.poll_completion(hr(), id, &mut b), Ok(JobState::Failed));
    m.confirm_end_event(hr(), id)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(m.pending_end(hr(), id), Ok(None));
}
#[test]
fn test_should_reject_known_hr_payload_with_bad_flags_or_unknown_schema_before_event() {
    let mut m = manager::<1>(1);
    let mut b = Fixture::default();
    let id = begin(&mut m, &mut b);
    m.confirm_start_response(hr(), id)
        .unwrap_or_else(|e| panic!("{e:?}"));
    b.sample = Some(sample(1));
    b.data[3] = 2;
    assert_eq!(
        m.fetch_result(hr(), id, &mut b, &mut [0; 4]),
        Err(Error::InvalidField)
    );
    b.data[3] = 1;
    b.sample
        .as_mut()
        .unwrap_or_else(|| panic!("missing"))
        .schema = 2;
    assert_eq!(
        m.fetch_result(hr(), id, &mut b, &mut [0; 4]),
        Err(Error::Unsupported)
    );
}
#[test]
fn test_should_drain_real_retained_result_when_completion_arrives_before_fetch() {
    let mut m = manager::<1>(1);
    let mut b = Fixture::default();
    let id = begin(&mut m, &mut b);
    m.confirm_start_response(hr(), id)
        .unwrap_or_else(|e| panic!("{e:?}"));
    b.sample = Some(sample(1));
    b.completion = Some(Outcome::Completed);
    assert_eq!(m.poll_completion(hr(), id, &mut b), Ok(JobState::Completed));
    assert_eq!(m.pending_end(hr(), id), Ok(None));
    let mut out = [0; 4];
    assert!(
        m.fetch_result(hr(), id, &mut b, &mut out)
            .unwrap_or_else(|e| panic!("{e:?}"))
            .is_some()
    );
    m.confirm_result(hr(), id, 1, &mut b)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(m.fetch_result(hr(), id, &mut b, &mut out), Ok(None));
    assert!(
        m.pending_end(hr(), id)
            .unwrap_or_else(|e| panic!("{e:?}"))
            .is_some()
    );
    assert_eq!(b.acknowledgements, 1);
}
#[test]
fn test_should_retain_eight_completed_jobs_while_starting_a_ninth() {
    let mut m = manager::<8>(1);
    let mut b = Fixture::default();
    let mut ids = Vec::new();
    for _ in 0..8 {
        let id = begin(&mut m, &mut b);
        ids.push(id);
        m.confirm_start_response(hr(), id)
            .unwrap_or_else(|e| panic!("{e:?}"));
        m.stop(hr(), id, &mut b).unwrap_or_else(|e| panic!("{e:?}"));
        m.confirm_end_event(hr(), id)
            .unwrap_or_else(|e| panic!("{e:?}"));
    }
    let ninth = begin(&mut m, &mut b);
    assert_eq!(ninth.get(), 9);
    for id in ids {
        assert_eq!(m.query(hr(), id).map(|s| s.state), Ok(JobState::Cancelled));
    }
}

#[cfg(feature = "blood-pressure")]
#[test]
fn test_should_reject_cuff_fault_outside_module25_without_finishing_job() {
    let mut m = manager::<1>(1);
    let mut b = Fixture::default();
    let id = begin(&mut m, &mut b);
    m.confirm_start_response(hr(), id)
        .unwrap_or_else(|e| panic!("{e:?}"));
    b.completion = Some(Outcome::Failed(Reason::CuffEeprom));
    assert_eq!(
        m.poll_completion(hr(), id, &mut b),
        Err(Error::InvalidField)
    );
    assert_eq!(
        m.query(hr(), id).map(|job| job.state),
        Ok(JobState::Running)
    );
}

#[test]
fn test_should_keep_pending_result_metadata_out_of_terminal_archive_budget() {
    let bytes = core::mem::size_of::<device_health::JobManager<3>>();
    assert!(bytes <= 640, "retained job owner uses {bytes} bytes");
}

#[test]
fn test_should_preserve_archived_replays_and_mute_after_actual_slot_reuse() {
    let mut m = manager::<1>(1);
    let mut b = Fixture {
        context: 9,
        ..Fixture::default()
    };
    let request =
        HealthStart::decode(0x20, &[1, 0x20, 0, 9, 0, 0, 0, 0]).unwrap_or_else(|e| panic!("{e:?}"));
    let old = m
        .start(request, Owner::Application, TriggerSource::Sport, &mut b)
        .unwrap_or_else(|e| panic!("{e:?}"));
    m.confirm_start_response(hr(), old)
        .unwrap_or_else(|e| panic!("{e:?}"));
    b.sample = Some(sample(7));
    let mut bytes = [0; 4];
    assert!(m.fetch_result(hr(), old, &mut b, &mut bytes).is_ok());
    m.confirm_result(hr(), old, 7, &mut b)
        .unwrap_or_else(|e| panic!("{e:?}"));
    m.stop(hr(), old, &mut b)
        .unwrap_or_else(|e| panic!("{e:?}"));
    m.confirm_end_event(hr(), old)
        .unwrap_or_else(|e| panic!("{e:?}"));
    let next = begin(&mut m, &mut b);
    assert_ne!(old, next);
    let counts = (b.starts, b.stops, b.acknowledgements);
    m.confirm_start_response(hr(), old)
        .unwrap_or_else(|e| panic!("{e:?}"));
    m.confirm_end_event(hr(), old)
        .unwrap_or_else(|e| panic!("{e:?}"));
    m.confirm_result(hr(), old, 7, &mut b)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        m.confirm_result(hr(), old, 8, &mut b),
        Err(Error::InvalidState)
    );
    assert_eq!(
        m.fetch_result(hr(), old, &mut b, &mut bytes),
        Err(Error::InvalidState)
    );
    assert_eq!(
        m.poll_completion(hr(), old, &mut b),
        Ok(JobState::Cancelled)
    );
    assert_eq!(m.stop(hr(), old, &mut b), Ok(()));
    assert_eq!(m.pending_end(hr(), old), Ok(None));
    assert_eq!((b.starts, b.stops, b.acknowledgements), counts);
    m.disconnect(&mut b).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        m.confirm_start_response(hr(), old),
        Err(Error::InvalidState)
    );
    assert_eq!(m.confirm_end_event(hr(), old), Err(Error::InvalidState));
    assert_eq!(
        m.confirm_result(hr(), old, 7, &mut b),
        Err(Error::InvalidState)
    );
    assert_eq!(m.pending_end(hr(), old), Ok(None));
    assert_eq!(
        m.query(hr(), old).map(|j| (j.state, j.source)),
        Ok((JobState::Cancelled, TriggerSource::Sport))
    );
    assert_eq!(b.acknowledgements, counts.2);
}
