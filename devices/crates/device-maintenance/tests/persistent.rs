//! Atomic named-operation ledger admission; Memory is a test-only non-durable fixture.
#![cfg(any(feature = "engineering", feature = "factory-config"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use device_maintenance::{Authority, Capability, Error, Profile, persistent::*};
use std::cell::Cell;
struct Auth {
    allowed: Cell<bool>,
    dangerous: bool,
}
impl Authority for Auth {
    fn allows(&self, capability: Capability) -> bool {
        self.allowed.get() && (capability != Capability::DangerousMaintenance || self.dangerous)
    }
    fn station_id(&self) -> Option<u32> {
        Some(42)
    }
}
struct Schema;
impl OperationSchema for Schema {
    fn validate(&self, operation: NamedOperation<'_>) -> Result<OperationPolicy, Error> {
        if operation.operation != 1 || operation.params != [1, 0, 7, 0] {
            return Err(Error::Invalid);
        }
        Ok(OperationPolicy {
            revision: 3,
            dangerous: false,
        })
    }
    fn validate_output(
        &self,
        module: u8,
        operation: u16,
        revision: u16,
        bytes: &[u8],
    ) -> Result<(), Error> {
        if operation != 1 || revision != 3 {
            return Err(Error::Unavailable);
        }
        let expected: &[u8] = if module == 0x72 {
            &[7, 0, 0, 0]
        } else {
            &[7, 0]
        };
        if bytes != expected {
            return Err(Error::Invalid);
        }
        Ok(())
    }
}
struct Memory {
    records: Vec<(PersistentRecord, Vec<u8>)>,
    ready: bool,
    full: bool,
    claims: u32,
    execution: bool,
}
impl Memory {
    fn new() -> Self {
        Self {
            records: Vec::new(),
            ready: true,
            full: false,
            claims: 0,
            execution: true,
        }
    }
    fn reboot(&mut self) {
        for (r, _) in &mut self.records {
            if matches!(
                r.state,
                PersistentState::Accepted | PersistentState::Running
            ) {
                r.state = PersistentState::Interrupted;
                r.result = 15;
            }
        }
    }
}
impl StorageBackend for Memory {
    fn guarantees(&self) -> Guarantees {
        Guarantees {
            atomic_claim: Guarantee::Implemented,
            durable_identity: Guarantee::Implemented,
            durable_terminal: Guarantee::Implemented,
            recovery_ready: if self.ready {
                Guarantee::Implemented
            } else {
                Guarantee::Unavailable
            },
            retained_terminal_records: 32,
        }
    }
    fn available(&self, _: u8, _: u16) -> bool {
        self.execution
    }
    fn claim_atomic(
        &mut self,
        op: NamedOperation<'_>,
        policy: OperationPolicy,
        schema: &dyn OperationSchema,
        auth: &dyn Authority,
    ) -> Result<Claim, Error> {
        schema.validate(op)?;
        if !auth.allows(Capability::DangerousMaintenance) {
            return Err(Error::Denied);
        }
        self.claims += 1;
        if let Some((r, params)) = self
            .records
            .iter()
            .find(|(r, _)| r.operation_id == op.operation_id)
        {
            if r.module != op.module
                || r.operation != op.operation
                || params != op.params
                || r.station_id != auth.station_id().unwrap()
            {
                return Err(Error::Conflict);
            }
            return Ok(Claim {
                record: *r,
                replayed: true,
            });
        }
        if self.full {
            return Err(Error::Capacity);
        }
        let r = PersistentRecord {
            operation_id: op.operation_id,
            module: op.module,
            operation: op.operation,
            schema: 1,
            job_id: u32::try_from(self.records.len() + 1).unwrap(),
            station_id: 42,
            policy_revision: policy.revision,
            state: PersistentState::Accepted,
            result: 0,
            reason: 0,
        };
        self.records.push((r, op.params.to_vec()));
        Ok(Claim {
            record: r,
            replayed: false,
        })
    }
    fn query(
        &self,
        module: u8,
        id: [u8; 16],
        _: &dyn Authority,
    ) -> Result<PersistentRecord, Error> {
        self.records
            .iter()
            .find(|(r, _)| r.module == module && r.operation_id == id)
            .map(|(r, _)| *r)
            .ok_or(Error::NotFound)
    }
    fn output(
        &self,
        module: u8,
        id: [u8; 16],
        out: &mut [u8],
        auth: &dyn Authority,
    ) -> Result<OutputMeta, Error> {
        let r = self.query(module, id, auth)?;
        if matches!(
            r.state,
            PersistentState::Accepted | PersistentState::Running
        ) {
            return Err(Error::Conflict);
        }
        let bytes: &[u8] = if module == 0x72 {
            &[7, 0, 0, 0]
        } else {
            &[7, 0]
        };
        if out.len() < bytes.len() {
            return Err(Error::Capacity);
        }
        out[..bytes.len()].copy_from_slice(bytes);
        Ok(OutputMeta {
            operation: 1,
            schema: 1,
            length: bytes.len(),
        })
    }
}
fn module() -> u8 {
    if cfg!(feature = "factory-config") {
        0x73
    } else {
        0x72
    }
}
fn op(id: u8) -> NamedOperation<'static> {
    NamedOperation {
        module: module(),
        operation: 1,
        operation_id: [id; 16],
        params: &[1, 0, 7, 0],
    }
}
fn auth() -> Auth {
    Auth {
        allowed: Cell::new(true),
        dangerous: true,
    }
}
#[test]
fn atomic_identity_dedup_survives_engine_replacement_and_never_executes_on_submit() {
    let mut memory = Memory::new();
    let auth = auth();
    let first = {
        let mut e =
            PersistentEngine::new(Profile::Factory, Some(&mut memory), &Schema, &auth).unwrap();
        let c = e.submit(op(1)).unwrap();
        assert!(!c.replayed);
        assert_eq!(c.record.state, PersistentState::Accepted);
        c.record
    };
    let mut e = PersistentEngine::new(Profile::Factory, Some(&mut memory), &Schema, &auth).unwrap();
    let again = e.submit(op(1)).unwrap();
    assert!(again.replayed);
    assert_eq!(again.record, first);
    assert_eq!(e.query(module(), [1; 16]).unwrap().job_id, first.job_id);
}
#[test]
fn same_operation_id_different_valid_parameters_must_conflict_atomically() {
    struct Flexible;
    impl OperationSchema for Flexible {
        fn validate(&self, _: NamedOperation<'_>) -> Result<OperationPolicy, Error> {
            Ok(OperationPolicy {
                revision: 3,
                dangerous: false,
            })
        }
    }
    let mut memory = Memory::new();
    let auth = auth();
    let mut e =
        PersistentEngine::new(Profile::Factory, Some(&mut memory), &Flexible, &auth).unwrap();
    e.submit(op(1)).unwrap();
    assert_eq!(
        e.submit(NamedOperation {
            params: &[1, 0, 8, 0],
            ..op(1)
        }),
        Err(Error::Conflict)
    );
}
#[test]
fn missing_recovery_executor_and_ledger_never_admit_fake_jobs() {
    let mut memory = Memory::new();
    memory.ready = false;
    let auth = auth();
    assert_eq!(
        PersistentEngine::new(Profile::Factory, Some(&mut memory), &Schema, &auth).err(),
        Some(Error::Unavailable)
    );
    assert_eq!(
        PersistentEngine::new(Profile::Factory, None, &Schema, &auth).err(),
        Some(Error::Unavailable)
    );
    assert_eq!(
        PersistentEngine::new(Profile::Commercial, Some(&mut memory), &Schema, &auth).err(),
        Some(Error::Commercial)
    );
    memory.ready = true;
    memory.execution = false;
    let mut e = PersistentEngine::new(Profile::Factory, Some(&mut memory), &Schema, &auth).unwrap();
    assert_eq!(e.submit(op(1)), Err(Error::Unavailable));
    assert_eq!(memory.claims, 0);
}
#[test]
fn reboot_unresolved_is_interrupted_not_replayed_or_successful() {
    let mut memory = Memory::new();
    let auth = auth();
    {
        let mut e =
            PersistentEngine::new(Profile::Factory, Some(&mut memory), &Schema, &auth).unwrap();
        e.submit(op(1)).unwrap();
    }
    memory.reboot();
    let mut e = PersistentEngine::new(Profile::Factory, Some(&mut memory), &Schema, &auth).unwrap();
    let c = e.submit(op(1)).unwrap();
    assert!(c.replayed);
    assert_eq!(c.record.state, PersistentState::Interrupted);
    assert_eq!(c.record.result, 15);
    // Diagnostic output may exist after any terminal outcome; reading it never
    // changes the interrupted result into a successful physical operation.
    let mut out = [0; 8];
    assert!(e.output(module(), [1; 16], &mut out).is_ok());
    assert_eq!(
        e.query(module(), [1; 16]).unwrap().state,
        PersistentState::Interrupted
    );
}
#[test]
fn full_storage_rejects_new_identity_but_keeps_old_dedup_tombstones() {
    let mut memory = Memory::new();
    let auth = auth();
    {
        let mut e =
            PersistentEngine::new(Profile::Factory, Some(&mut memory), &Schema, &auth).unwrap();
        e.submit(op(1)).unwrap();
    }
    memory.full = true;
    let mut e = PersistentEngine::new(Profile::Factory, Some(&mut memory), &Schema, &auth).unwrap();
    assert_eq!(e.submit(op(2)), Err(Error::Capacity));
    assert!(e.submit(op(1)).unwrap().replayed);
}
#[test]
fn dangerous_scope_schema_and_current_station_are_required() {
    let mut memory = Memory::new();
    let limited = Auth {
        dangerous: false,
        ..auth()
    };
    let mut e =
        PersistentEngine::new(Profile::Development, Some(&mut memory), &Schema, &limited).unwrap();
    assert_eq!(e.submit(op(1)), Err(Error::Denied));
    let auth = auth();
    let mut e =
        PersistentEngine::new(Profile::Development, Some(&mut memory), &Schema, &auth).unwrap();
    assert_eq!(
        e.submit(NamedOperation {
            operation_id: [0; 16],
            ..op(1)
        }),
        Err(Error::Invalid)
    );
    assert_eq!(
        e.submit(NamedOperation {
            params: &[1, 0, 7, 0, 0],
            ..op(1)
        }),
        Err(Error::Invalid)
    );
    assert_eq!(
        e.submit(NamedOperation {
            params: &[2, 0, 7, 0],
            ..op(1)
        }),
        Err(Error::Invalid)
    );
}
#[test]
fn terminal_state_output_and_revocation_use_actual_ledger_only() {
    let mut memory = Memory::new();
    let auth = auth();
    {
        let mut e =
            PersistentEngine::new(Profile::Factory, Some(&mut memory), &Schema, &auth).unwrap();
        e.submit(op(1)).unwrap();
        assert_eq!(
            e.output(module(), [1; 16], &mut [0; 8]),
            Err(Error::Conflict)
        );
    }
    memory.records[0].0.state = PersistentState::Succeeded;
    let mut e = PersistentEngine::new(Profile::Factory, Some(&mut memory), &Schema, &auth).unwrap();
    let mut out = [0xa5; 8];
    assert_eq!(
        e.output(module(), [1; 16], &mut out),
        Ok(OutputMeta {
            operation: 1,
            schema: 1,
            length: if module() == 0x72 { 4 } else { 2 }
        })
    );
    assert_eq!(&out[..2], &[7, 0]);
    auth.allowed.set(false);
    assert_eq!(e.output(module(), [1; 16], &mut out), Err(Error::Denied));
    assert_eq!(out, [0; 8]);
    auth.allowed.set(true);
    assert_eq!(e.query(module(), [1; 16]), Err(Error::Closed));
}
#[test]
fn debug_hides_operation_ids_and_parameter_bytes() {
    let debug = format!("{:?}", op(1));
    assert!(!debug.contains("operation_id"));
    assert!(!debug.contains("[1, 0, 7"));
}

#[test]
fn output_without_a_defined_product_output_schema_is_unavailable() {
    struct InputOnly;
    impl OperationSchema for InputOnly {
        fn validate(&self, _: NamedOperation<'_>) -> Result<OperationPolicy, Error> {
            Ok(OperationPolicy {
                revision: 3,
                dangerous: false,
            })
        }
    }
    let mut memory = Memory::new();
    let auth = auth();
    {
        let mut e =
            PersistentEngine::new(Profile::Factory, Some(&mut memory), &InputOnly, &auth).unwrap();
        e.submit(op(1)).unwrap();
    }
    memory.records[0].0.state = PersistentState::Succeeded;
    let mut e =
        PersistentEngine::new(Profile::Factory, Some(&mut memory), &InputOnly, &auth).unwrap();
    let mut out = [0xa5; 8];
    assert_eq!(
        e.output(module(), [1; 16], &mut out),
        Err(Error::Unavailable)
    );
    assert_eq!(out, [0; 8]);
}
