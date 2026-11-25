//! Durable commercial identity, lifecycle and access contracts.
mod support;
use device_job_store::JobStore;
use device_maintenance_service::{
    DeviceJobLedger, DeviceJobState as State, DeviceJobTerminal, Durable, Error, ExecutionClaim,
    NamedRequest,
};
use support::*;

fn success() -> DeviceJobTerminal {
    DeviceJobTerminal {
        state: State::Succeeded,
        result: 0,
        reason: 0,
    }
}

#[test]
fn test_should_initialize_explicitly_and_replay_exact_durable_identity() {
    let media = Nor::blank();
    let boot = Boot::new();
    let mut ledger = open(media.clone(), boot.clone(), Policy::new(), Isolation::new());
    assert_eq!(media.0.borrow().effects, 0);
    assert_eq!(
        ledger.guarantees().current_boot_recovered,
        Durable::Unavailable
    );
    assert_eq!(
        ledger.claim_atomic(request(1, 1), &Access::new(7)),
        Err(Error::Unavailable)
    );
    must(ledger.initialize());
    assert_eq!(must(ledger.revision()), 1);
    assert_eq!(ledger.initialize(), Err(Error::Conflict));
    let access = Access::new(7);
    let claim = must(ledger.claim_atomic(request(1, 1), &access));
    assert_eq!(
        (claim.record.job, claim.record.state, claim.replayed),
        (1, State::Accepted, false)
    );
    assert_eq!(must(ledger.revision()), 2);
    let revision = must(ledger.revision());
    assert_eq!(
        must(ledger.claim_atomic(request(1, 1), &access)).record,
        claim.record
    );
    assert_eq!(must(ledger.revision()), revision);
    let (s, b, p, i) = ledger.into_parts();
    let mut ledger = must(JobStore::<_, _, _, _, 32, 4096>::open(s, b, p, i));
    assert!(must(ledger.claim_atomic(request(1, 1), &access)).replayed);
    assert_eq!(must(ledger.query([1; 16], &access)), claim.record);
}
#[test]
fn test_should_preserve_full_binding_and_independent_principal_domains() {
    let mut ledger = initialized();
    let access = Access::new(7);
    let b = Access::new(8);
    let ca = must(ledger.claim_atomic(request(1, 1), &access));
    let cb = must(ledger.claim_atomic(request(1, 2), &b));
    assert_eq!((ca.record.job, cb.record.job), (1, 2));
    assert_eq!(ledger.execution_record([1; 16]), Err(Error::Conflict));
    assert_eq!(must(ledger.execution_record_bound(ca.record)), ca.record);
    assert_eq!(must(ledger.query([1; 16], &access)), ca.record);
    assert_eq!(must(ledger.query([1; 16], &b)), cb.record);
    for record in [
        request(1, 2),
        NamedRequest {
            params: &[2, 0],
            ..request(1, 1)
        },
        NamedRequest {
            params: &[1, 0, 0],
            ..request(1, 1)
        },
    ] {
        assert_eq!(ledger.claim_atomic(record, &access), Err(Error::Conflict));
    }
    let station = Access::new(9);
    assert_eq!(ledger.query([1; 16], &station), Err(Error::NotFound));
    station.foreign.set(true);
    assert_eq!(ledger.query([1; 16], &station), Err(Error::Conflict));
    access.foreign.set(true);
    access.deny_own.set(true);
    assert_eq!(ledger.query([1; 16], &access), Err(Error::Denied));
    let only = must(ledger.claim_atomic(request(2, 1), &b));
    assert_eq!(must(ledger.query([2; 16], &station)), only.record);
}
#[test]
fn test_should_persist_running_and_real_terminal_without_current_app_or_policy() {
    let policy = Policy::new();
    let mut ledger = open(Nor::blank(), Boot::new(), policy.clone(), Isolation::new());
    must(ledger.initialize());
    let access = Access::new(7);
    let claim = must(ledger.claim_atomic(request(1, 1), &access));
    assert_eq!(
        ledger.finish_atomic(claim.record, success()),
        Err(Error::Conflict)
    );
    let ExecutionClaim::Acquired(record) =
        must(ledger.begin_execution_atomic(claim.record, &access))
    else {
        panic!("missing execution claim")
    };
    assert_eq!(record.state, State::Running);
    assert!(matches!(
        must(ledger.begin_execution_atomic(claim.record, &access)),
        ExecutionClaim::AlreadyStarted(_)
    ));
    access.identity.set(None);
    policy.allowed.set(false);
    policy.revision.set(Err(Error::Unavailable));
    let t = must(ledger.finish_atomic(record, success()));
    assert_eq!(t.state, State::Succeeded);
    let rev = must(ledger.revision());
    assert_eq!(must(ledger.finish_atomic(record, success())), t);
    assert_eq!(must(ledger.revision()), rev);
    assert_eq!(
        ledger.finish_atomic(
            record,
            DeviceJobTerminal {
                state: State::Failed,
                result: 14,
                reason: 0
            }
        ),
        Err(Error::Conflict)
    );
}
#[test]
fn test_should_recover_actual_boot_and_preserve_same_boot_jobs() {
    let boot = Boot::new();
    let mut ledger = open(Nor::blank(), boot.clone(), Policy::new(), Isolation::new());
    must(ledger.initialize());
    let access = Access::new(7);
    let accepted = must(ledger.claim_atomic(request(1, 1), &access)).record;
    let running = must(ledger.claim_atomic(request(2, 2), &access)).record;
    must(ledger.begin_execution_atomic(running, &access));
    let done = must(ledger.claim_atomic(request(3, 1), &access)).record;
    let ExecutionClaim::Acquired(done) = must(ledger.begin_execution_atomic(done, &access)) else {
        panic!("missing execution claim")
    };
    let done = must(ledger.finish_atomic(done, success()));
    let rev = must(ledger.revision());
    must(ledger.recover_and_reload());
    assert_eq!(must(ledger.revision()), rev);
    assert_eq!(
        must(ledger.query(accepted.operation_id, &access)).state,
        State::Accepted
    );
    boot.0.set(Ok(2));
    assert!(!ledger.available(1));
    assert!(ledger.query([1; 16], &access).is_err());
    must(ledger.recover_and_reload());
    for id in [1, 2] {
        let record = must(ledger.query([id; 16], &access));
        assert_eq!(
            (record.state, record.result, record.reason),
            (State::Interrupted, 4, 0xff06)
        );
    }
    assert_eq!(must(ledger.query([3; 16], &access)), done);
    assert_eq!(must(ledger.revision()), rev + 1);
    assert!(must(ledger.claim_atomic(request(1, 1), &access)).replayed);
    boot.0.set(Ok(1));
    assert!(ledger.recover_and_reload().is_err());
    boot.0.set(Ok(2));
    must(ledger.recover_and_reload());
}
#[test]
fn test_should_retain_at_least_32_terminal_identities_without_eviction() {
    let mut ledger = initialized();
    let access = Access::new(7);
    for id in 1..=32 {
        let record = must(ledger.claim_atomic(request(id, 1), &access)).record;
        let ExecutionClaim::Acquired(record) = must(ledger.begin_execution_atomic(record, &access))
        else {
            panic!("missing claim")
        };
        must(ledger.finish_atomic(record, success()));
    }
    assert_eq!(must(ledger.retained()), 32);
    assert_eq!(
        ledger.claim_atomic(request(33, 1), &access),
        Err(Error::Capacity)
    );
    let (s, b, p, i) = ledger.into_parts();
    let mut ledger = must(JobStore::<_, _, _, _, 32, 4096>::open(s, b, p, i));
    for id in 1..=32 {
        assert_eq!(
            must(ledger.query([id; 16], &access)).state,
            State::Succeeded
        );
        assert!(must(ledger.claim_atomic(request(id, 1), &access)).replayed);
    }
}
#[test]
fn test_should_check_live_permissions_and_original_policy_revision() {
    let policy = Policy::new();
    let isolation = Isolation::new();
    let boot = Boot::new();
    let mut ledger = open(
        Nor::blank(),
        boot.clone(),
        policy.clone(),
        isolation.clone(),
    );
    must(ledger.initialize());
    let access = Access::new(7);
    let record = must(ledger.claim_atomic(request(1, 1), &access)).record;
    policy.revision.set(Ok(2));
    policy.historical.set(false);
    assert_eq!(
        ledger.claim_atomic(request(1, 1), &access),
        Err(Error::Denied)
    );
    assert_eq!(
        ledger.begin_execution_atomic(record, &access),
        Err(Error::Denied)
    );
    policy.historical.set(true);
    assert_eq!(
        must(ledger.claim_atomic(request(1, 1), &access))
            .record
            .policy_revision,
        1
    );
    access.danger.set(false);
    assert_eq!(
        ledger.claim_atomic(request(1, 1), &access),
        Err(Error::DangerousDenied)
    );
    access.danger.set(true);
    access.start.set(false);
    assert_eq!(
        ledger.claim_atomic(request(2, 1), &access),
        Err(Error::Denied)
    );
    access.start.set(true);
    isolation.0.set(false);
    assert!(!ledger.available(1));
    assert_eq!(
        ledger.guarantees().reset_preserves_ledger,
        Durable::Unavailable
    );
    assert!(ledger.claim_atomic(request(2, 1), &access).is_err());
    isolation.0.set(true);
    boot.0.set(Ok(0));
    assert!(!ledger.available(1));
    boot.0.set(Err(Error::Unavailable));
    assert!(!ledger.available(1));
}
#[test]
fn test_should_reject_invalid_capacity_and_canonical_new_requests() {
    assert!(
        JobStore::<_, _, _, _, 31, 4096>::open(
            snapshot(Nor::blank()),
            Boot::new(),
            Policy::new(),
            Isolation::new()
        )
        .is_err()
    );
    assert!(
        JobStore::<_, _, _, _, 32, 2111>::open(
            snapshot(Nor::blank()),
            Boot::new(),
            Policy::new(),
            Isolation::new()
        )
        .is_err()
    );
    let mut ledger = initialized();
    let access = Access::new(7);
    assert_eq!(
        ledger.claim_atomic(request(0, 1), &access),
        Err(Error::Invalid)
    );
    assert_eq!(
        ledger.claim_atomic(
            NamedRequest {
                params: &[1, 1],
                ..request(1, 1)
            },
            &access
        ),
        Err(Error::Invalid)
    );
    access.identity.set(Some([0; 16]));
    assert_eq!(
        ledger.claim_atomic(request(1, 1), &access),
        Err(Error::Denied)
    );
}

