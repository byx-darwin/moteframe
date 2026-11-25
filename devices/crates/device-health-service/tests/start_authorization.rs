#![cfg(feature = "heart-rate")]
#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Permanent regressions copied from independently authored final review health probes.
use core::cell::Cell;
use device_protocol::{Frame, Header, Kind, ModuleEntry};
use device_services::{ServiceHandler, SessionError};
const HEART: ModuleEntry = ModuleEntry {
    module: 0x20,
    major: 1,
    minor: 0,
    availability: 1,
    features: 0,
    input_max: 128,
    output_max: 128,
};
fn request(op: u8, bytes: &[u8]) -> Frame<'_> {
    Frame {
        header: Header {
            kind: Kind::Request,
            module: 0x20,
            opcode: op,
            message_id: 1,
            offset: 0,
            total_length: u16::try_from(bytes.len()).unwrap(),
        },
        payload: bytes,
    }
}

use device_health::{
    AcquisitionKind, Backend, BackendError, HealthStart, JobId, JobManager, Module, Outcome, Owner,
    ProductDeclaration, Provenance, ResourceSet, ResultInfo, ResultSource, StopProgress,
    TriggerSource,
};
use device_health_service::{
    Error, HealthAccess, HealthClock, HealthHandler, HealthProduct, HealthSource, LatestStore,
    ModuleRunStatus, Operation, SharedHealthOwner, StoredLatest,
};
struct Access(Cell<bool>);
impl HealthAccess for Access {
    fn allowed(&self, _: u8, _: Operation, _: Option<u32>) -> bool {
        self.0.get()
    }
}
struct Clock;
impl HealthClock for Clock {
    fn now_ms(&self) -> Result<u64, Error> {
        Ok(1)
    }
}
struct Product<'a> {
    access: &'a Access,
    revoke: bool,
}
impl HealthSource for Product<'_> {
    fn module_run_status(&self, _: u8) -> Result<ModuleRunStatus, Error> {
        Ok(ModuleRunStatus {
            state: 1,
            reason: 0,
        })
    }
}
impl HealthProduct for Product<'_> {
    fn declaration(&self, m: u8, t: u16) -> Result<ProductDeclaration<'_>, Error> {
        Ok(ProductDeclaration {
            module: Module::from_wire(m)?,
            record_type: t,
            acquisition: AcquisitionKind::SensorMeasurement,
            algorithm_id: "review-fixture",
            algorithm_version: "1",
            temperature_modes: 0,
            mood_labels: &[],
        })
    }
    fn provenance(&self, _: u8, _: u16, source: ResultSource) -> Result<Provenance<'_>, Error> {
        Ok(Provenance {
            source,
            acquisition: AcquisitionKind::SensorMeasurement,
            algorithm_id: "review-fixture",
            algorithm_version: "1",
        })
    }
    fn start_binding(&self, _: HealthStart) -> Result<(Owner, TriggerSource), Error> {
        if self.revoke {
            self.access.0.set(false);
        }
        Ok((Owner::Application, TriggerSource::App))
    }
}
struct Port<'a> {
    access: &'a Access,
    revoke_resources: bool,
    revoke_context: bool,
    starts: &'a Cell<u32>,
    unauthorized_starts: &'a Cell<u32>,
}
impl Backend for Port<'_> {
    fn resources(&self, _: HealthStart) -> Result<ResourceSet, BackendError> {
        if self.revoke_resources {
            self.access.0.set(false);
        }
        Ok(ResourceSet::from_bits(1))
    }
    fn context_is_valid(&self, _: u32) -> bool {
        if self.revoke_context {
            self.access.0.set(false);
        }
        true
    }
    fn start(&mut self, _: JobId, _: HealthStart) -> Result<(), BackendError> {
        self.starts.set(self.starts.get() + 1);
        if !self.access.0.get() {
            self.unauthorized_starts
                .set(self.unauthorized_starts.get() + 1);
        }
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
struct Store;
impl LatestStore for Store {
    fn load(&self, _: u8, _: u16, _: &mut [u8]) -> Result<StoredLatest, Error> {
        Err(Error::Health(device_health::Error::NotFound))
    }
    fn publish(&mut self, _: u8, _: u32, _: ResultInfo, _: &[u8]) -> Result<(), Error> {
        Ok(())
    }
    fn current(&self, _: u8, _: u16, _: u64) -> bool {
        true
    }
}
fn probe(revoke_product: bool, revoke_resources: bool, revoke_context: bool) {
    let access = Access(Cell::new(true));
    let starts = Cell::new(0);
    let unauthorized = Cell::new(0);
    let owner = SharedHealthOwner::new(
        JobManager::<2>::new(1).unwrap(),
        Port {
            access: &access,
            revoke_resources,
            revoke_context,
            starts: &starts,
            unauthorized_starts: &unauthorized,
        },
    );
    let product = Product {
        access: &access,
        revoke: revoke_product,
    };
    let mut store = Store;
    let entries = [HEART];
    let mut handler = HealthHandler::new(
        &owner, 1, &entries, &access, &Clock, &product, &mut store, None,
    )
    .unwrap();
    let mut out = [0xa5; 128];
    let result = handler.dispatch(
        request(0x30, &[1, 0x20, 0, u8::from(revoke_context), 0, 0, 0, 0]),
        &mut out,
    );
    assert_eq!(result, Err(SessionError::AuthorizationRevoked));
    assert_eq!(out, [0; 128]);
    assert_eq!(
        unauthorized.get(),
        0,
        "actual backend.start was invoked after trusted Start scope had already been revoked; total starts={}",
        starts.get()
    );
}
#[test]
fn final_review_health_start_rechecks_after_product_policy() {
    probe(true, false, false);
}
#[test]
fn final_review_health_start_rechecks_after_backend_capability() {
    probe(false, true, false);
}

#[test]
fn start_must_recheck_scope_after_actual_context_validation() {
    probe(false, false, true);
}
struct SpecificAccess<'a>(&'a Access);
impl HealthAccess for SpecificAccess<'_> {
    fn allowed(&self, _: u8, operation: Operation, id: Option<u32>) -> bool {
        self.0.0.get() && !(operation == Operation::Start && id.is_some())
    }
}
#[test]
fn start_must_check_allocated_job_scope_before_actual_physical_start() {
    let access = Access(Cell::new(true));
    let starts = Cell::new(0);
    let unauthorized = Cell::new(0);
    let owner = SharedHealthOwner::new(
        JobManager::<2>::new(1).unwrap(),
        Port {
            access: &access,
            revoke_resources: false,
            revoke_context: false,
            starts: &starts,
            unauthorized_starts: &unauthorized,
        },
    );
    let product = Product {
        access: &access,
        revoke: false,
    };
    let mut store = Store;
    let entries = [HEART];
    let specific = SpecificAccess(&access);
    let mut handler = HealthHandler::new(
        &owner, 1, &entries, &specific, &Clock, &product, &mut store, None,
    )
    .unwrap();
    let mut out = [0xa5; 128];
    assert_eq!(
        handler.dispatch(request(0x30, &[1, 0x20, 0, 0, 0, 0, 0, 0]), &mut out),
        Err(SessionError::AuthorizationRevoked)
    );
    assert_eq!(out, [0; 128]);
    assert_eq!(
        starts.get(),
        0,
        "job-specific Start denial must precede the physical start"
    );
}
