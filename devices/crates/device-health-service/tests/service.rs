#![cfg(feature = "heart-rate")]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use device_health::*;
use device_health_service::*;
use device_protocol::{Frame, Header, Kind, ModuleEntry};
use device_services::ServiceHandler;
use std::cell::Cell;
const ENTRY: ModuleEntry = ModuleEntry {
    module: 0x20,
    major: 1,
    minor: 0,
    availability: 1,
    features: 0,
    input_max: 128,
    output_max: 128,
};
struct Auth(Cell<bool>);
impl HealthAccess for Auth {
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
struct Store;
impl LatestStore for Store {
    fn load(
        &self,
        _: u8,
        _: u16,
        _: &mut [u8],
    ) -> Result<StoredLatest, device_health_service::Error> {
        Err(device_health_service::Error::Health(
            device_health::Error::NotFound,
        ))
    }
    fn publish(
        &mut self,
        _: u8,
        _: u32,
        _: ResultInfo,
        _: &[u8],
    ) -> Result<(), device_health_service::Error> {
        Ok(())
    }
    fn current(&self, _: u8, _: u16, _: u64) -> bool {
        true
    }
}
struct Actual<'a> {
    starts: &'a Cell<u32>,
}
impl Backend for Actual<'_> {
    fn resources(&self, _: HealthStart) -> Result<ResourceSet, BackendError> {
        Ok(ResourceSet::from_bits(1))
    }
    fn context_is_valid(&self, _: u32) -> bool {
        false
    }
    fn start(&mut self, _: JobId, _: HealthStart) -> Result<(), BackendError> {
        self.starts.set(self.starts.get() + 1);
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
        Ok(())
    }
}
fn request(opcode: u8, id: u16, payload: &[u8]) -> Frame<'_> {
    Frame {
        header: Header {
            kind: Kind::Request,
            module: 0x20,
            opcode,
            message_id: id,
            offset: 0,
            total_length: u16::try_from(payload.len()).unwrap(),
        },
        payload,
    }
}
#[test]
fn test_should_start_real_backend_and_emit_exact_nonzero_job() {
    let starts = Cell::new(0);
    let owner =
        SharedHealthOwner::new(JobManager::<2>::new(1).unwrap(), Actual { starts: &starts });
    let auth = Auth(Cell::new(true));
    let mut store = Store;
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
    .unwrap();
    let mut out = [0xa5; 128];
    assert_eq!(
        h.dispatch(request(0x30, 2, &[1, 0x20, 0, 0, 0, 0, 0, 0]), &mut out),
        Ok(8)
    );
    assert_eq!(&out[..8], &[0, 0, 0, 0, 1, 0, 0, 0]);
    assert_eq!(starts.get(), 1);
}
struct Stream<'a> {
    starts: &'a Cell<u32>,
    acks: &'a Cell<u32>,
    stops: &'a Cell<u32>,
    sample: &'a Cell<bool>,
    done: &'a Cell<bool>,
}
impl Backend for Stream<'_> {
    fn resources(&self, _: HealthStart) -> Result<ResourceSet, BackendError> {
        Ok(ResourceSet::from_bits(1))
    }
    fn context_is_valid(&self, _: u32) -> bool {
        false
    }
    fn start(&mut self, _: JobId, _: HealthStart) -> Result<(), BackendError> {
        self.starts.set(self.starts.get() + 1);
        Ok(())
    }
    fn stop(&mut self, _: JobId) -> Result<StopProgress, BackendError> {
        self.stops.set(self.stops.get() + 1);
        Ok(StopProgress::Stopped)
    }
    fn completion(&mut self, _: JobId) -> Result<Option<Outcome>, BackendError> {
        Ok(self.done.get().then_some(Outcome::Completed))
    }
    fn has_pending_result(&self, _: JobId) -> Result<bool, BackendError> {
        Ok(self.sample.get())
    }
    fn result(&mut self, _: JobId, out: &mut [u8]) -> Result<Option<ResultInfo>, BackendError> {
        if !self.sample.get() {
            return Ok(None);
        }
        if out.len() < 4 {
            return Err(BackendError::Capacity);
        }
        out[..4].copy_from_slice(&[60, 0, 100, 1]);
        Ok(Some(ResultInfo {
            sample_id: 1,
            record_type: 0x20,
            schema: 1,
            timestamp: Timestamp::unknown(),
            length: 4,
            source: ResultSource::DeviceAlgorithm,
        }))
    }
    fn acknowledge_result(&mut self, _: JobId, _: u64) -> Result<(), BackendError> {
        self.acks.set(self.acks.get() + 1);
        self.sample.set(false);
        Ok(())
    }
}
struct Memory<'a> {
    version: &'a Cell<u64>,
    value: std::cell::RefCell<Option<(ResultInfo, Vec<u8>)>>,
}
impl LatestStore for Memory<'_> {
    fn load(
        &self,
        _: u8,
        t: u16,
        out: &mut [u8],
    ) -> Result<StoredLatest, device_health_service::Error> {
        let value = self.value.borrow();
        let (info, body) = value.as_ref().ok_or(device_health_service::Error::Health(
            device_health::Error::NotFound,
        ))?;
        if info.record_type != t {
            return Err(device_health_service::Error::Health(
                device_health::Error::NotFound,
            ));
        }
        out[..body.len()].copy_from_slice(body);
        Ok(StoredLatest {
            info: *info,
            generation: self.version.get(),
        })
    }
    fn publish(
        &mut self,
        _: u8,
        _: u32,
        info: ResultInfo,
        body: &[u8],
    ) -> Result<(), device_health_service::Error> {
        *self.value.borrow_mut() = Some((info, body.to_vec()));
        Ok(())
    }
    fn current(&self, _: u8, _: u16, g: u64) -> bool {
        g == self.version.get()
    }
}

