//! Feature selection is compile capability, not a synthetic actual backend.
#[test]
fn test_should_forward_only_the_explicit_module_features() {
    for (module, enabled) in [
        (0x20, cfg!(feature = "heart-rate")),
        (0x25, cfg!(feature = "blood-pressure")),
        (0x26, cfg!(feature = "oxygen")),
        (0x27, cfg!(feature = "glucose")),
        (0x28, cfg!(feature = "temperature")),
        (0x29, cfg!(feature = "ecg")),
        (0x2a, cfg!(feature = "wellbeing")),
        (0x2b, cfg!(feature = "microcirculation")),
    ] {
        assert_eq!(device_health::Module::from_wire(module).is_ok(), enabled);
    }
}
use device_health::{
    AcquisitionKind, Backend, BackendError, HealthStart, JobId, JobManager, Outcome, Owner,
    ProductDeclaration, Provenance, ResourceSet, ResultInfo, ResultSource, StopProgress,
    TriggerSource,
};
use device_health_service::{
    Error, HealthAccess, HealthClock, HealthHandler, HealthProduct, LatestStore, Operation,
    SharedHealthOwner, StoredLatest,
};
use device_protocol::{Frame, Header, Kind, ModuleEntry};
use device_services::ServiceHandler;
struct Fixture;
impl Backend for Fixture {
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
    fn has_pending_result(&self, _: JobId) -> Result<bool, BackendError> {
        Ok(false)
    }
    fn result(&mut self, _: JobId, _: &mut [u8]) -> Result<Option<ResultInfo>, BackendError> {
        Ok(None)
    }
    fn acknowledge_result(&mut self, _: JobId, _: u64) -> Result<(), BackendError> {
        Err(BackendError::Fault)
    }
}
impl HealthAccess for Fixture {
    fn allowed(&self, _: u8, _: Operation, _: Option<u32>) -> bool {
        true
    }
}
impl HealthClock for Fixture {
    fn now_ms(&self) -> Result<u64, Error> {
        Ok(0)
    }
}
impl HealthProduct for Fixture {
    fn declaration(&self, m: u8, t: u16) -> Result<ProductDeclaration<'_>, Error> {
        Ok(ProductDeclaration {
            module: device_health::Module::from_wire(m)?,
            record_type: t,
            acquisition: AcquisitionKind::SensorMeasurement,
            algorithm_id: "offline-fixture",
            algorithm_version: "1",
            temperature_modes: if m == 0x28 { 15 } else { 0 },
            mood_labels: &[],
        })
    }
    fn provenance(&self, _: u8, _: u16, s: ResultSource) -> Result<Provenance<'_>, Error> {
        Ok(Provenance {
            source: s,
            acquisition: AcquisitionKind::SensorMeasurement,
            algorithm_id: "offline-fixture",
            algorithm_version: "1",
        })
    }
    fn start_binding(&self, _: HealthStart) -> Result<(Owner, TriggerSource), Error> {
        Ok((Owner::Application, TriggerSource::App))
    }
}
impl LatestStore for Fixture {
    fn load(&self, _: u8, _: u16, _: &mut [u8]) -> Result<StoredLatest, Error> {
        Err(Error::Health(device_health::Error::NotFound))
    }
    fn publish(&mut self, _: u8, _: u32, _: ResultInfo, _: &[u8]) -> Result<(), Error> {
        Err(Error::Backend)
    }
    fn current(&self, _: u8, _: u16, _: u64) -> bool {
        false
    }
}
#[test]
fn test_should_route_every_selected_start_and_preserve_real_latest_absence() {
    for (m, t) in [
        (0x20, 0x20u16),
        (0x25, 0x2501),
        (0x26, 0x2601),
        (0x27, 0x2701),
        (0x28, 0x2801),
        (0x29, 0x2901),
        (0x2a, 0x2a01),
        (0x2b, 0x2b01),
    ] {
        if device_health::Module::from_wire(m).is_err() {
            continue;
        }
        let owner = SharedHealthOwner::new(
            JobManager::<1>::new(1).unwrap_or_else(|e| panic!("{e:?}")),
            Fixture,
        );
        let entries = [ModuleEntry {
            module: m,
            major: 1,
            minor: 0,
            availability: 1,
            features: 0,
            input_max: 128,
            output_max: 128,
        }];
        let mut latest = Fixture;
        let mut h = HealthHandler::new(
            &owner,
            1,
            &entries,
            &Fixture,
            &Fixture,
            &Fixture,
            &mut latest,
            None,
        )
        .unwrap_or_else(|e| panic!("{e:?}"));
        let mut body = [1, 0, 0, 0, 0, 0, 0, if m == 0x28 { 3 } else { 0 }];
        body[1..3].copy_from_slice(&t.to_le_bytes());
        let mut f = Frame {
            header: Header {
                kind: Kind::Request,
                module: m,
                opcode: 0x30,
                message_id: 2,
                offset: 0,
                total_length: 8,
            },
            payload: &body,
        };
        let mut out = [0; 128];
        assert_eq!(h.dispatch(f, &mut out), Ok(8));
        assert_eq!(&out[..8], &[0, 0, 0, 0, 1, 0, 0, 0]);
        f.header.opcode = 1;
        f.header.message_id = 3;
        f.payload = &[];
        f.header.total_length = 0;
        assert_eq!(h.dispatch(f, &mut out), Ok(7));
        assert_eq!(&out[..7], &[0, 0, 0, 0, 3, 5, 1]);
        f.header.opcode = 2;
        f.header.message_id = 3;
        let selector = t.to_le_bytes();
        f.payload = if m == 0x2a { &selector } else { &[] };
        f.header.total_length = if m == 0x2a { 2 } else { 0 };
        assert_eq!(h.dispatch(f, &mut out), Ok(4));
        assert_eq!(&out[..4], &[8, 0, 0, 0]);
        h.disconnect().unwrap_or_else(|e| panic!("{e:?}"));
    }
}
struct FailedStop;
impl Backend for FailedStop {
    fn resources(&self, r: HealthStart) -> Result<ResourceSet, BackendError> {
        Fixture.resources(r)
    }
    fn context_is_valid(&self, _: u32) -> bool {
        false
    }
    fn start(&mut self, j: JobId, r: HealthStart) -> Result<(), BackendError> {
        Fixture.start(j, r)
    }
    fn stop(&mut self, _: JobId) -> Result<StopProgress, BackendError> {
        Err(BackendError::Fault)
    }
    fn completion(&mut self, _: JobId) -> Result<Option<Outcome>, BackendError> {
        Ok(None)
    }
    fn has_pending_result(&self, _: JobId) -> Result<bool, BackendError> {
        Ok(false)
    }
    fn result(&mut self, _: JobId, _: &mut [u8]) -> Result<Option<ResultInfo>, BackendError> {
        Ok(None)
    }
    fn acknowledge_result(&mut self, _: JobId, _: u64) -> Result<(), BackendError> {
        Err(BackendError::Fault)
    }
}
#[test]
fn test_should_report_failed_cleanup_and_preserve_the_global_resource_lease() {
    let Some((module, t)) = [
        (0x20, 0x20u16),
        (0x25, 0x2501),
        (0x26, 0x2601),
        (0x27, 0x2701),
        (0x28, 0x2801),
        (0x29, 0x2901),
        (0x2a, 0x2a01),
        (0x2b, 0x2b01),
    ]
    .into_iter()
    .find(|(m, _)| device_health::Module::from_wire(*m).is_ok()) else {
        return;
    };
    let owner = SharedHealthOwner::new(
        JobManager::<2>::new(1).unwrap_or_else(|e| panic!("{e:?}")),
        FailedStop,
    );
    let entries = [ModuleEntry {
        module,
        major: 1,
        minor: 0,
        availability: 1,
        features: 0,
        input_max: 128,
        output_max: 128,
    }];
    let mut store = Fixture;
    let mut h = HealthHandler::new(
        &owner, 1, &entries, &Fixture, &Fixture, &Fixture, &mut store, None,
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    let mut body = [1, 0, 0, 0, 0, 0, 0, if module == 0x28 { 3 } else { 0 }];
    body[1..3].copy_from_slice(&t.to_le_bytes());
    let f = Frame {
        header: Header {
            kind: Kind::Request,
            module,
            opcode: 0x30,
            message_id: 2,
            offset: 0,
            total_length: 8,
        },
        payload: &body,
    };
    assert_eq!(h.dispatch(f, &mut [0; 128]), Ok(8));
    assert!(h.disconnect().is_err());
    assert!(h.must_end_session());
    let r = HealthStart::decode(module, &body).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        owner.with_manager(|m, b| m
            .start(r, Owner::Application, TriggerSource::App, b)
            .map_err(Error::from)),
        Err(Error::Health(device_health::Error::Busy))
    );
}