#[test]
fn test_should_reject_every_nonterminal_or_invalid_terminal_outcome_without_write() {
    let mut ledger = initialized();
    let access = Access::new(7);
    let accepted = must(ledger.claim_atomic(request(1, 1), &access)).record;
    let ExecutionClaim::Acquired(running) = must(ledger.begin_execution_atomic(accepted, &access))
    else {
        panic!("missing execution claim")
    };
    let revision = must(ledger.revision());
    for terminal in [
        DeviceJobTerminal {
            state: State::Accepted,
            result: 0,
            reason: 0,
        },
        DeviceJobTerminal {
            state: State::Running,
            result: 0,
            reason: 0,
        },
        DeviceJobTerminal {
            state: State::Succeeded,
            result: 1,
            reason: 0,
        },
        DeviceJobTerminal {
            state: State::Succeeded,
            result: 0,
            reason: 1,
        },
        DeviceJobTerminal {
            state: State::Failed,
            result: 0,
            reason: 0,
        },
        DeviceJobTerminal {
            state: State::Interrupted,
            result: 0x10,
            reason: 0,
        },
        DeviceJobTerminal {
            state: State::Failed,
            result: 0x13,
            reason: 0,
        },
    ] {
        assert_eq!(ledger.finish_atomic(running, terminal), Err(Error::Invalid));
        assert_eq!(must(ledger.revision()), revision);
    }
    let terminal = DeviceJobTerminal {
        state: State::Interrupted,
        result: 14,
        reason: 0,
    };
    assert_eq!(
        must(ledger.finish_atomic(running, terminal)).state,
        State::Interrupted
    );
}