fn start(h: &mut impl ServiceHandler, out: &mut [u8]) {
    assert_eq!(
        h.dispatch(request(0x30, 2, &[1, 0x20, 0, 0, 0, 0, 0, 0]), out),
        Ok(8)
    );
}
#[test]
fn test_should_gate_real_result_until_response_and_retry_without_double_ack() {
    let starts = Cell::new(0);
    let acks = Cell::new(0);
    let stops = Cell::new(0);
    let sample = Cell::new(true);
    let done = Cell::new(false);
    let owner = SharedHealthOwner::new(
        JobManager::<2>::new(1).unwrap(),
        Stream {
            starts: &starts,
            acks: &acks,
            stops: &stops,
            sample: &sample,
            done: &done,
        },
    );
    let auth = Auth(Cell::new(true));
    let version = Cell::new(1);
    let mut store = Memory {
        version: &version,
        value: std::cell::RefCell::new(None),
    };
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
    .unwrap();
    let mut out = [0; 128];
    start(&mut h, &mut out);
    assert!(h.prepare_result(0x20, 1, &mut out, 128).is_err());
    assert_eq!(acks.get(), 0);
    h.response_admitted(
        request(0x30, 2, &[1, 0x20, 0, 0, 0, 0, 0, 0]),
        &[0, 0, 0, 0, 1, 0, 0, 0],
    )
    .unwrap();
    let (token, n) = h.prepare_result(0x20, 1, &mut out, 128).unwrap().unwrap();
    assert_eq!(n, 19);
    assert_eq!(
        &out[..19],
        &[
            0x20, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 0, 60, 0, 100, 1
        ]
    );
    let original = out;
    assert_eq!(
        h.prepare_result(0x20, 1, &mut out, 128).unwrap(),
        Some((token, n))
    );
    assert_eq!(out, original);
    assert_eq!(acks.get(), 0);
    h.confirm_event(token).unwrap();
    h.confirm_event(token).unwrap();
    assert_eq!(acks.get(), 1);
    assert!(h.prepare_result(0x20, 1, &mut out, 128).unwrap().is_none());
}
#[test]
fn test_should_wait_for_stop_response_before_real_end() {
    let starts = Cell::new(0);
    let acks = Cell::new(0);
    let stops = Cell::new(0);
    let sample = Cell::new(true);
    let done = Cell::new(false);
    let owner = SharedHealthOwner::new(
        JobManager::<2>::new(1).unwrap(),
        Stream {
            starts: &starts,
            acks: &acks,
            stops: &stops,
            sample: &sample,
            done: &done,
        },
    );
    let auth = Auth(Cell::new(true));
    let version = Cell::new(1);
    let mut store = Memory {
        version: &version,
        value: std::cell::RefCell::new(None),
    };
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
    .unwrap();
    let mut out = [0; 128];
    sample.set(false);
    start(&mut h, &mut out);
    h.response_admitted(
        request(0x30, 2, &[1, 0x20, 0, 0, 0, 0, 0, 0]),
        &[0, 0, 0, 0, 1, 0, 0, 0],
    )
    .unwrap();
    assert_eq!(h.dispatch(request(0x31, 3, &[1, 0, 0, 0]), &mut out), Ok(4));
    assert!(h.prepare_end(0x20, 1, &mut out, 128).is_err());
    h.response_admitted(request(0x31, 3, &[1, 0, 0, 0]), &[0, 0, 0, 0])
        .unwrap();
    let (token, n) = h.prepare_end(0x20, 1, &mut out, 128).unwrap().unwrap();
    assert_eq!(n, 10);
    assert_eq!(&out[..10], &[1, 0, 0, 0, 1, 0, 4, 0, 4, 1]);
    h.confirm_event(token).unwrap();
    assert_eq!(h.prepare_end(0x20, 1, &mut out, 128).unwrap(), None);
    assert_eq!(stops.get(), 1);
}
#[test]
fn test_should_retain_sample_on_carrier_failure_and_revoke_pending_bytes() {
    let starts = Cell::new(0);
    let acks = Cell::new(0);
    let stops = Cell::new(0);
    let sample = Cell::new(true);
    let done = Cell::new(false);
    let owner = SharedHealthOwner::new(
        JobManager::<2>::new(1).unwrap(),
        Stream {
            starts: &starts,
            acks: &acks,
            stops: &stops,
            sample: &sample,
            done: &done,
        },
    );
    let auth = Auth(Cell::new(true));
    let version = Cell::new(1);
    let mut store = Memory {
        version: &version,
        value: std::cell::RefCell::new(None),
    };
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
    .unwrap();
    let mut out = [0; 128];
    start(&mut h, &mut out);
    h.response_admitted(
        request(0x30, 2, &[1, 0x20, 0, 0, 0, 0, 0, 0]),
        &[0, 0, 0, 0, 1, 0, 0, 0],
    )
    .unwrap();
    assert!(h.prepare_result(0x20, 1, &mut [0; 18], 128).is_err());
    assert_eq!(acks.get(), 0);
    h.prepare_result(0x20, 1, &mut out, 128).unwrap().unwrap();
    auth.0.set(false);
    out.fill(0xa5);
    assert!(h.prepare_result(0x20, 1, &mut out, 128).is_err());
    assert!(out.iter().all(|b| *b == 0));
    assert!(h.must_end_session());
    assert_eq!(stops.get(), 1);
}
#[test]
fn test_should_bind_latest_cache_to_actual_store_generation() {
    let starts = Cell::new(0);
    let acks = Cell::new(0);
    let stops = Cell::new(0);
    let sample = Cell::new(true);
    let done = Cell::new(false);
    let owner = SharedHealthOwner::new(
        JobManager::<2>::new(1).unwrap(),
        Stream {
            starts: &starts,
            acks: &acks,
            stops: &stops,
            sample: &sample,
            done: &done,
        },
    );
    let auth = Auth(Cell::new(true));
    let version = Cell::new(1);
    let mut store = Memory {
        version: &version,
        value: std::cell::RefCell::new(None),
    };
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
    .unwrap();
    let mut out = [0; 128];
    start(&mut h, &mut out);
    h.response_admitted(
        request(0x30, 2, &[1, 0x20, 0, 0, 0, 0, 0, 0]),
        &[0, 0, 0, 0, 1, 0, 0, 0],
    )
    .unwrap();
    h.prepare_result(0x20, 1, &mut out, 128).unwrap().unwrap();
    let frame = request(2, 3, &[]);
    assert_eq!(h.dispatch(frame, &mut out), Ok(23));
    assert!(h.replay_valid(frame, &out[..23]));
    version.set(2);
    assert!(!h.replay_valid(frame, &out[..23]));
}
#[test]
fn test_should_reject_second_connection_and_no_reuse_after_close() {
    let starts = Cell::new(0);
    let acks = Cell::new(0);
    let stops = Cell::new(0);
    let sample = Cell::new(true);
    let done = Cell::new(false);
    let owner = SharedHealthOwner::new(
        JobManager::<2>::new(1).unwrap(),
        Stream {
            starts: &starts,
            acks: &acks,
            stops: &stops,
            sample: &sample,
            done: &done,
        },
    );
    let auth = Auth(Cell::new(true));
    let version = Cell::new(1);
    let mut store = Memory {
        version: &version,
        value: std::cell::RefCell::new(None),
    };
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
    .unwrap();
    assert!(
        HealthHandler::new(
            &owner,
            2,
            &[ENTRY],
            &auth,
            &Clock,
            &Product,
            &mut Store,
            None
        )
        .is_err()
    );
    h.disconnect().unwrap();
    assert!(
        HealthHandler::new(
            &owner,
            1,
            &[ENTRY],
            &auth,
            &Clock,
            &Product,
            &mut Store,
            None
        )
        .is_err()
    );
    assert!(
        HealthHandler::new(
            &owner,
            2,
            &[ENTRY],
            &auth,
            &Clock,
            &Product,
            &mut Store,
            None
        )
        .is_ok()
    );
}
#[test]
fn test_should_make_per_job_revocation_visible_even_before_an_unrelated_core_ping() {
    let starts = Cell::new(0);
    let acks = Cell::new(0);
    let stops = Cell::new(0);
    let sample = Cell::new(true);
    let done = Cell::new(false);
    let owner = SharedHealthOwner::new(
        JobManager::<2>::new(1).unwrap(),
        Stream {
            starts: &starts,
            acks: &acks,
            stops: &stops,
            sample: &sample,
            done: &done,
        },
    );
    let auth = Auth(Cell::new(true));
    let version = Cell::new(1);
    let mut store = Memory {
        version: &version,
        value: std::cell::RefCell::new(None),
    };
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
    .unwrap();
    let mut out = [0; 128];
    start(&mut h, &mut out);
    auth.0.set(false);
    assert!(h.must_end_session());
    assert_eq!(stops.get(), 1);
}
struct Zones<'a>(&'a Cell<bool>);
impl ZonePort for Zones<'_> {
    fn snapshot(&self, _: u32) -> Result<ZoneSummary, device_health_service::Error> {
        ZoneSummary::new([90, 120, 150, 180, 220], [1, 2, 3, 4, 5])
    }
    fn pending(&self, _: u32) -> Result<Option<(u64, ZoneSummary)>, device_health_service::Error> {
        Ok(Some((1, self.snapshot(1)?)))
    }
    fn confirm(&mut self, _: u32, _: u64) -> Result<(), device_health_service::Error> {
        Ok(())
    }
    fn current(&self, _: u32, _: u64) -> bool {
        self.0.get()
    }
}
#[test]
fn test_should_reject_zone_event_after_actual_context_revision_expires() {
    let starts = Cell::new(0);
    let owner =
        SharedHealthOwner::new(JobManager::<1>::new(1).unwrap(), Actual { starts: &starts });
    let auth = Auth(Cell::new(true));
    let valid = Cell::new(true);
    let mut zones = Zones(&valid);
    let mut store = Store;
    let mut h = HealthHandler::new(
        &owner,
        1,
        &[ENTRY],
        &auth,
        &Clock,
        &Product,
        &mut store,
        Some(&mut zones),
    )
    .unwrap();
    let mut out = [0; 128];
    let (token, n) = h.prepare_zones(7, &mut out, 128).unwrap().unwrap();
    assert_eq!(n, 35);
    assert_eq!(&out[..5], &[7, 0, 0, 0, 31]);
    valid.set(false);
    assert!(h.confirm_event(token).is_err());
}
#[test]
fn test_should_validate_zone_wire_and_real_query_port() {
    let z = ZoneSummary::new([90, 120, 150, 180, 220], [1, 2, 3, 4, 5]).unwrap();
    let mut b = [0; 31];
    assert_eq!(z.encode(&mut b), Ok(31));
    assert_eq!(ZoneSummary::decode(&b), Ok(z));
    b[0] = 0xff;
    assert!(ZoneSummary::decode(&b).is_err());
    assert!(ZoneSummary::new([90, 90, 150, 180, 220], [0; 5]).is_err());
}
#[test]
fn test_should_reject_start_capacity_and_malformed_body_before_backend_side_effect() {
    let starts = Cell::new(0);
    let owner =
        SharedHealthOwner::new(JobManager::<2>::new(1).unwrap(), Actual { starts: &starts });
    let auth = Auth(Cell::new(true));
    let mut store = Store;
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
    .unwrap();
    assert_eq!(
        h.dispatch(request(0x30, 2, &[1, 0x20, 0, 0, 0, 0, 0, 0]), &mut [0; 7]),
        Err(device_services::SessionError::Capacity)
    );
    let mut out = [0; 128];
    assert_eq!(
        h.dispatch(request(0x30, 2, &[1, 0x20, 0, 0, 0, 0, 0]), &mut out),
        Ok(4)
    );
    assert_eq!(out[0], 1);
    assert_eq!(starts.get(), 0);
}
#[cfg(feature = "oxygen")]
#[test]
fn test_should_share_ppg_resource_and_job_id_space_across_health_modules() {
    let starts = Cell::new(0);
    let owner =
        SharedHealthOwner::new(JobManager::<2>::new(1).unwrap(), Actual { starts: &starts });
    let auth = Auth(Cell::new(true));
    let mut store = Store;
    let entries = [
        ENTRY,
        ModuleEntry {
            module: 0x26,
            ..ENTRY
        },
    ];
    let mut h = HealthHandler::new(
        &owner, 1, &entries, &auth, &Clock, &Product, &mut store, None,
    )
    .unwrap();
    let mut out = [0; 128];
    start(&mut h, &mut out);
    let mut f = request(0x30, 3, &[1, 1, 0x26, 0, 0, 0, 0, 0]);
    f.header.module = 0x26;
    assert_eq!(h.dispatch(f, &mut out), Ok(4));
    assert_eq!(out[0], 6);
    assert_eq!(starts.get(), 1);
}
struct WrongSource(Cell<bool>);
impl HealthProduct for WrongSource {
    fn declaration(
        &self,
        m: u8,
        t: u16,
    ) -> Result<ProductDeclaration<'_>, device_health_service::Error> {
        let mut d = Product.declaration(m, t)?;
        if self.0.get() {
            d.acquisition = AcquisitionKind::PhoneImport;
        }
        Ok(d)
    }
    fn provenance(
        &self,
        m: u8,
        t: u16,
        _: ResultSource,
    ) -> Result<Provenance<'_>, device_health_service::Error> {
        let mut p = Product.provenance(m, t, ResultSource::PhoneImport)?;
        p.acquisition = AcquisitionKind::PhoneImport;
        Ok(p)
    }
    fn start_binding(
        &self,
        r: HealthStart,
    ) -> Result<(Owner, TriggerSource), device_health_service::Error> {
        Product.start_binding(r)
    }
}
#[test]
fn test_should_reject_provider_reclassifying_actual_backend_source() {
    let starts = Cell::new(0);
    let acks = Cell::new(0);
    let stops = Cell::new(0);
    let sample = Cell::new(true);
    let done = Cell::new(false);
    let owner = SharedHealthOwner::new(
        JobManager::<2>::new(1).unwrap(),
        Stream {
            starts: &starts,
            acks: &acks,
            stops: &stops,
            sample: &sample,
            done: &done,
        },
    );
    let auth = Auth(Cell::new(true));
    let mut store = Store;
    let product = WrongSource(Cell::new(false));
    let mut h = HealthHandler::new(
        &owner,
        1,
        &[ENTRY],
        &auth,
        &Clock,
        &product,
        &mut store,
        None,
    )
    .unwrap();
    let mut out = [0; 128];
    start(&mut h, &mut out);
    h.response_admitted(
        request(0x30, 2, &[1, 0x20, 0, 0, 0, 0, 0, 0]),
        &[0, 0, 0, 0, 1, 0, 0, 0],
    )
    .unwrap();
    product.0.set(true);
    assert!(h.prepare_result(0x20, 1, &mut out, 128).is_err());
    assert_eq!(acks.get(), 0);
}
#[test]
fn test_should_not_repeat_start_effect_when_session_replays_cached_response() {
    use device_protocol::ProtocolLimits;
    use device_services::{BuildProfile, Config, DeviceInfo, Session};
    let starts = Cell::new(0);
    let owner =
        SharedHealthOwner::new(JobManager::<2>::new(1).unwrap(), Actual { starts: &starts });
    let auth = Auth(Cell::new(true));
    let mut store = Store;
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
    .unwrap();
    let modules = [
        ModuleEntry {
            module: 0,
            major: 1,
            minor: 2,
            ..ENTRY
        },
        ENTRY,
    ];
    let mut s = Session::new(Config {
        session: 1,
        capability_rev: 1,
        profile: BuildProfile::Commercial,
        info: DeviceInfo {
            product_id: 5,
            fw_major: 1,
            fw_minor: 0,
            fw_patch: 0,
            build_id: 1,
        },
        modules: &modules,
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
    .unwrap();
    s.control_notifications_enabled().unwrap();
    let mut hello = request(1, 1, &[1, 1, 128, 0, 0, 0, 3, 0]);
    hello.header.module = 0;
    let mut out = [0; 128];
    s.dispatch_with_handler(hello, &mut h, &mut out).unwrap();
    let f = request(0x30, 2, &[1, 0x20, 0, 0, 0, 0, 0, 0]);
    assert_eq!(s.dispatch_with_handler(f, &mut h, &mut out), Ok(8));
    assert_eq!(s.dispatch_with_handler(f, &mut h, &mut out), Ok(8));
    assert_eq!(starts.get(), 1);
    h.response_admitted(f, &out[..8]).unwrap();
    assert_eq!(s.dispatch_with_handler(f, &mut h, &mut out), Ok(8));
    assert_eq!(starts.get(), 1);
}
#[test]
fn test_should_defer_stop_until_an_already_prepared_result_is_actually_admitted() {
    let starts = Cell::new(0);
    let acks = Cell::new(0);
    let stops = Cell::new(0);
    let sample = Cell::new(true);
    let done = Cell::new(false);
    let owner = SharedHealthOwner::new(
        JobManager::<2>::new(1).unwrap(),
        Stream {
            starts: &starts,
            acks: &acks,
            stops: &stops,
            sample: &sample,
            done: &done,
        },
    );
    let auth = Auth(Cell::new(true));
    let mut store = Store;
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
    .unwrap();
    let mut out = [0; 128];
    start(&mut h, &mut out);
    h.response_admitted(
        request(0x30, 2, &[1, 0x20, 0, 0, 0, 0, 0, 0]),
        &[0, 0, 0, 0, 1, 0, 0, 0],
    )
    .unwrap();
    let (token, _) = h.prepare_result(0x20, 1, &mut out, 128).unwrap().unwrap();
    let stop = request(0x31, 3, &[1, 0, 0, 0]);
    assert_eq!(h.response_ready(stop), Ok(false));
    assert_eq!(stops.get(), 0);
    h.confirm_event(token).unwrap();
    assert_eq!(h.response_ready(stop), Ok(true));
    assert_eq!(h.dispatch(stop, &mut out), Ok(4));
    assert_eq!(stops.get(), 1);
}
struct Mono(Cell<u64>);
impl HealthClock for Mono {
    fn now_ms(&self) -> Result<u64, device_health_service::Error> {
        Ok(self.0.get())
    }
}
#[test]
fn test_should_end_owned_work_when_trusted_clock_regresses() {
    let starts = Cell::new(0);
    let acks = Cell::new(0);
    let stops = Cell::new(0);
    let sample = Cell::new(false);
    let done = Cell::new(false);
    let owner = SharedHealthOwner::new(
        JobManager::<2>::new(1).unwrap(),
        Stream {
            starts: &starts,
            acks: &acks,
            stops: &stops,
            sample: &sample,
            done: &done,
        },
    );
    let auth = Auth(Cell::new(true));
    let clock = Mono(Cell::new(100));
    let mut store = Store;
    let mut h = HealthHandler::new(
        &owner,
        1,
        &[ENTRY],
        &auth,
        &clock,
        &Product,
        &mut store,
        None,
    )
    .unwrap();
    let mut out = [0; 128];
    start(&mut h, &mut out);
    clock.0.set(99);
    assert!(
        h.dispatch(request(0x32, 3, &[1, 0, 0, 0]), &mut out)
            .is_err()
    );
    assert!(h.must_end_session());
    assert_eq!(stops.get(), 1);
}
#[test]
fn test_should_return_actual_source_fault_status_without_deriving_ready_from_idle_jobs() {
    let starts = Cell::new(0);
    let owner =
        SharedHealthOwner::new(JobManager::<2>::new(1).unwrap(), Actual { starts: &starts });
    let auth = Auth(Cell::new(true));
    let mut store = Store;
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
    .unwrap();
    let mut out = [0; 128];
    assert_eq!(h.dispatch(request(1, 2, &[]), &mut out), Ok(7));
    assert_eq!(&out[..7], &[0, 0, 0, 0, 3, 5, 1]);
    assert_eq!(starts.get(), 0);
}

impl HealthSource for Product {
    fn module_run_status(&self, _: u8) -> Result<ModuleRunStatus, device_health_service::Error> {
        Ok(ModuleRunStatus {
            state: 3,
            reason: 0x105,
        })
    }
}
impl HealthSource for WrongSource {
    fn module_run_status(&self, m: u8) -> Result<ModuleRunStatus, device_health_service::Error> {
        Product.module_run_status(m)
    }
}
struct InvalidDeclaration;
impl HealthSource for InvalidDeclaration {
    fn module_run_status(&self, m: u8) -> Result<ModuleRunStatus, device_health_service::Error> {
        Product.module_run_status(m)
    }
}
impl HealthProduct for InvalidDeclaration {
    fn declaration(
        &self,
        m: u8,
        t: u16,
    ) -> Result<ProductDeclaration<'_>, device_health_service::Error> {
        let mut d = Product.declaration(m, t)?;
        d.temperature_modes = 1;
        Ok(d)
    }
    fn provenance(
        &self,
        m: u8,
        t: u16,
        s: ResultSource,
    ) -> Result<Provenance<'_>, device_health_service::Error> {
        Product.provenance(m, t, s)
    }
    fn start_binding(
        &self,
        r: HealthStart,
    ) -> Result<(Owner, TriggerSource), device_health_service::Error> {
        Product.start_binding(r)
    }
}
#[test]
fn test_should_reject_invalid_product_manifest_before_actual_backend_start() {
    let starts = Cell::new(0);
    let owner =
        SharedHealthOwner::new(JobManager::<1>::new(1).unwrap(), Actual { starts: &starts });
    let auth = Auth(Cell::new(true));
    let mut store = Store;
    let mut h = HealthHandler::new(
        &owner,
        1,
        &[ENTRY],
        &auth,
        &Clock,
        &InvalidDeclaration,
        &mut store,
        None,
    )
    .unwrap();
    let mut out = [0; 128];
    assert_eq!(
        h.dispatch(request(0x30, 2, &[1, 0x20, 0, 0, 0, 0, 0, 0]), &mut out),
        Ok(4)
    );
    assert_eq!(out[0], 1);
    assert_eq!(starts.get(), 0);
}
#[test]
fn test_should_return_business_limit_when_all_connection_job_slots_are_owned() {
    let starts = Cell::new(0);
    let owner =
        SharedHealthOwner::new(JobManager::<1>::new(1).unwrap(), Actual { starts: &starts });
    let auth = Auth(Cell::new(true));
    let mut store = Store;
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
    .unwrap();
    let mut out = [0; 128];
    start(&mut h, &mut out);
    h.response_admitted(
        request(0x30, 2, &[1, 0x20, 0, 0, 0, 0, 0, 0]),
        &[0, 0, 0, 0, 1, 0, 0, 0],
    )
    .unwrap();
    assert_eq!(
        h.dispatch(request(0x30, 3, &[1, 0x20, 0, 0, 0, 0, 0, 0]), &mut out),
        Ok(4)
    );
    assert_eq!(&out[..4], &[7, 0, 0, 0]);
    assert_eq!(starts.get(), 1);
}

