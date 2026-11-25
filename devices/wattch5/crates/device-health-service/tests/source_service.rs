#![cfg(feature = "blood-pressure")]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use device_health::*;
use device_health_service::*;
use device_protocol::{Frame, Header, Kind, ModuleEntry};
use device_services::ServiceHandler;
struct Allow;
impl HealthAccess for Allow {
    fn allowed(&self, _: u8, _: Operation, _: Option<u32>) -> bool {
        true
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
            acquisition: AcquisitionKind::DerivedEstimate,
            algorithm_id: "legacy-bp",
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
            acquisition: AcquisitionKind::DerivedEstimate,
            algorithm_id: "legacy-bp",
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
struct NoBackend;
impl Backend for NoBackend {
    fn context_is_valid(&self, _: u32) -> bool {
        false
    }
    fn has_pending_result(&self, _: JobId) -> Result<bool, BackendError> {
        Ok(false)
    }
    fn resources(&self, _: HealthStart) -> Result<ResourceSet, BackendError> {
        Err(BackendError::Unavailable)
    }
    fn start(&mut self, _: JobId, _: HealthStart) -> Result<(), BackendError> {
        Err(BackendError::Unavailable)
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
        Ok(())
    }
}
struct Store;
impl LatestStore for Store {
    fn load(
        &self,
        _: u8,
        _: u16,
        out: &mut [u8],
    ) -> Result<StoredLatest, device_health_service::Error> {
        out[..9].copy_from_slice(&[120, 0, 80, 0, 60, 0, 7, 0, 1]);
        Ok(StoredLatest {
            generation: 1,
            info: ResultInfo {
                sample_id: 1,
                record_type: 0x2501,
                schema: 2,
                timestamp: Timestamp::unknown(),
                length: 9,
                source: ResultSource::DeviceAlgorithm,
            },
        })
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
    fn current(&self, _: u8, _: u16, g: u64) -> bool {
        g == 1
    }
}
const ENTRIES: [ModuleEntry; 1] = [ModuleEntry {
    module: 0x25,
    major: 1,
    minor: 0,
    availability: 1,
    features: 0,
    input_max: 128,
    output_max: 128,
}];
fn frame(op: u8, bytes: &[u8]) -> Frame<'_> {
    Frame {
        header: Header {
            kind: Kind::Request,
            module: 0x25,
            opcode: op,
            message_id: 1,
            offset: 0,
            total_length: u16::try_from(bytes.len()).unwrap(),
        },
        payload: bytes,
    }
}
#[test]
fn test_should_send_schema2_latest_without_losing_the_source_byte() {
    let owner = SharedHealthOwner::new(JobManager::<1>::new(1).unwrap(), NoBackend);
    let mut store = Store;
    let mut h = HealthHandler::new(
        &owner, 1, &ENTRIES, &Allow, &Clock, &Product, &mut store, None,
    )
    .unwrap();
    let mut out = [0; 128];
    let n = h.dispatch(frame(2, &[]), &mut out).unwrap();
    assert_eq!(&out[..4], &[0, 0, 0, 0]);
    assert_eq!(n, 28);
    assert_eq!(&out[19..n], &[120, 0, 80, 0, 60, 0, 7, 0, 1]);
}

#[test]
fn test_should_accept_authorized_app_import_as_a_health_operation() {
    let owner = SharedHealthOwner::new(JobManager::<1>::new(1).unwrap(), NoBackend);
    let mut store = Store;
    let inner = HealthHandler::new(
        &owner, 1, &ENTRIES, &Allow, &Clock, &Product, &mut store, None,
    )
    .unwrap();
    let port = CommittedImport;
    let mut h = ImportHealthHandler::new(inner, 1, &port, &Allow).unwrap();
    let mut out = [0; 128];
    let mut request = [0; 19];
    request[..2].copy_from_slice(&[1, 0]);
    request[11..].copy_from_slice(&[118, 0, 78, 0, 0, 0, 3, 0]);
    assert!(h.response_ready(frame(0x33, &request)).unwrap());
    let n = h.dispatch(frame(0x33, &request), &mut out).unwrap();
    assert_eq!(&out[..4], &[0, 0, 0, 0]);
    assert_eq!(n, 12);
}

struct CommittedImport;
impl HealthImportPort for CommittedImport {
    fn available(&self, _: u8) -> bool {
        true
    }
    fn prepare(
        &self,
        _: ImportKey,
        r: HealthImport<'_>,
    ) -> Result<bool, device_health_service::Error> {
        assert_eq!(r.value, &[118, 0, 78, 0, 0, 0, 3, 0]);
        Ok(true)
    }
    fn receipt(
        &self,
        _: ImportKey,
        _: HealthImport<'_>,
    ) -> Result<u64, device_health_service::Error> {
        Ok(3)
    }
    fn current(&self, _: ImportKey, r: u64) -> bool {
        r == 3
    }
    fn detach(&self, _: ImportKey) {}
}
#[test]
fn test_should_reject_peer_supplied_sources_and_absent_import_results() {
    let mut wire = [0; 19];
    wire[..2].copy_from_slice(&[1, 0]);
    wire[11..].copy_from_slice(&[118, 0, 78, 0, 0, 0, 3, 0]);
    assert!(HealthImport::decode(0x25, &wire).is_ok());
    let mut extra = wire.to_vec();
    extra.push(1);
    assert!(HealthImport::decode(0x25, &extra).is_err());
    wire[0] = 2;
    assert!(HealthImport::decode(0x25, &wire).is_err());
    wire[0] = 1;
    wire[2] = 1;
    assert!(HealthImport::decode(0x25, &wire).is_err());
    wire[2] = 0;
    wire[11..].fill(0);
    assert!(HealthImport::decode(0x25, &wire).is_err());
}
