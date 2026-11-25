//! Actual service dispatch/admission, domain isolation and lifecycle recovery.
mod support;
use device_maintenance_service::*;
use device_protocol::{Frame, Header, Kind, ModuleEntry};
use device_services::ServiceHandler;
use device_storage::persistence::{Error as StorageError, Snapshot, SnapshotStore};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use support::*;
const ENTRY: ModuleEntry = ModuleEntry {
    module: 2,
    major: 1,
    minor: 0,
    availability: 1,
    features: 0,
    input_max: 128,
    output_max: 128,
};
struct Time;
impl Clock for Time {
    fn now_ms(&self) -> Result<u64, Error> {
        Ok(1)
    }
}
fn frame(op: u8, id: u16, payload: &[u8]) -> Frame<'_> {
    Frame {
        header: Header {
            kind: Kind::Request,
            module: 2,
            opcode: op,
            message_id: id,
            offset: 0,
            total_length: must(u16::try_from(payload.len())),
        },
        payload,
    }
}
fn body(operation: u8) -> [u8; 22] {
    let mut b = [1; 22];
    b[16..].copy_from_slice(&[operation, 0, 2, 0, 1, 0]);
    b
}
#[derive(Clone, Debug)]
struct Effects {
    begins: Rc<RefCell<Vec<[u8; 16]>>>,
    polls: Rc<Cell<usize>>,
    finalized: Rc<Cell<usize>>,
}
impl Effects {
    fn new() -> Self {
        Self {
            begins: Rc::new(RefCell::new(Vec::new())),
            polls: Rc::new(Cell::new(0)),
            finalized: Rc::new(Cell::new(0)),
        }
    }
}
impl DeviceJobExecutor for Effects {
    fn available(&self, op: u16) -> bool {
        matches!(op, 1 | 2)
    }
    fn begin(
        &mut self,
        record: DeviceJobRecord,
        access: &dyn DeviceJobAccess,
    ) -> Result<DeviceJobProgress, Error> {
        if access.identity() != Some(record.principal)
            || !access.start_allowed(record.operation)
            || !access.dangerous_allowed()
        {
            return Err(Error::Denied);
        }
        assert_eq!(record.state, DeviceJobState::Running);
        self.begins.borrow_mut().push(record.principal);
        Ok(DeviceJobProgress::Deferred)
    }
    fn poll(&mut self, record: DeviceJobRecord) -> Result<DeviceJobProgress, Error> {
        if !self.begins.borrow().contains(&record.principal) {
            return Err(Error::Backend);
        }
        self.polls.set(self.polls.get() + 1);
        Ok(DeviceJobProgress::Terminal(DeviceJobTerminal {
            state: DeviceJobState::Succeeded,
            result: 0,
            reason: 0,
        }))
    }
    fn terminal_persisted(&mut self, record: DeviceJobRecord) -> Result<(), Error> {
        assert_eq!(record.state, DeviceJobState::Succeeded);
        self.finalized.set(self.finalized.get() + 1);
        Ok(())
    }
}
#[test]
fn test_should_execute_two_same_id_domains_only_after_bound_admission_and_poll_after_disconnect() {
    let mut ledger = initialized();
    let access = Access::new(7);
    let b = Access::new(8);
    let ra = must(ledger.claim_atomic(request(1, 1), &access)).record;
    let rb = must(ledger.claim_atomic(request(1, 2), &b)).record;
    let effects = Effects::new();
    let owner = DeviceJobsOwner::new(ledger, effects.clone());
    {
        let mut ha = must(DeviceJobsHandler::new(&owner, ENTRY, &access, &Time));
        let mut hb = must(DeviceJobsHandler::new(&owner, ENTRY, &b, &Time));
        let body_a = body(1);
        let body_b = body(2);
        let mut oa = [0; 128];
        let mut ob = [0; 128];
        assert_eq!(ha.dispatch(frame(0x40, 1, &body_a), &mut oa), Ok(8));
        assert_eq!(hb.dispatch(frame(0x40, 2, &body_b), &mut ob), Ok(8));
        assert!(effects.begins.borrow().is_empty());
        assert!(ha.replay_valid(frame(0x40, 1, &body_a), &oa[..8]));
        must(ha.response_admitted(frame(0x40, 1, &body_a), &oa[..8]));
        must(hb.response_admitted(frame(0x40, 2, &body_b), &ob[..8]));
        assert_eq!(&*effects.begins.borrow(), &[[7; 16], [8; 16]]);
        must(ha.connection_closed());
        must(hb.connection_closed());
    }
    assert_eq!(owner.poll([1; 16]), Err(Error::Conflict));
    let mut forged = ra;
    forged.job = rb.job;
    assert!(owner.poll_bound(forged).is_err());
    assert_eq!(effects.polls.get(), 0);
    must(owner.poll_bound(ra));
    must(owner.poll_bound(rb));
    assert_eq!((effects.polls.get(), effects.finalized.get()), (2, 2));
    assert_eq!(owner.poll_bound(ra), Err(Error::Conflict));
    let mut h = must(DeviceJobsHandler::new(&owner, ENTRY, &access, &Time));
    let mut out = [0; 128];
    assert_eq!(h.dispatch(frame(0x41, 3, &[1; 16]), &mut out), Ok(9));
    assert_eq!(&out[..9], &[0, 0, 0, 0, 3, 0, 0, 0, 0]);
    let body_a = body(1);
    assert_eq!(h.dispatch(frame(0x40, 4, &body_a), &mut out), Ok(8));
    must(h.response_admitted(frame(0x40, 4, &body_a), &out[..8]));
    assert_eq!(effects.begins.borrow().len(), 2);
}
#[derive(Debug)]
struct FailAfter {
    store: Store,
    fail: Rc<Cell<bool>>,
}
impl SnapshotStore for FailAfter {
    fn load(&self, out: &mut [u8]) -> Result<Option<Snapshot>, StorageError> {
        self.store.load(out)
    }
    fn commit(&mut self, expected: u64, bytes: &[u8]) -> Result<Snapshot, StorageError> {
        let s = self.store.commit(expected, bytes)?;
        if self.fail.replace(false) {
            Err(StorageError::Io)
        } else {
            Ok(s)
        }
    }
    fn recover(&mut self) -> Result<(), StorageError> {
        self.store.recover()
    }
}
#[test]
fn test_should_extract_explicitly_recover_and_rebuild_after_uncertain_wire_acceptance() {
    let fail = Rc::new(Cell::new(false));
    let mut ledger = must(device_job_store::JobStore::<_, _, _, _, 32, 4096>::open(
        FailAfter {
            store: snapshot(Nor::blank()),
            fail: fail.clone(),
        },
        Boot::new(),
        Policy::new(),
        Isolation::new(),
    ));
    must(ledger.initialize());
    let effects = Effects::new();
    let owner = DeviceJobsOwner::new(ledger, effects.clone());
    let access = Access::new(7);
    let b = body(1);
    {
        let mut h = must(DeviceJobsHandler::new(&owner, ENTRY, &access, &Time));
        let mut out = [0; 128];
        fail.set(true);
        assert_eq!(h.dispatch(frame(0x40, 1, &b), &mut out), Ok(4));
        assert_ne!(&out[..2], &[0, 0]);
        assert!(effects.begins.borrow().is_empty());
    }
    let (mut ledger, executor) = owner.into_parts();
    assert_eq!(ledger.revision(), Err(Error::Storage));
    must(ledger.recover_and_reload());
    let stored = must(ledger.query([1; 16], &access));
    assert_eq!((stored.job, stored.state), (1, DeviceJobState::Accepted));
    let owner = DeviceJobsOwner::new(ledger, executor);
    {
        let mut h = must(DeviceJobsHandler::new(&owner, ENTRY, &access, &Time));
        let mut out = [0; 128];
        assert_eq!(h.dispatch(frame(0x41, 2, &[1; 16]), &mut out), Ok(9));
        assert_eq!(out[4], 1);
        assert_eq!(h.dispatch(frame(0x40, 3, &b), &mut out), Ok(8));
        assert_eq!(&out[4..8], &1_u32.to_le_bytes());
        must(h.response_admitted(frame(0x40, 3, &b), &out[..8]));
        must(h.connection_closed());
    }
    assert_eq!(effects.begins.borrow().len(), 1);
    access.identity.set(None);
    must(owner.poll_bound(stored));
    assert_eq!(effects.finalized.get(), 1);
}