#[test]
fn test_should_configure_larger_lifetime_retention_without_reclaiming_original_slots() {
    let mut ledger = must(JobStore::<_, _, _, _, 33, 2176>::open(
        snapshot(Nor::blank()),
        Boot::new(),
        Policy::new(),
        Isolation::new(),
    ));
    must(ledger.initialize());
    for id in 1..=33 {
        must(ledger.claim_atomic(request(id, 1), &Access::new(7)));
    }
    assert_eq!(must(ledger.retained()), 33);
    assert_eq!(
        ledger.claim_atomic(request(34, 1), &Access::new(7)),
        Err(Error::Capacity)
    );
    let (store, boot, policy, isolation) = ledger.into_parts();
    assert!(JobStore::<_, _, _, _, 32, 4096>::open(store, boot, policy, isolation).is_err());
}

#[test]
fn test_should_recheck_live_read_scope_before_exposing_ambiguous_station_results() {
    struct RevokingStation {
        read: core::cell::Cell<bool>,
    }
    impl device_maintenance_service::DeviceJobAccess for RevokingStation {
        fn identity(&self) -> Option<[u8; 16]> {
            Some([9; 16])
        }
        fn start_allowed(&self, _: u16) -> bool {
            false
        }
        fn dangerous_allowed(&self) -> bool {
            false
        }
        fn read_scope(&self) -> bool {
            self.read.get()
        }
        fn may_read(&self, _: device_maintenance_service::DeviceJobRecord) -> bool {
            self.read.set(false);
            true
        }
    }
    let mut ledger = initialized();
    must(ledger.claim_atomic(request(1, 1), &Access::new(7)));
    must(ledger.claim_atomic(request(1, 2), &Access::new(8)));
    assert_eq!(
        ledger.query(
            [1; 16],
            &RevokingStation {
                read: core::cell::Cell::new(true)
            }
        ),
        Err(Error::Denied)
    );
}

