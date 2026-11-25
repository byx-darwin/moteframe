#![cfg(feature = "heart-rate")]
#![allow(clippy::unwrap_used, clippy::expect_used)]
//! A phone-import provider is never an independent measurement capability.
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
struct Product<'a> {
    imported: &'a Cell<bool>,
    import_during_binding: bool,
}
impl HealthProduct for Product<'_> {
    fn declaration(
        &self,
        m: u8,
        t: u16,
    ) -> Result<ProductDeclaration<'_>, device_health_service::Error> {
        Ok(ProductDeclaration {
            module: Module::from_wire(m)?,
            record_type: t,
            acquisition: if self.imported.get() {
                AcquisitionKind::PhoneImport
            } else {
                AcquisitionKind::SensorMeasurement
            },
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
            acquisition: if self.imported.get() {
                AcquisitionKind::PhoneImport
            } else {
                AcquisitionKind::SensorMeasurement
            },
            algorithm_id: "actual-fixture",
            algorithm_version: "1",
        })
    }
    fn start_binding(
        &self,
        _: HealthStart,
    ) -> Result<(Owner, TriggerSource), device_health_service::Error> {
        if self.import_during_binding {
            self.imported.set(true);
        }
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
    imported: &'a Cell<bool>,
    import_during_resources: bool,
    starts: &'a Cell<u32>,
}
impl Backend for Actual<'_> {
    fn resources(&self, _: HealthStart) -> Result<ResourceSet, BackendError> {
        if self.import_during_resources {
            self.imported.set(true);
        }
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

impl HealthSource for Product<'_> {
    fn module_run_status(&self, _: u8) -> Result<ModuleRunStatus, device_health_service::Error> {
        Ok(ModuleRunStatus {
            state: 1,
            reason: 0,
        })
    }
}
fn probe(initially_imported: bool, import_during_binding: bool, import_during_resources: bool) {
    let starts = Cell::new(0);
    let imported = Cell::new(initially_imported);
    let owner = SharedHealthOwner::new(
        JobManager::<2>::new(1).unwrap(),
        Actual {
            starts: &starts,
            imported: &imported,
            import_during_resources,
        },
    );
    let auth = Auth(Cell::new(true));
    let product = Product {
        imported: &imported,
        import_during_binding,
    };
    let mut store = Store;
    let mut handler = HealthHandler::new(
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
    let mut out = [0u8; 128];
    assert_eq!(
        handler.dispatch(request(0x30, 2, &[1, 0x20, 0, 0, 0, 0, 0, 0]), &mut out),
        Ok(4)
    );
    assert_eq!(&out[..4], &[3, 0, 0, 0]);
    assert_eq!(
        starts.get(),
        0,
        "phone-import-only declaration must never reach backend.start"
    );
}
#[test]
fn test_should_reject_phone_import_only_measurement_start() {
    probe(true, false, false);
}
#[test]
fn test_should_recheck_measurement_source_after_product_binding() {
    probe(false, true, false);
}
#[test]
fn test_should_recheck_measurement_source_after_backend_resources() {
    probe(false, false, true);
}

struct ImportedLatest;
impl LatestStore for ImportedLatest {
    fn load(
        &self,
        _: u8,
        _: u16,
        out: &mut [u8],
    ) -> Result<StoredLatest, device_health_service::Error> {
        out[..4].copy_from_slice(&[60, 0, 100, 1]);
        Ok(StoredLatest {
            generation: 1,
            info: ResultInfo {
                sample_id: 1,
                record_type: 0x20,
                schema: 1,
                timestamp: Timestamp::unknown(),
                length: 4,
                source: ResultSource::PhoneImport,
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
        Err(device_health_service::Error::Backend)
    }
    fn current(&self, _: u8, _: u16, generation: u64) -> bool {
        generation == 1
    }
}

#[test]
fn test_should_preserve_actual_imported_latest_without_starting_measurement() {
    let starts = Cell::new(0);
    let imported = Cell::new(true);
    let owner = SharedHealthOwner::new(
        JobManager::<2>::new(1).unwrap(),
        Actual {
            starts: &starts,
            imported: &imported,
            import_during_resources: false,
        },
    );
    let auth = Auth(Cell::new(true));
    let product = Product {
        imported: &imported,
        import_during_binding: false,
    };
    let mut store = ImportedLatest;
    let mut handler = HealthHandler::new(
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
    let mut out = [0u8; 128];
    assert_eq!(handler.dispatch(request(2, 2, &[]), &mut out), Ok(23));
    assert_eq!(&out[..4], &[0, 0, 0, 0]);
    assert_eq!(&out[19..23], &[60, 0, 100, 1]);
    assert_eq!(starts.get(), 0);
}
