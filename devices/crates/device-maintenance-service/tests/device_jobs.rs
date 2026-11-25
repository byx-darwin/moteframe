#![cfg(feature = "device-jobs")]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use device_maintenance_service::*;
use device_protocol::{Frame, Header, Kind, ModuleEntry};
use device_services::ServiceHandler;
use std::cell::{Cell, RefCell};
const ENTRY: ModuleEntry = ModuleEntry {
    module: 2,
    major: 1,
    minor: 0,
    availability: 1,
    features: 0,
    input_max: 128,
    output_max: 128,
};
struct Auth(Cell<bool>);
impl DeviceJobAccess for Auth {
    fn identity(&self) -> Option<[u8; 16]> {
        Some([7; 16])
    }
    fn start_allowed(&self, _: u16) -> bool {
        self.0.get()
    }
    fn dangerous_allowed(&self) -> bool {
        self.0.get()
    }
    fn read_scope(&self) -> bool {
        self.0.get()
    }
    fn may_read(&self, _: DeviceJobRecord) -> bool {
        self.0.get()
    }
}
struct Time;
impl Clock for Time {
    fn now_ms(&self) -> Result<u64, Error> {
        Ok(1)
    }
}
struct Memory<'a> {
    record: &'a RefCell<Option<DeviceJobRecord>>,
    claims: &'a Cell<u32>,
    full: bool,
}
impl DeviceJobLedger for Memory<'_> {
    fn guarantees(&self) -> DeviceJobGuarantees {
        DeviceJobGuarantees {
            atomic_identity: Durable::Implemented,
            durable_identity: Durable::Implemented,
            durable_terminal: Durable::Implemented,
            current_boot_recovered: Durable::Implemented,
            reset_preserves_ledger: Durable::Implemented,
            retained_terminal: 32,
        }
    }
    fn available(&self, _: u16) -> bool {
        true
    }
    fn claim_atomic(
        &mut self,
        r: NamedRequest<'_>,
        a: &dyn DeviceJobAccess,
    ) -> Result<DeviceJobClaim, Error> {
        if self.full {
            return Err(Error::Capacity);
        }
        self.claims.set(self.claims.get() + 1);
        if let Some(old) = *self.record.borrow() {
            if old.operation_id != r.operation_id || old.operation != r.operation {
                return Err(Error::Conflict);
            }
            return Ok(DeviceJobClaim {
                record: old,
                replayed: true,
            });
        }
        let record = DeviceJobRecord {
            operation_id: r.operation_id,
            operation: r.operation,
            job: 1,
            principal: a.identity().ok_or(Error::Denied)?,
            policy_revision: 1,
            state: DeviceJobState::Accepted,
            result: 0,
            reason: 0,
        };
        *self.record.borrow_mut() = Some(record);
        Ok(DeviceJobClaim {
            record,
            replayed: false,
        })
    }
    fn query(&self, id: [u8; 16], a: &dyn DeviceJobAccess) -> Result<DeviceJobRecord, Error> {
        if !a.read_scope() {
            return Err(Error::Denied);
        }
        self.record
            .borrow()
            .filter(|r| r.operation_id == id)
            .ok_or(Error::NotFound)
    }
    fn begin_execution_atomic(
        &mut self,
        r: DeviceJobRecord,
        _: &dyn DeviceJobAccess,
    ) -> Result<ExecutionClaim, Error> {
        let mut r = r;
        r.state = DeviceJobState::Running;
        *self.record.borrow_mut() = Some(r);
        Ok(ExecutionClaim::Acquired(r))
    }
    fn execution_record(&self, _: [u8; 16]) -> Result<DeviceJobRecord, Error> {
        self.record.borrow().ok_or(Error::NotFound)
    }
    fn finish_atomic(
        &mut self,
        mut r: DeviceJobRecord,
        t: DeviceJobTerminal,
    ) -> Result<DeviceJobRecord, Error> {
        r.state = t.state;
        r.result = t.result;
        r.reason = t.reason;
        *self.record.borrow_mut() = Some(r);
        Ok(r)
    }
}
struct Executor<'a>(&'a Cell<u32>);
impl DeviceJobExecutor for Executor<'_> {
    fn available(&self, _: u16) -> bool {
        true
    }
    fn begin(
        &mut self,
        _: DeviceJobRecord,
        _: &dyn DeviceJobAccess,
    ) -> Result<DeviceJobProgress, Error> {
        self.0.set(self.0.get() + 1);
        Ok(DeviceJobProgress::Deferred)
    }
    fn poll(&mut self, _: DeviceJobRecord) -> Result<DeviceJobProgress, Error> {
        Ok(DeviceJobProgress::Deferred)
    }
    fn terminal_persisted(&mut self, _: DeviceJobRecord) -> Result<(), Error> {
        Ok(())
    }
}
fn request(op: u8, b: &[u8]) -> Frame<'_> {
    Frame {
        header: Header {
            kind: Kind::Request,
            module: 2,
            opcode: op,
            message_id: 2,
            offset: 0,
            total_length: u16::try_from(b.len()).unwrap(),
        },
        payload: b,
    }
}
#[test]
fn test_should_claim_a_commercial_device_job_without_executing_before_response_admission() {
    let record = RefCell::new(None);
    let claims = Cell::new(0);
    let begins = Cell::new(0);
    let owner = DeviceJobsOwner::new(
        Memory {
            record: &record,
            claims: &claims,
            full: false,
        },
        Executor(&begins),
    );
    let auth = Auth(Cell::new(true));
    let mut h = DeviceJobsHandler::new(&owner, ENTRY, &auth, &Time).unwrap();
    let mut body = [1; 22];
    body[16..].copy_from_slice(&[1, 0, 2, 0, 1, 0]);
    let mut out = [0; 128];
    assert_eq!(h.dispatch(request(0x40, &body), &mut out), Ok(8));
    assert_eq!(&out[..8], &[0, 0, 0, 0, 1, 0, 0, 0]);
    assert_eq!(claims.get(), 1);
    assert_eq!(begins.get(), 0);
}
#[test]
fn test_should_reject_mutated_full_request_binding_during_response_replay() {
    let record = RefCell::new(None);
    let claims = Cell::new(0);
    let begins = Cell::new(0);
    let owner = DeviceJobsOwner::new(
        Memory {
            record: &record,
            claims: &claims,
            full: false,
        },
        Executor(&begins),
    );
    let auth = Auth(Cell::new(true));
    let mut handler = DeviceJobsHandler::new(&owner, ENTRY, &auth, &Time).unwrap();
    let mut body = [1; 22];
    body[16..].copy_from_slice(&[1, 0, 2, 0, 1, 0]);
    let mut out = [0; 128];
    assert_eq!(handler.dispatch(request(0x40, &body), &mut out), Ok(8));
    assert!(handler.replay_valid(request(0x40, &body), &out[..8]));
    body[16] = 2;
    assert!(!handler.replay_valid(request(0x40, &body), &out[..8]));
    body[16] = 1;
    body[21] = 1;
    assert!(!handler.replay_valid(request(0x40, &body), &out[..8]));
    assert_eq!(begins.get(), 0);
}
#[test]
fn test_should_preserve_legacy_ledger_compatibility_and_reject_bound_mismatch() {
    let record = RefCell::new(None);
    let claims = Cell::new(0);
    let mut ledger = Memory {
        record: &record,
        claims: &claims,
        full: false,
    };
    let auth = Auth(Cell::new(true));
    let accepted = ledger
        .claim_atomic(
            NamedRequest {
                operation_id: [1; 16],
                operation: 1,
                params: &[1, 0],
            },
            &auth,
        )
        .unwrap()
        .record;
    assert_eq!(ledger.execution_record_bound(accepted), Ok(accepted));
    let mut forged = accepted;
    forged.principal = [8; 16];
    assert_eq!(ledger.execution_record_bound(forged), Err(Error::Conflict));
    forged = accepted;
    forged.job += 1;
    assert_eq!(ledger.execution_record_bound(forged), Err(Error::Conflict));
}
#[test]
fn test_should_start_exactly_once_after_real_response_admission_and_not_on_terminal_replay() {
    let record = RefCell::new(None);
    let claims = Cell::new(0);
    let begins = Cell::new(0);
    let owner = DeviceJobsOwner::new(
        Memory {
            record: &record,
            claims: &claims,
            full: false,
        },
        Executor(&begins),
    );
    let auth = Auth(Cell::new(true));
    let mut h = DeviceJobsHandler::new(&owner, ENTRY, &auth, &Time).unwrap();
    let mut body = [1; 22];
    body[16..].copy_from_slice(&[1, 0, 2, 0, 1, 0]);
    let mut out = [0; 128];
    let f = request(0x40, &body);
    assert_eq!(h.dispatch(f, &mut out), Ok(8));
    h.response_admitted(f, &out[..8]).unwrap();
    h.response_admitted(f, &out[..8]).unwrap();
    assert_eq!(begins.get(), 1);
    let mut terminal = record.borrow().unwrap();
    terminal.state = DeviceJobState::Succeeded;
    *record.borrow_mut() = Some(terminal);
    let mut f = f;
    f.header.message_id = 3;
    assert_eq!(h.dispatch(f, &mut out), Ok(8));
    h.response_admitted(f, &out[..8]).unwrap();
    assert_eq!(begins.get(), 1);
}
#[test]
fn test_should_reject_full_parameter_and_identity_errors_before_claim() {
    for params in [[0, 0], [1, 1], [2, 0]] {
        let record = RefCell::new(None);
        let claims = Cell::new(0);
        let begins = Cell::new(0);
        let owner = DeviceJobsOwner::new(
            Memory {
                record: &record,
                claims: &claims,
                full: false,
            },
            Executor(&begins),
        );
        let auth = Auth(Cell::new(true));
        let mut h = DeviceJobsHandler::new(&owner, ENTRY, &auth, &Time).unwrap();
        let mut body = [1; 22];
        body[16..].copy_from_slice(&[1, 0, 2, 0, params[0], params[1]]);
        let mut out = [0; 128];
        assert_eq!(h.dispatch(request(0x40, &body), &mut out), Ok(4));
        assert_eq!(out[0], 1);
        assert_eq!(claims.get(), 0);
        assert_eq!(begins.get(), 0);
    }
}
#[test]
fn test_should_mute_start_on_scope_revocation_before_actual_admission() {
    let record = RefCell::new(None);
    let claims = Cell::new(0);
    let begins = Cell::new(0);
    let owner = DeviceJobsOwner::new(
        Memory {
            record: &record,
            claims: &claims,
            full: false,
        },
        Executor(&begins),
    );
    let auth = Auth(Cell::new(true));
    let mut h = DeviceJobsHandler::new(&owner, ENTRY, &auth, &Time).unwrap();
    let mut body = [1; 22];
    body[16..].copy_from_slice(&[2, 0, 2, 0, 1, 0]);
    let mut out = [0; 128];
    let f = request(0x40, &body);
    h.dispatch(f, &mut out).unwrap();
    auth.0.set(false);
    assert!(h.must_end_session());
    assert!(h.response_admitted(f, &out[..8]).is_err());
    assert_eq!(begins.get(), 0);
    assert_eq!(record.borrow().unwrap().state, DeviceJobState::Accepted);
}
#[test]
fn test_should_return_exact_persistent_five_byte_query_not_selftest_state() {
    let record = RefCell::new(None);
    let claims = Cell::new(0);
    let begins = Cell::new(0);
    let owner = DeviceJobsOwner::new(
        Memory {
            record: &record,
            claims: &claims,
            full: false,
        },
        Executor(&begins),
    );
    let auth = Auth(Cell::new(true));
    let mut h = DeviceJobsHandler::new(&owner, ENTRY, &auth, &Time).unwrap();
    let mut body = [1; 22];
    body[16..].copy_from_slice(&[1, 0, 2, 0, 1, 0]);
    let mut out = [0; 128];
    h.dispatch(request(0x40, &body), &mut out).unwrap();
    assert_eq!(h.dispatch(request(0x41, &[1; 16]), &mut out), Ok(9));
    assert_eq!(&out[..9], &[0, 0, 0, 0, 1, 0, 0, 0, 0]);
    assert_eq!(h.dispatch(request(0x41, &[2; 16]), &mut out), Ok(4));
    assert_eq!(out[0], 8);
}
struct Uncertain<'a>(&'a Cell<u32>);
impl DeviceJobExecutor for Uncertain<'_> {
    fn available(&self, _: u16) -> bool {
        true
    }
    fn begin(
        &mut self,
        _: DeviceJobRecord,
        access: &dyn DeviceJobAccess,
    ) -> Result<DeviceJobProgress, Error> {
        if !access.dangerous_allowed() {
            return Err(Error::Denied);
        }
        self.0.set(self.0.get() + 1);
        Err(Error::UnknownOutcome)
    }
    fn poll(&mut self, _: DeviceJobRecord) -> Result<DeviceJobProgress, Error> {
        Err(Error::UnknownOutcome)
    }
    fn terminal_persisted(&mut self, _: DeviceJobRecord) -> Result<(), Error> {
        Err(Error::UnknownOutcome)
    }
}
#[test]
fn test_should_retain_unknown_execution_identity_and_never_repeat_it_after_reconnect() {
    let record = RefCell::new(None);
    let claims = Cell::new(0);
    let begins = Cell::new(0);
    let owner = DeviceJobsOwner::new(
        Memory {
            record: &record,
            claims: &claims,
            full: false,
        },
        Uncertain(&begins),
    );
    let auth = Auth(Cell::new(true));
    let mut body = [1; 22];
    body[16..].copy_from_slice(&[2, 0, 2, 0, 1, 0]);
    let f = request(0x40, &body);
    let mut out = [0; 128];
    let mut first = DeviceJobsHandler::new(&owner, ENTRY, &auth, &Time).unwrap();
    assert_eq!(first.dispatch(f, &mut out), Ok(8));
    assert!(first.response_admitted(f, &out[..8]).is_err());
    assert!(first.must_end_session());
    assert_eq!(record.borrow().unwrap().state, DeviceJobState::Running);
    let mut reconnected = DeviceJobsHandler::new(&owner, ENTRY, &auth, &Time).unwrap();
    assert_eq!(reconnected.dispatch(f, &mut out), Ok(8));
    reconnected.response_admitted(f, &out[..8]).unwrap();
    assert_eq!(begins.get(), 1);
    assert_eq!(
        reconnected.dispatch(request(0x41, &[1; 16]), &mut out),
        Ok(9)
    );
    assert_eq!(&out[..9], &[0, 0, 0, 0, 2, 0, 0, 0, 0]);
    assert_eq!(owner.poll([2; 16]), Err(Error::Storage));
    assert_eq!(begins.get(), 1);
}
struct RoleWithoutDanger;
impl DeviceJobAccess for RoleWithoutDanger {
    fn identity(&self) -> Option<[u8; 16]> {
        Some([7; 16])
    }
    fn start_allowed(&self, _: u16) -> bool {
        true
    }
    fn dangerous_allowed(&self) -> bool {
        false
    }
    fn read_scope(&self) -> bool {
        true
    }
    fn may_read(&self, _: DeviceJobRecord) -> bool {
        true
    }
}
#[test]
fn test_should_reject_missing_dangerous_scope_before_durable_claim_with_ff04() {
    let record = RefCell::new(None);
    let claims = Cell::new(0);
    let begins = Cell::new(0);
    let owner = DeviceJobsOwner::new(
        Memory {
            record: &record,
            claims: &claims,
            full: false,
        },
        Executor(&begins),
    );
    let mut h = DeviceJobsHandler::new(&owner, ENTRY, &RoleWithoutDanger, &Time).unwrap();
    let mut body = [1; 22];
    body[16..].copy_from_slice(&[1, 0, 2, 0, 1, 0]);
    let mut out = [0; 128];
    assert_eq!(h.dispatch(request(0x40, &body), &mut out), Ok(4));
    assert_eq!(&out[..4], &[5, 0, 4, 255]);
    assert_eq!(claims.get(), 0);
    assert_eq!(begins.get(), 0);
}