#[test]
fn test_should_reject_direct_shared_query_recursion_from_read_authorization() {
    struct RecursiveRead<'a> {
        ledger: &'a Ledger,
        observed:
            core::cell::Cell<Option<Result<device_maintenance_service::DeviceJobRecord, Error>>>,
    }
    impl device_maintenance_service::DeviceJobAccess for RecursiveRead<'_> {
        fn identity(&self) -> Option<[u8; 16]> {
            Some([7; 16])
        }
        fn start_allowed(&self, _: u16) -> bool {
            true
        }
        fn dangerous_allowed(&self) -> bool {
            true
        }
        fn read_scope(&self) -> bool {
            true
        }
        fn may_read(&self, _: device_maintenance_service::DeviceJobRecord) -> bool {
            self.observed
                .set(Some(self.ledger.query([1; 16], &Access::new(7))));
            true
        }
    }
    let mut ledger = initialized();
    must(ledger.claim_atomic(request(1, 1), &Access::new(7)));
    let access = RecursiveRead {
        ledger: &ledger,
        observed: core::cell::Cell::new(None),
    };
    must(ledger.query([1; 16], &access));
    assert_eq!(access.observed.get(), Some(Err(Error::Busy)));
}

#[test]
fn test_should_reject_shared_recursion_from_actual_boot_and_isolation_ports() {
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };
    type Callback = Rc<RefCell<Option<Box<dyn Fn()>>>>;
    #[derive(Clone)]
    struct Context {
        callback: Callback,
    }
    impl device_job_store::TrustedBoot for Context {
        fn current_boot(&self) -> Result<u64, Error> {
            if let Some(callback) = &*self.callback.borrow() {
                callback();
            }
            Ok(1)
        }
    }
    impl device_job_store::ResetIsolation for Context {
        fn preserves_ledger(&self, _: u16) -> bool {
            if let Some(callback) = &*self.callback.borrow() {
                callback();
            }
            true
        }
    }
    let callback: Callback = Rc::new(RefCell::new(None));
    let context = Context {
        callback: callback.clone(),
    };
    let mut ledger = must(JobStore::<_, _, _, _, 32, 4096>::open(
        snapshot(Nor::blank()),
        context.clone(),
        Policy::new(),
        context,
    ));
    must(ledger.initialize());
    must(ledger.claim_atomic(request(1, 1), &Access::new(7)));
    let ledger = Rc::new(ledger);
    let weak = Rc::downgrade(&ledger);
    let entered = Rc::new(Cell::new(false));
    let results = Rc::new(RefCell::new(Vec::new()));
    let observed = results.clone();
    *callback.borrow_mut() = Some(Box::new(move || {
        if entered.replace(true) {
            return;
        }
        let ledger = weak
            .upgrade()
            .unwrap_or_else(|| panic!("owner disappeared"));
        observed.borrow_mut().push(ledger.execution_record([1; 16]));
        assert!(!ledger.available(1));
        assert_eq!(
            ledger.guarantees().current_boot_recovered,
            Durable::Unavailable
        );
        entered.set(false);
    }));
    assert_eq!(
        must(ledger.query([1; 16], &Access::new(7))).state,
        State::Accepted
    );
    assert!(!results.borrow().is_empty());
    assert!(results.borrow().iter().all(|r| *r == Err(Error::Busy)));
}