#[test]
fn test_should_reject_recursive_owner_access_without_panicking_or_executing() {
    struct RecursiveAccess {
        callback: Box<dyn Fn()>,
    }
    impl DeviceJobAccess for RecursiveAccess {
        fn identity(&self) -> Option<[u8; 16]> {
            Some([7; 16])
        }
        fn start_allowed(&self, _: u16) -> bool {
            (self.callback)();
            true
        }
        fn dangerous_allowed(&self) -> bool {
            true
        }
        fn read_scope(&self) -> bool {
            true
        }
        fn may_read(&self, _: DeviceJobRecord) -> bool {
            true
        }
    }
    let effects = Effects::new();
    let owner = Rc::new(DeviceJobsOwner::new(initialized(), effects.clone()));
    let weak = Rc::downgrade(&owner);
    let observed = Rc::new(RefCell::new(Vec::new()));
    let results = observed.clone();
    let access = RecursiveAccess {
        callback: Box::new(move || {
            let owner = weak
                .upgrade()
                .unwrap_or_else(|| panic!("owner disappeared"));
            results.borrow_mut().push(owner.poll([1; 16]));
        }),
    };
    let mut handler = must(DeviceJobsHandler::new(&owner, ENTRY, &access, &Time));
    let mut out = [0; 128];
    assert_eq!(handler.dispatch(frame(0x40, 1, &body(1)), &mut out), Ok(8));
    assert!(observed.borrow().contains(&Err(Error::Busy)));
    assert!(effects.begins.borrow().is_empty());
}