impl device_health_service::HealthSource for Fixture {
    fn module_run_status(&self, _: u8) -> Result<device_health_service::ModuleRunStatus, Error> {
        Ok(device_health_service::ModuleRunStatus {
            state: 3,
            reason: 0x105,
        })
    }
}
#[cfg(feature = "blood-pressure")]
struct ActualCuff;
#[cfg(feature = "blood-pressure")]
impl device_health_service::CuffPort for ActualCuff {
    fn running_cuff_job(&self, j: u32) -> bool {
        j == 1
    }
    fn pending(&self, j: u32) -> Result<Option<(u64, device_health_service::CuffPressure)>, Error> {
        Ok(Some((
            1,
            device_health_service::CuffPressure {
                job: j,
                pressure_centi_mmhg: 12000,
                battery: Some(73),
            },
        )))
    }
    fn confirm(&mut self, _: u32, _: u64) -> Result<(), Error> {
        Ok(())
    }
    fn current(&self, _: u32, _: u64) -> bool {
        true
    }
}
#[cfg(feature = "blood-pressure")]
#[test]
fn test_should_emit_actual_cuff_pressure_only_after_start_response() {
    let owner = SharedHealthOwner::new(
        JobManager::<1>::new(1).unwrap_or_else(|e| panic!("{e:?}")),
        Fixture,
    );
    let entries = [ModuleEntry {
        module: 0x25,
        major: 1,
        minor: 0,
        availability: 1,
        features: 0,
        input_max: 128,
        output_max: 128,
    }];
    let mut store = Fixture;
    let mut cuff = ActualCuff;
    let mut h = HealthHandler::new(
        &owner, 1, &entries, &Fixture, &Fixture, &Fixture, &mut store, None,
    )
    .unwrap_or_else(|e| panic!("{e:?}"))
    .with_cuff(&mut cuff)
    .unwrap_or_else(|e| panic!("{e:?}"));
    let f = Frame {
        header: Header {
            kind: Kind::Request,
            module: 0x25,
            opcode: 0x30,
            message_id: 2,
            offset: 0,
            total_length: 8,
        },
        payload: &[1, 1, 0x25, 0, 0, 0, 0, 0],
    };
    let mut out = [0; 128];
    assert_eq!(h.dispatch(f, &mut out), Ok(8));
    assert!(h.prepare_cuff(1, &mut out, 128).is_err());
    h.response_admitted(f, &[0, 0, 0, 0, 1, 0, 0, 0])
        .unwrap_or_else(|e| panic!("{e:?}"));
    let (token, n) = h
        .prepare_cuff(1, &mut out, 128)
        .unwrap_or_else(|e| panic!("{e:?}"))
        .unwrap_or_else(|| panic!("missing actual cuff sample"));
    assert_eq!(n, 9);
    assert_eq!(&out[..9], &[1, 0, 0, 0, 0xe0, 0x2e, 0, 0, 73]);
    h.confirm_event(token).unwrap_or_else(|e| panic!("{e:?}"));
}
#[cfg(any(feature = "ecg", feature = "wellbeing"))]
struct BodyBackend<'a> {
    record: u16,
    schema: u16,
    body: &'a [u8],
}
#[cfg(any(feature = "ecg", feature = "wellbeing"))]
impl Backend for BodyBackend<'_> {
    fn resources(&self, r: HealthStart) -> Result<ResourceSet, BackendError> {
        Fixture.resources(r)
    }
    fn context_is_valid(&self, _: u32) -> bool {
        false
    }
    fn start(&mut self, j: JobId, r: HealthStart) -> Result<(), BackendError> {
        Fixture.start(j, r)
    }
    fn stop(&mut self, _: JobId) -> Result<StopProgress, BackendError> {
        Ok(StopProgress::Stopped)
    }
    fn completion(&mut self, _: JobId) -> Result<Option<Outcome>, BackendError> {
        Ok(None)
    }
    fn has_pending_result(&self, _: JobId) -> Result<bool, BackendError> {
        Ok(true)
    }
    fn result(&mut self, _: JobId, out: &mut [u8]) -> Result<Option<ResultInfo>, BackendError> {
        out[..self.body.len()].copy_from_slice(self.body);
        Ok(Some(ResultInfo {
            sample_id: 1,
            record_type: self.record,
            schema: self.schema,
            timestamp: device_health::Timestamp::unknown(),
            length: self.body.len(),
            source: ResultSource::DeviceAlgorithm,
        }))
    }
    fn acknowledge_result(&mut self, _: JobId, _: u64) -> Result<(), BackendError> {
        Ok(())
    }
}
#[cfg(any(feature = "ecg", feature = "wellbeing"))]
struct Sink {
    info: Option<ResultInfo>,
    body: Vec<u8>,
}
#[cfg(any(feature = "ecg", feature = "wellbeing"))]
impl LatestStore for Sink {
    fn load(&self, _: u8, _: u16, out: &mut [u8]) -> Result<StoredLatest, Error> {
        let info = self
            .info
            .ok_or(Error::Health(device_health::Error::NotFound))?;
        out[..self.body.len()].copy_from_slice(&self.body);
        Ok(StoredLatest {
            info,
            generation: 1,
        })
    }
    fn publish(&mut self, _: u8, _: u32, info: ResultInfo, body: &[u8]) -> Result<(), Error> {
        self.body = body.to_vec();
        self.info = Some(info);
        Ok(())
    }
    fn current(&self, _: u8, _: u16, g: u64) -> bool {
        g == 1 && self.info.is_some()
    }
}
#[cfg(any(feature = "ecg", feature = "wellbeing"))]
fn check_typed_event(module: u8, record: u16, schema: u16, body: &[u8]) {
    let owner = SharedHealthOwner::new(
        JobManager::<1>::new(1).unwrap_or_else(|e| panic!("{e:?}")),
        BodyBackend {
            record,
            schema,
            body,
        },
    );
    let entries = [ModuleEntry {
        module,
        major: 1,
        minor: 0,
        availability: 1,
        features: 0,
        input_max: 128,
        output_max: 128,
    }];
    let mut store = Sink {
        info: None,
        body: Vec::new(),
    };
    let mut h = HealthHandler::new(
        &owner, 1, &entries, &Fixture, &Fixture, &Fixture, &mut store, None,
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    let mut request_body = [1, 0, 0, 0, 0, 0, 0, 0];
    request_body[1..3].copy_from_slice(&record.to_le_bytes());
    let f = Frame {
        header: Header {
            kind: Kind::Request,
            module,
            opcode: 0x30,
            message_id: 2,
            offset: 0,
            total_length: 8,
        },
        payload: &request_body,
    };
    let mut out = [0; 128];
    assert_eq!(h.dispatch(f, &mut out), Ok(8));
    h.response_admitted(f, &[0, 0, 0, 0, 1, 0, 0, 0])
        .unwrap_or_else(|e| panic!("{e:?}"));
    let (_, n) = h
        .prepare_result(module, 1, &mut out, 128)
        .unwrap_or_else(|e| panic!("{e:?}"))
        .unwrap_or_else(|| panic!("actual typed result absent"));
    assert_eq!(n, body.len() + 15);
    assert_eq!(&out[..2], &record.to_le_bytes());
    assert_eq!(&out[2..4], &schema.to_le_bytes());
    assert_eq!(&out[15..15 + body.len()], body);
}
#[cfg(feature = "ecg")]
#[test]
fn test_should_preserve_every_ecg_process_field_in_schema2_latest_event() {
    let mut body = [0; 21];
    let value = device_health::Ecg {
        measurement: 0x0001_0000_0001,
        user_slot: 7,
        ecg_bpm: Some(60),
        ppg_bpm: Some(61),
        avg_bpm: Some(65),
        min_bpm: Some(55),
        max_bpm: Some(75),
    };
    value.encode(&mut body).unwrap_or_else(|e| panic!("{e:?}"));
    check_typed_event(0x29, 0x2901, 2, &body);
}
#[cfg(feature = "wellbeing")]
#[test]
fn test_should_preserve_selected_wellbeing_type_unit_scale_source_and_actual_value() {
    let value = device_health::Wellbeing {
        kind: device_health::WellbeingKind::Rmssd,
        value: Some(1234),
        scale_exp: -3,
        source: Some(ResultSource::DeviceAlgorithm),
    };
    let mut body = [0; 8];
    value.encode(&mut body).unwrap_or_else(|e| panic!("{e:?}"));
    check_typed_event(0x2a, 0x2a04, 1, &body);
}

#[test]
fn test_should_reject_reserved_module_state_and_invalid_cuff_battery() {
    use device_health_service::{CuffPressure, ModuleRunStatus};
    assert!(ModuleRunStatus::decode(&[4, 0, 0]).is_err());
    assert!(ModuleRunStatus::decode(&[1, 0, 0, 0]).is_err());
    assert!(CuffPressure::decode(&[1, 0, 0, 0, 0, 0, 0, 0, 101]).is_err());
    let p =
        CuffPressure::decode(&[1, 0, 0, 0, 0, 0, 0, 0, 255]).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(p.battery, None);
}
#[test]
fn test_should_redact_latest_metadata_timestamps_and_sample_ids() {
    let v = StoredLatest {
        info: ResultInfo {
            sample_id: 123_456_789,
            record_type: 0x20,
            schema: 1,
            timestamp: device_health::Timestamp::new(987_654_321, true)
                .unwrap_or_else(|e| panic!("{e:?}")),
            length: 4,
            source: ResultSource::DeviceAlgorithm,
        },
        generation: 1,
    };
    let debug = format!("{v:?}");
    assert!(!debug.contains("123456789"));
    assert!(!debug.contains("987654321"));
}
#[cfg(feature = "heart-rate")]
struct NoCuff;
#[cfg(feature = "heart-rate")]
impl device_health_service::CuffPort for NoCuff {
    fn running_cuff_job(&self, _: u32) -> bool {
        false
    }
    fn pending(&self, _: u32) -> Result<Option<(u64, device_health_service::CuffPressure)>, Error> {
        Err(Error::Invalid)
    }
    fn confirm(&mut self, _: u32, _: u64) -> Result<(), Error> {
        Err(Error::Invalid)
    }
    fn current(&self, _: u32, _: u64) -> bool {
        false
    }
}
#[cfg(feature = "heart-rate")]
#[test]
fn test_should_release_connection_binding_when_cuff_builder_rejects_missing_module() {
    let owner = SharedHealthOwner::new(
        JobManager::<1>::new(1).unwrap_or_else(|e| panic!("{e:?}")),
        Fixture,
    );
    let entries = [ModuleEntry {
        module: 0x20,
        major: 1,
        minor: 0,
        availability: 1,
        features: 0,
        input_max: 128,
        output_max: 128,
    }];
    let mut store = Fixture;
    let mut cuff = NoCuff;
    assert!(
        HealthHandler::new(
            &owner, 1, &entries, &Fixture, &Fixture, &Fixture, &mut store, None
        )
        .unwrap_or_else(|e| panic!("{e:?}"))
        .with_cuff(&mut cuff)
        .is_err()
    );
    assert!(
        HealthHandler::new(
            &owner, 2, &entries, &Fixture, &Fixture, &Fixture, &mut store, None
        )
        .is_ok()
    );
}