#[test]
fn test_should_bound_event_workspace_by_declared_module_output_capacity() {
    let starts = Cell::new(0);
    let owner =
        SharedHealthOwner::new(JobManager::<1>::new(1).unwrap(), Actual { starts: &starts });
    let auth = Auth(Cell::new(true));
    let mut store = Store;
    let too_large = [ModuleEntry {
        output_max: 256,
        ..ENTRY
    }];
    assert!(
        HealthHandler::<_, 1, 128>::new_bounded(
            &owner, 1, &too_large, &auth, &Clock, &Product, &mut store, None
        )
        .is_err()
    );
    // Failed preflight must not claim this connection identity or manager.
    let mut handler = HealthHandler::<_, 1, 128>::new_bounded(
        &owner,
        1,
        &[ENTRY],
        &auth,
        &Clock,
        &Product,
        &mut store,
        None,
    )
    .unwrap();
    handler.disconnect().unwrap();
    assert_eq!(starts.get(), 0);
}

#[test]
fn test_should_deliver_and_retry_result_with_bounded_event_workspace() {
    let starts = Cell::new(0);
    let acks = Cell::new(0);
    let stops = Cell::new(0);
    let sample = Cell::new(true);
    let done = Cell::new(false);
    let owner = SharedHealthOwner::new(
        JobManager::<2>::new(1).unwrap(),
        Stream {
            starts: &starts,
            acks: &acks,
            stops: &stops,
            sample: &sample,
            done: &done,
        },
    );
    let auth = Auth(Cell::new(true));
    let version = Cell::new(1);
    let mut store = Memory {
        version: &version,
        value: std::cell::RefCell::new(None),
    };
    let mut h = HealthHandler::<_, 2, 128>::new_bounded(
        &owner,
        1,
        &[ENTRY],
        &auth,
        &Clock,
        &Product,
        &mut store,
        None,
    )
    .unwrap();
    let request = request(0x30, 2, &[1, 0x20, 0, 0, 0, 0, 0, 0]);
    let mut out = [0; 128];
    assert_eq!(h.dispatch(request, &mut out), Ok(8));
    h.response_admitted(request, &[0, 0, 0, 0, 1, 0, 0, 0])
        .unwrap();
    let (token, length) = h.prepare_result(0x20, 1, &mut out, 128).unwrap().unwrap();
    assert_eq!(
        &out[..length],
        &[
            0x20, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 0, 60, 0, 100, 1
        ]
    );
    assert_eq!(
        h.prepare_result(0x20, 1, &mut out, 128).unwrap(),
        Some((token, length))
    );
    assert_eq!(acks.get(), 0);
    h.confirm_event(token).unwrap();
    h.confirm_event(token).unwrap();
    assert_eq!(acks.get(), 1);
}
