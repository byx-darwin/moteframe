#![cfg(feature = "blood-pressure")]
//! Cuff reasons preserve the frozen query and END widths without requiring heart-rate selection.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use device_health::*;
use device_health_service::*;
use device_protocol::{Frame, Header, Kind, ModuleEntry};
use device_services::ServiceHandler;
use std::cell::Cell;
const ENTRY: ModuleEntry = ModuleEntry {
    module: 0x25,
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
fn request(opcode: u8, id: u16, payload: &[u8]) -> Frame<'_> {
    Frame {
        header: Header {
            kind: Kind::Request,
            module: 0x25,
            opcode,
            message_id: id,
            offset: 0,
            total_length: u16::try_from(payload.len()).unwrap(),
        },
        payload,
    }
}

impl HealthSource for Product {
    fn module_run_status(&self, _: u8) -> Result<ModuleRunStatus, device_health_service::Error> {
        Ok(ModuleRunStatus {
            state: 1,
            reason: 0,
        })
    }
}

#[cfg(feature = "blood-pressure")]
struct CuffFailure(Reason);
#[cfg(feature = "blood-pressure")]
impl Backend for CuffFailure {
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
        Ok(Some(Outcome::Failed(self.0)))
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
#[cfg(feature = "blood-pressure")]
#[test]
fn test_should_preserve_each_cuff_reason_in_query_and_exact_ten_byte_end() {
    for raw in 1u8..=7 {
        let reason = Reason::from_legacy_cuff_error(raw).unwrap();
        let owner = SharedHealthOwner::new(JobManager::<1>::new(1).unwrap(), CuffFailure(reason));
        let auth = Auth(Cell::new(true));
        let mut store = Store;
        let entries = [ModuleEntry {
            module: 0x25,
            ..ENTRY
        }];
        let mut h = HealthHandler::new(
            &owner, 1, &entries, &auth, &Clock, &Product, &mut store, None,
        )
        .unwrap();
        let start = [1, 1, 0x25, 0, 0, 0, 0, 0];
        let mut command = request(0x30, 2, &start);
        command.header.module = 0x25;
        let mut out = [0; 128];
        assert_eq!(h.dispatch(command, &mut out), Ok(8));
        h.response_admitted(command, &out[..8]).unwrap();
        assert_eq!(h.poll_job(0x25, 1), Ok(JobState::Failed));
        let mut query = request(0x32, 3, &[1, 0, 0, 0]);
        query.header.module = 0x25;
        assert_eq!(h.dispatch(query, &mut out), Ok(7));
        assert_eq!(&out[5..7], &(reason as u16).to_le_bytes());
        let (_, length) = h.prepare_end(0x25, 1, &mut out, 128).unwrap().unwrap();
        assert_eq!(length, 10);
        assert_eq!(&out[8..10], &(reason as u16).to_le_bytes());
    }
}