#[test]
fn test_should_encode_real_durable_ledger_full_as_limit_exceeded() {
    let record = RefCell::new(None);
    let claims = Cell::new(0);
    let begins = Cell::new(0);
    let owner = DeviceJobsOwner::new(
        Memory {
            record: &record,
            claims: &claims,
            full: true,
        },
        Executor(&begins),
    );
    let auth = Auth(Cell::new(true));
    let mut h = DeviceJobsHandler::new(&owner, ENTRY, &auth, &Time).unwrap();
    let mut body = [1; 22];
    body[16..].copy_from_slice(&[1, 0, 2, 0, 1, 0]);
    let mut out = [0; 128];
    let result = h.dispatch(request(0x40, &body), &mut out);
    assert_eq!(result, Ok(4));
    assert_eq!(&out[..4], &[7, 0, 0, 0]);
    assert_eq!(begins.get(), 0);
}

#[test]
fn test_should_reject_short_caller_buffers_before_any_durable_claim() {
    for size in [0, 3, 4, 7] {
        let record = RefCell::new(None);
        let claims = Cell::new(0);
        let begins = Cell::new(0);
        let owner = DeviceJobsOwner::new(
            Memory {
                record: &record,
                claims: &claims,
                full: false,
            },
            Executor(&begins),
        );
        let auth = Auth(Cell::new(true));
        let mut handler = DeviceJobsHandler::new(&owner, ENTRY, &auth, &Time).unwrap();
        let mut body = [1; 22];
        body[16..].copy_from_slice(&[1, 0, 2, 0, 1, 0]);
        assert_eq!(
            handler.dispatch(request(0x40, &body), &mut [0; 128][..size]),
            Err(device_services::SessionError::Capacity)
        );
        assert_eq!(claims.get(), 0);
        assert_eq!(begins.get(), 0);
        assert!(record.borrow().is_none());
    }
}