#[test]
fn test_should_recheck_revoked_scope_after_durable_claim_and_running_commits() {
    struct Revokes<'a> {
        store: Store,
        access: &'a Access,
        enabled: Rc<Cell<bool>>,
    }
    impl SnapshotStore for Revokes<'_> {
        fn load(&self, out: &mut [u8]) -> Result<Option<Snapshot>, StorageError> {
            self.store.load(out)
        }
        fn commit(&mut self, expected: u64, bytes: &[u8]) -> Result<Snapshot, StorageError> {
            let snapshot = self.store.commit(expected, bytes)?;
            if self.enabled.replace(false) {
                self.access.start.set(false);
            }
            Ok(snapshot)
        }
        fn recover(&mut self) -> Result<(), StorageError> {
            self.store.recover()
        }
    }
    let access = Access::new(7);
    let enabled = Rc::new(Cell::new(false));
    let mut ledger = must(device_job_store::JobStore::<_, _, _, _, 32, 4096>::open(
        Revokes {
            store: snapshot(Nor::blank()),
            access: &access,
            enabled: enabled.clone(),
        },
        Boot::new(),
        Policy::new(),
        Isolation::new(),
    ));
    must(ledger.initialize());
    enabled.set(true);
    assert_eq!(
        ledger.claim_atomic(request(1, 1), &access),
        Err(Error::Denied)
    );
    let accepted = must(ledger.query([1; 16], &access));
    assert_eq!(accepted.state, DeviceJobState::Accepted);
    access.start.set(true);
    enabled.set(true);
    assert_eq!(
        ledger.begin_execution_atomic(accepted, &access),
        Err(Error::Denied)
    );
    assert_eq!(
        must(ledger.query([1; 16], &access)).state,
        DeviceJobState::Running
    );
    access.start.set(true);
    assert!(matches!(
        must(ledger.begin_execution_atomic(accepted, &access)),
        ExecutionClaim::AlreadyStarted(_)
    ));
}
