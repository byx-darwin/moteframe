#![cfg(feature = "wellbeing")]
//! Actual typed fatigue snapshots traverse START, E80, storage and LATEST without source promotion.
use core::cell::{Cell, RefCell};
use device_health::*;
use device_health_service::*;
use device_protocol::{Frame, Header, Kind, ModuleEntry};
use device_services::ServiceHandler;
const BODY: [u8; 12] = [2, 0, 0, 0, 0, 2, 1, 1, 60, 0, 1, 5];
const ENTRY: ModuleEntry = ModuleEntry {
    module: 0x2a,
    major: 1,
    minor: 0,
    availability: 1,
    features: 0,
    input_max: 128,
    output_max: 128,
};
struct Access(Cell<bool>);
impl HealthAccess for Access {
    fn allowed(&self, _: u8, _: Operation, _: Option<u32>) -> bool {
        self.0.get()
    }
}
struct Clock;
impl HealthClock for Clock {
    fn now_ms(&self) -> Result<u64, device_health_service::Error> {
        Ok(1)
    }
}
struct Product;
impl HealthSource for Product {
    fn module_run_status(&self, _: u8) -> Result<ModuleRunStatus, device_health_service::Error> {
        Ok(ModuleRunStatus {
            state: 1,
            reason: 0,
        })
    }
}
impl HealthProduct for Product {
    fn declaration(
        &self,
        m: u8,
        t: u16,
    ) -> Result<ProductDeclaration<'_>, device_health_service::Error> {
        Ok(ProductDeclaration {
            module: Module::from_wire(m)?,
            record_type: t,
            acquisition: AcquisitionKind::SensorMeasurement,
            algorithm_id: "actual-fixture",
            algorithm_version: "1",
            temperature_modes: 0,
            mood_labels: &[],
        })
    }
    fn provenance(
        &self,
        _: u8,
        _: u16,
        source: ResultSource,
    ) -> Result<Provenance<'_>, device_health_service::Error> {
        Ok(Provenance {
            source,
            acquisition: AcquisitionKind::SensorMeasurement,
            algorithm_id: "actual-fixture",
            algorithm_version: "1",
        })
    }
    fn start_binding(
        &self,
        _: HealthStart,
    ) -> Result<(Owner, TriggerSource), device_health_service::Error> {
        Ok((Owner::Application, TriggerSource::App))
    }
}
struct Actual<'a> {
    pending: &'a Cell<bool>,
    acks: &'a Cell<u32>,
    source: ResultSource,
}
fn info(schema: u16, length: usize, source: ResultSource) -> ResultInfo {
    ResultInfo {
        sample_id: 1,
        record_type: 0x2a01,
        schema,
        timestamp: Timestamp::unknown(),
        length,
        source,
    }
}
impl Backend for Actual<'_> {
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
        Ok(self.pending.get())
    }
    fn result(&mut self, _: JobId, out: &mut [u8]) -> Result<Option<ResultInfo>, BackendError> {
        if !self.pending.get() {
            return Ok(None);
        }
        out[..12].copy_from_slice(&BODY);
        Ok(Some(info(2, 12, self.source)))
    }
    fn acknowledge_result(&mut self, _: JobId, _: u64) -> Result<(), BackendError> {
        self.acks.set(self.acks.get() + 1);
        self.pending.set(false);
        Ok(())
    }
}
#[derive(Default)]
struct Store {
    snapshot: RefCell<Option<(ResultInfo, [u8; 12])>>,
}
impl LatestStore for Store {
    fn load(
        &self,
        _: u8,
        _: u16,
        out: &mut [u8],
    ) -> Result<StoredLatest, device_health_service::Error> {
        let (i, body) = self
            .snapshot
            .borrow()
            .ok_or(device_health_service::Error::Health(
                device_health::Error::NotFound,
            ))?;
        out[..i.length].copy_from_slice(&body[..i.length]);
        Ok(StoredLatest {
            info: i,
            generation: 1,
        })
    }
    fn publish(
        &mut self,
        _: u8,
        _: u32,
        i: ResultInfo,
        bytes: &[u8],
    ) -> Result<(), device_health_service::Error> {
        let mut body = [0; 12];
        body[..bytes.len()].copy_from_slice(bytes);
        *self.snapshot.borrow_mut() = Some((i, body));
        Ok(())
    }
    fn current(&self, _: u8, _: u16, generation: u64) -> bool {
        generation == 1 && self.snapshot.borrow().is_some()
    }
}
fn request(opcode: u8, id: u16, payload: &[u8]) -> Frame<'_> {
    Frame {
        header: Header {
            kind: Kind::Request,
            module: 0x2a,
            opcode,
            message_id: id,
            offset: 0,
            total_length: u16::try_from(payload.len()).unwrap_or_default(),
        },
        payload,
    }
}
#[test]
fn test_should_stream_fatigue_schema2_and_read_same_actual_latest() {
    let pending = Cell::new(true);
    let acks = Cell::new(0);
    let auth = Access(Cell::new(true));
    let mut store = Store::default();
    let owner = SharedHealthOwner::new(
        JobManager::<2>::new(1).unwrap_or_else(|e| panic!("{e:?}")),
        Actual {
            pending: &pending,
            acks: &acks,
            source: ResultSource::DeviceAlgorithm,
        },
    );
    let mut h = HealthHandler::new(
        &owner,
        1,
        &[ENTRY],
        &auth,
        &Clock,
        &Product,
        &mut store,
        None,
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    let mut out = [0; 128];
    let start = [1, 1, 0x2a, 0, 0, 0, 0, 0];
    assert_eq!(h.dispatch(request(0x30, 2, &start), &mut out), Ok(8));
    h.response_admitted(request(0x30, 2, &start), &out[..8])
        .unwrap_or_else(|e| panic!("{e:?}"));
    let (token, n) = h
        .prepare_result(0x2a, 1, &mut out, 128)
        .unwrap_or_else(|e| panic!("{e:?}"))
        .unwrap_or_else(|| panic!("missing result"));
    assert_eq!(n, 27);
    assert_eq!(&out[..4], &[1, 0x2a, 2, 0]);
    assert_eq!(&out[15..27], &BODY);
    h.confirm_event(token).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(acks.get(), 1);
    assert_eq!(h.dispatch(request(2, 3, &[1, 0x2a]), &mut out), Ok(31));
    assert_eq!(&out[19..31], &BODY);
}

#[test]
fn test_should_reject_fatigue_snapshot_source_mismatch_without_acknowledging() {
    let pending = Cell::new(true);
    let acks = Cell::new(0);
    let auth = Access(Cell::new(true));
    let mut store = Store::default();
    let owner = SharedHealthOwner::new(
        JobManager::<2>::new(1).unwrap_or_else(|e| panic!("{e:?}")),
        Actual {
            pending: &pending,
            acks: &acks,
            source: ResultSource::PhoneImport,
        },
    );
    let mut h = HealthHandler::new(
        &owner,
        1,
        &[ENTRY],
        &auth,
        &Clock,
        &Product,
        &mut store,
        None,
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    let mut out = [0; 128];
    let start = [1, 1, 0x2a, 0, 0, 0, 0, 0];
    assert_eq!(h.dispatch(request(0x30, 2, &start), &mut out), Ok(8));
    h.response_admitted(request(0x30, 2, &start), &out[..8])
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert!(h.prepare_result(0x2a, 1, &mut out, 128).is_err());
    assert_eq!(out, [0; 128]);
    assert_eq!(acks.get(), 0);
    assert!(pending.get());
}
#[test]
fn test_should_recheck_scope_for_retained_fatigue_snapshot() {
    let pending = Cell::new(true);
    let acks = Cell::new(0);
    let auth = Access(Cell::new(true));
    let mut store = Store::default();
    let owner = SharedHealthOwner::new(
        JobManager::<2>::new(1).unwrap_or_else(|e| panic!("{e:?}")),
        Actual {
            pending: &pending,
            acks: &acks,
            source: ResultSource::DeviceAlgorithm,
        },
    );
    let mut h = HealthHandler::new(
        &owner,
        1,
        &[ENTRY],
        &auth,
        &Clock,
        &Product,
        &mut store,
        None,
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    let mut out = [0; 128];
    let start = [1, 1, 0x2a, 0, 0, 0, 0, 0];
    assert_eq!(h.dispatch(request(0x30, 2, &start), &mut out), Ok(8));
    h.response_admitted(request(0x30, 2, &start), &out[..8])
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert!(h.prepare_result(0x2a, 1, &mut out, 128).is_ok());
    auth.0.set(false);
    assert!(h.prepare_result(0x2a, 1, &mut out, 128).is_err());
    assert_eq!(out, [0; 128]);
    assert_eq!(acks.get(), 0);
}
#[test]
fn test_should_keep_original_fatigue_schema1_latest_readable() {
    let pending = Cell::new(false);
    let acks = Cell::new(0);
    let auth = Access(Cell::new(true));
    let mut body = [0; 12];
    body[..8].copy_from_slice(&BODY[..8]);
    let mut store = Store {
        snapshot: RefCell::new(Some((info(1, 8, ResultSource::DeviceAlgorithm), body))),
    };
    let owner = SharedHealthOwner::new(
        JobManager::<2>::new(1).unwrap_or_else(|e| panic!("{e:?}")),
        Actual {
            pending: &pending,
            acks: &acks,
            source: ResultSource::DeviceAlgorithm,
        },
    );
    let mut h = HealthHandler::new(
        &owner,
        1,
        &[ENTRY],
        &auth,
        &Clock,
        &Product,
        &mut store,
        None,
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    let mut out = [0; 128];
    assert_eq!(h.dispatch(request(2, 3, &[1, 0x2a]), &mut out), Ok(27));
    assert_eq!(&out[19..27], &BODY[..8]);
    assert_eq!(&out[6..8], &[1, 0]);
}
