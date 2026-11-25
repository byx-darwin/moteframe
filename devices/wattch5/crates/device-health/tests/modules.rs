//! Execute the common engine for every individually selected feature, without a physiological algorithm.
use device_health::{
    Backend, BackendError, HealthStart, JobId, JobManager, JobState, Outcome, Owner, ResourceSet,
    ResultInfo, StopProgress, TriggerSource,
};
struct TestPort;
impl Backend for TestPort {
    fn has_pending_result(&self, _: JobId) -> Result<bool, BackendError> {
        Ok(false)
    }
    fn resources(&self, _: HealthStart) -> Result<ResourceSet, BackendError> {
        Ok(ResourceSet::from_bits(1))
    }
    fn context_is_valid(&self, _: u32) -> bool {
        false
    }
    fn start(&mut self, _: JobId, _: HealthStart) -> Result<(), BackendError> {
        Ok(())
    }
    fn stop(&mut self, _: JobId) -> Result<StopProgress, BackendError> {
        Ok(StopProgress::Stopped)
    }
    fn completion(&mut self, _: JobId) -> Result<Option<Outcome>, BackendError> {
        Ok(None)
    }
    fn result(&mut self, _: JobId, _: &mut [u8]) -> Result<Option<ResultInfo>, BackendError> {
        Ok(None)
    }
    fn acknowledge_result(&mut self, _: JobId, _: u64) -> Result<(), BackendError> {
        Err(BackendError::Fault)
    }
}
#[test]
fn test_should_use_common_engine_for_every_selected_measurement_module() {
    let mut manager = JobManager::<1>::new(1).unwrap_or_else(|e| panic!("{e:?}"));
    let mut port = TestPort;
    let mut expected_id = 1;
    for (selected, module, record, temp) in [
        (cfg!(feature = "heart-rate"), 0x20, 0x0020u16, 0),
        (cfg!(feature = "blood-pressure"), 0x25, 0x2501, 0),
        (cfg!(feature = "oxygen"), 0x26, 0x2601, 0),
        (cfg!(feature = "glucose"), 0x27, 0x2701, 0),
        (cfg!(feature = "temperature"), 0x28, 0x2801, 3),
        (cfg!(feature = "ecg"), 0x29, 0x2901, 0),
        (cfg!(feature = "wellbeing"), 0x2a, 0x2a01, 0),
        (cfg!(feature = "microcirculation"), 0x2b, 0x2b01, 0),
    ] {
        if !selected {
            continue;
        }
        let [lo, hi] = record.to_le_bytes();
        let request = HealthStart::decode(module, &[1, lo, hi, 0, 0, 0, 0, temp])
            .unwrap_or_else(|e| panic!("{e:?}"));
        let id = manager
            .start(request, Owner::Application, TriggerSource::App, &mut port)
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(id.get(), expected_id);
        expected_id += 1;
        assert_eq!(
            manager.query(request.module(), id).map(|j| j.state),
            Ok(JobState::Accepted)
        );
        manager
            .confirm_start_response(request.module(), id)
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(
            manager.fetch_result(request.module(), id, &mut port, &mut [0; 4]),
            Ok(None)
        );
        manager
            .stop(request.module(), id, &mut port)
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(
            manager.query(request.module(), id).map(|j| j.state),
            Ok(JobState::Cancelled)
        );
        assert!(
            manager
                .pending_end(request.module(), id)
                .unwrap_or_else(|e| panic!("{e:?}"))
                .is_some()
        );
        manager
            .confirm_end_event(request.module(), id)
            .unwrap_or_else(|e| panic!("{e:?}"));
    }
}
