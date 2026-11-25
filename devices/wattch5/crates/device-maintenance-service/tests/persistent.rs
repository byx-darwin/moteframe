#![cfg(any(feature = "engineering", feature = "factory-config"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use device_maintenance::{Authority, Capability, Profile, persistent::*};
use device_maintenance_service::*;
use device_protocol::{Frame, Header, Kind, ModuleEntry};
use device_services::ServiceHandler;
use std::cell::Cell;
struct Auth;
impl Authority for Auth {
    fn allows(&self, _: Capability) -> bool {
        true
    }
    fn station_id(&self) -> Option<u32> {
        Some(42)
    }
}
struct Time;
impl Clock for Time {
    fn now_ms(&self) -> Result<u64, Error> {
        Ok(1)
    }
}
struct Schema;
impl OperationSchema for Schema {
    fn validate(
        &self,
        _: NamedOperation<'_>,
    ) -> Result<OperationPolicy, device_maintenance::Error> {
        Ok(OperationPolicy {
            revision: 1,
            dangerous: false,
        })
    }
}
struct Store<'a>(&'a Cell<u32>);
impl StorageBackend for Store<'_> {
    fn guarantees(&self) -> Guarantees {
        Guarantees {
            atomic_claim: Guarantee::Implemented,
            durable_identity: Guarantee::Implemented,
            durable_terminal: Guarantee::Implemented,
            recovery_ready: Guarantee::Implemented,
            retained_terminal_records: 32,
        }
    }
    fn available(&self, _: u8, _: u16) -> bool {
        true
    }
    fn claim_atomic(
        &mut self,
        op: NamedOperation<'_>,
        _: OperationPolicy,
        _: &dyn OperationSchema,
        _: &dyn Authority,
    ) -> Result<Claim, device_maintenance::Error> {
        self.0.set(self.0.get() + 1);
        Ok(Claim {
            record: record(op.module, op.operation_id, op.operation),
            replayed: false,
        })
    }
    fn query(
        &self,
        m: u8,
        id: [u8; 16],
        _: &dyn Authority,
    ) -> Result<PersistentRecord, device_maintenance::Error> {
        Ok(record(m, id, 1))
    }
    fn output(
        &self,
        _: u8,
        _: [u8; 16],
        _: &mut [u8],
        _: &dyn Authority,
    ) -> Result<OutputMeta, device_maintenance::Error> {
        Err(device_maintenance::Error::Unavailable)
    }
}
fn record(module: u8, id: [u8; 16], operation: u16) -> PersistentRecord {
    PersistentRecord {
        operation_id: id,
        module,
        operation,
        schema: 1,
        job_id: 1,
        station_id: 42,
        policy_revision: 1,
        state: PersistentState::Accepted,
        result: 0,
        reason: 0,
    }
}
struct Executor<'a>(&'a Cell<u32>);
impl PersistentMaintenanceExecutor for Executor<'_> {
    fn available(&self, _: u8, _: u16) -> bool {
        true
    }
    fn authorized(&self, _: PersistentRecord, _: bool, _: &dyn Authority) -> bool {
        true
    }
    fn binding_current(&self, _: PersistentRecord) -> bool {
        true
    }
    fn response_admitted(
        &mut self,
        _: PersistentRecord,
        _: &dyn Authority,
    ) -> Result<(), device_maintenance::Error> {
        self.0.set(self.0.get() + 1);
        Ok(())
    }
}
#[test]
fn test_should_encode_maintenance_claim_before_starting_actual_executor() {
    let module = if cfg!(feature = "engineering") {
        0x72
    } else {
        0x73
    };
    let entry = ModuleEntry {
        module,
        major: 1,
        minor: 0,
        availability: 1,
        features: 0,
        input_max: 128,
        output_max: 128,
    };
    let claims = Cell::new(0);
    let begins = Cell::new(0);
    let mut store = Store(&claims);
    let mut executor = Executor(&begins);
    let mut h = PersistentMaintenanceHandler::new(
        entry,
        Profile::Factory,
        &mut store,
        &Schema,
        &mut executor,
        &Auth,
        &Time,
        false,
    )
    .unwrap();
    let mut b = [1; 24];
    b[16..].copy_from_slice(&[1, 0, 4, 0, 1, 0, 1, 0]);
    let f = Frame {
        header: Header {
            kind: Kind::Request,
            module,
            opcode: 0x40,
            message_id: 2,
            offset: 0,
            total_length: 24,
        },
        payload: &b,
    };
    let mut out = [0; 128];
    assert_eq!(h.dispatch(f, &mut out), Ok(8));
    assert_eq!(&out[..8], &[0, 0, 0, 0, 1, 0, 0, 0]);
    assert_eq!(claims.get(), 1);
    assert_eq!(begins.get(), 0);
    h.response_admitted(f, &out[..8]).unwrap();
    h.response_admitted(f, &out[..8]).unwrap();
    assert_eq!(begins.get(), 1);
    let query = Frame {
        header: Header {
            opcode: 0x41,
            message_id: 3,
            total_length: 16,
            ..f.header
        },
        payload: &[1; 16],
    };
    assert_eq!(h.dispatch(query, &mut out), Ok(9));
    assert_eq!(&out[..9], &[0, 0, 0, 0, 1, 0, 0, 0, 0]);
}
#[cfg(feature = "engineering")]
struct ExtraDanger;
#[cfg(feature = "engineering")]
impl OperationSchema for ExtraDanger {
    fn validate(
        &self,
        _: NamedOperation<'_>,
    ) -> Result<OperationPolicy, device_maintenance::Error> {
        Ok(OperationPolicy {
            revision: 1,
            dangerous: true,
        })
    }
}
#[cfg(feature = "engineering")]
struct MissingDanger(Cell<bool>);
#[cfg(feature = "engineering")]
impl Authority for MissingDanger {
    fn allows(&self, c: Capability) -> bool {
        c != Capability::DangerousMaintenance || self.0.get()
    }
    fn station_id(&self) -> Option<u32> {
        Some(42)
    }
}
#[cfg(feature = "engineering")]
#[test]
fn test_should_reject_same_schema_extra_danger_policy_before_engine_and_allow_later_authorized_retry()
 {
    let entry = ModuleEntry {
        module: 0x72,
        major: 1,
        minor: 0,
        availability: 1,
        features: 0,
        input_max: 128,
        output_max: 128,
    };
    let claims = Cell::new(0);
    let begins = Cell::new(0);
    let mut store = Store(&claims);
    let mut executor = Executor(&begins);
    let auth = MissingDanger(Cell::new(false));
    let mut h = PersistentMaintenanceHandler::new(
        entry,
        Profile::Factory,
        &mut store,
        &ExtraDanger,
        &mut executor,
        &auth,
        &Time,
        false,
    )
    .unwrap();
    let mut b = [1; 26];
    b[16..].copy_from_slice(&[3, 0, 6, 0, 1, 0, 1, 0, 0, 0]);
    let f = Frame {
        header: Header {
            kind: Kind::Request,
            module: 0x72,
            opcode: 0x40,
            message_id: 2,
            offset: 0,
            total_length: 26,
        },
        payload: &b,
    };
    let mut out = [0; 128];
    assert_eq!(h.dispatch(f, &mut out), Ok(4));
    assert_eq!(&out[..4], &[5, 0, 4, 255]);
    assert_eq!(claims.get(), 0);
    auth.0.set(true);
    assert_eq!(h.dispatch(f, &mut out), Ok(8));
    assert_eq!(claims.get(), 1);
}
#[cfg(feature = "factory-config")]
struct PermissiveOutputSchema;
#[cfg(feature = "factory-config")]
impl OperationSchema for PermissiveOutputSchema {
    fn validate(
        &self,
        _: NamedOperation<'_>,
    ) -> Result<OperationPolicy, device_maintenance::Error> {
        Ok(OperationPolicy {
            revision: 1,
            dangerous: false,
        })
    }
    fn validate_output(
        &self,
        _: u8,
        _: u16,
        _: u16,
        _: &[u8],
    ) -> Result<(), device_maintenance::Error> {
        Ok(())
    }
}
#[cfg(feature = "factory-config")]
struct MalformedOutput;
#[cfg(feature = "factory-config")]
impl StorageBackend for MalformedOutput {
    fn guarantees(&self) -> Guarantees {
        Store(&Cell::new(0)).guarantees()
    }
    fn available(&self, _: u8, _: u16) -> bool {
        true
    }
    fn claim_atomic(
        &mut self,
        _: NamedOperation<'_>,
        _: OperationPolicy,
        _: &dyn OperationSchema,
        _: &dyn Authority,
    ) -> Result<Claim, device_maintenance::Error> {
        Err(device_maintenance::Error::Unavailable)
    }
    fn query(
        &self,
        m: u8,
        id: [u8; 16],
        _: &dyn Authority,
    ) -> Result<PersistentRecord, device_maintenance::Error> {
        let mut actual = record(m, id, 4);
        actual.state = PersistentState::Succeeded;
        Ok(actual)
    }
    fn output(
        &self,
        _: u8,
        _: [u8; 16],
        out: &mut [u8],
        _: &dyn Authority,
    ) -> Result<OutputMeta, device_maintenance::Error> {
        out[0] = 2; // Invalid actual_enabled; the product schema deliberately misses it.
        Ok(OutputMeta {
            operation: 4,
            schema: 1,
            length: 1,
        })
    }
}
#[cfg(feature = "factory-config")]
#[test]
fn test_should_reject_malformed_typed_output_even_with_a_permissive_product_schema() {
    let entry = ModuleEntry {
        module: 0x73,
        major: 1,
        minor: 0,
        availability: 1,
        features: 0,
        input_max: 128,
        output_max: 128,
    };
    let mut store = MalformedOutput;
    let begins = Cell::new(0);
    let mut executor = Executor(&begins);
    let mut handler = PersistentMaintenanceHandler::new(
        entry,
        Profile::Factory,
        &mut store,
        &PermissiveOutputSchema,
        &mut executor,
        &Auth,
        &Time,
        true,
    )
    .unwrap();
    let request = Frame {
        header: Header {
            kind: Kind::Request,
            module: 0x73,
            opcode: 0x42,
            message_id: 1,
            offset: 0,
            total_length: 16,
        },
        payload: &[1; 16],
    };
    let mut out = [0xa5; 128];
    assert_eq!(handler.dispatch(request, &mut out), Ok(4));
    assert_eq!(&out[..4], &[9, 0, 0, 0]);
    assert!(out[4..].iter().all(|b| *b == 0));
    assert_eq!(begins.get(), 0);
}
