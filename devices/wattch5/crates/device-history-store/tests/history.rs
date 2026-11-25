//! Durable history lifecycle and authorization behavior.
use device_history_service::{HistoryAccess, HistoryFailure, HistoryOperation, RecordStore};
use device_history_store::{AppendRecord, DomainConfig, HistoryClock, HistorySchema, HistoryStore};
use device_protocol::HistoryFilter;
use device_storage::{
    StoredRecord,
    persistence::{AtomicSnapshot, Error, Geometry, Layout, Medium},
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

#[derive(Clone, Debug)]
struct Media(Rc<RefCell<Vec<u8>>>);
impl Medium for Media {
    fn geometry(&self) -> Geometry {
        Geometry {
            capacity: 8192,
            erase_size: 4096,
            write_size: 1,
        }
    }
    fn read(&mut self, offset: u32, out: &mut [u8]) -> Result<(), Error> {
        let at = offset as usize;
        out.copy_from_slice(&self.0.borrow()[at..at + out.len()]);
        Ok(())
    }
    fn erase(&mut self, offset: u32, length: u32) -> Result<(), Error> {
        self.0.borrow_mut()[offset as usize..(offset + length) as usize].fill(255);
        Ok(())
    }
    fn program(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Error> {
        for (a, b) in self.0.borrow_mut()[offset as usize..].iter_mut().zip(bytes) {
            *a &= *b;
        }
        Ok(())
    }
    fn sync(&mut self) -> Result<(), Error> {
        Ok(())
    }
}
#[derive(Debug)]
struct Schema;
impl HistorySchema for Schema {
    fn validate(&self, r: StoredRecord<'_>, parent: u64) -> Result<(), HistoryFailure> {
        if r.schema == 1 && r.payload.len() == 2 && u64::from(r.payload[0]) == parent {
            Ok(())
        } else {
            Err(HistoryFailure::Invalid)
        }
    }
}
struct Access;
impl HistoryAccess for Access {
    fn module_allowed(&self) -> bool {
        true
    }
    fn allowed(&self, _: [u8; 16], _: Option<u8>, _: HistoryOperation) -> bool {
        true
    }
}
#[derive(Clone, Debug)]
struct Clock(Rc<Cell<u64>>);
impl HistoryClock for Clock {
    fn now_ms(&self) -> Result<u64, HistoryFailure> {
        Ok(self.0.get())
    }
}
fn clock() -> Clock {
    Clock(Rc::new(Cell::new(100)))
}
type Store = HistoryStore<AtomicSnapshot<Media, 2048>, Schema, Clock, 2, 8, 16, 2048>;
fn snap(m: Media) -> AtomicSnapshot<Media, 2048> {
    AtomicSnapshot::open(
        m,
        Layout {
            start: 0,
            bank_bytes: 4096,
        },
    )
    .unwrap_or_else(|error| panic!("{error:?}"))
}
fn fresh() -> (Store, Media) {
    let m = Media(Rc::new(RefCell::new(vec![255; 8192])));
    let s = Store::initialize(
        snap(m.clone()),
        Schema,
        clock(),
        [7; 16],
        &[DomainConfig {
            domain: 1,
            epoch: 3,
        }],
        10,
    )
    .unwrap_or_else(|error| panic!("{error:?}"));
    (s, m)
}
fn record(parent: u8, time: i64, flags: u16) -> AppendRecord<'static> {
    let payload = match parent {
        1 => &[1, 11][..],
        2 => &[2, 22][..],
        _ => &[0, 33][..],
    };
    AppendRecord {
        domain: 1,
        record_type: 2,
        schema: 1,
        utc_ms: time,
        flags,
        parent_session: u64::from(parent),
        payload,
    }
}
fn filter() -> HistoryFilter {
    HistoryFilter {
        domain: 1,
        filter_flags: 0,
        start_utc_ms: 0,
        end_utc_ms: 0,
        record_type: 0,
        parent_session: 0,
    }
}

// Catches returning before snapshot commit or rebuilding IDs/watermarks from a volatile count.
#[test]
fn test_should_preserve_records_ack_identity_and_next_id_on_restart() {
    let (mut s, m) = fresh();
    assert_eq!(s.append(record(1, 100, 1)), Ok(1));
    let l = s
        .open(1, filter(), 20, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    s.record_admitted(l, 1, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(s.ack_atomic(l, filter(), 3, 1, 21, &Access), Ok(1));
    drop(s);
    let mut reopened =
        Store::open(snap(m), Schema, clock(), 10).unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(
        reopened
            .directory()
            .unwrap_or_else(|error| panic!("{error:?}"))
            .dataset,
        [7; 16]
    );
    let l = reopened
        .open(2, filter(), 30, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(l.committed_id, 1);
    assert_eq!(l.snapshot.epoch, 3);
    let mut body = [0; 16];
    let content = reopened
        .read(l, 1, &mut body, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(&body[..content.length], &[1, 11]);
    assert_eq!(reopened.append(record(2, 90, 1)), Ok(2));
}
// Catches selection by UTC ordering, invalid timestamps, truncation and moving upper bounds.
#[test]
fn test_should_freeze_actual_parent_time_matches_and_copy_whole_body() {
    let (mut s, _) = fresh();
    s.append(record(1, 100, 1))
        .unwrap_or_else(|error| panic!("{error:?}"));
    s.append(record(2, 80, 1))
        .unwrap_or_else(|error| panic!("{error:?}"));
    s.append(record(1, 0, 0))
        .unwrap_or_else(|error| panic!("{error:?}"));
    let f = HistoryFilter {
        filter_flags: 7,
        start_utc_ms: 70,
        end_utc_ms: 110,
        record_type: 2,
        parent_session: 1,
        ..filter()
    };
    let l = s
        .open(1, f, 0, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(
        (l.snapshot.first_id, l.snapshot.last_id, l.snapshot.count),
        (1, 1, 1)
    );
    s.append(record(1, 90, 1))
        .unwrap_or_else(|error| panic!("{error:?}"));
    let p = s
        .plan(l, 0, 16, 1, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(&p.ids[..usize::from(p.count)], &[1]);
    assert!(p.eof);
    assert_eq!(
        s.read(l, 1, &mut [0; 1], &Access),
        Err(HistoryFailure::Capacity)
    );
    assert_eq!(
        s.read(l, 2, &mut [0; 16], &Access),
        Err(HistoryFailure::NotFound)
    );
    assert_eq!(s.count(f, &Access), Ok((3, 2)));
}
// Catches numeric high-water inference, COUNT proof, duplicate-prefix reset and filtered retries.
#[test]
fn test_should_require_exact_admitted_prefix_for_every_ack() {
    let (mut s, _) = fresh();
    s.append(record(1, 100, 1))
        .unwrap_or_else(|error| panic!("{error:?}"));
    s.append(record(2, 200, 1))
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(s.count(filter(), &Access), Ok((3, 2)));
    let l = s
        .open(1, filter(), 0, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(
        s.ack_atomic(l, filter(), 3, 2, 1, &Access),
        Err(HistoryFailure::InvalidState)
    );
    assert_eq!(s.record_admitted(l, 2, &Access), Ok(()));
    assert_eq!(
        s.ack_atomic(l, filter(), 3, 2, 1, &Access),
        Err(HistoryFailure::InvalidState)
    );
    s.record_admitted(l, 1, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    s.record_admitted(l, 1, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    s.record_admitted(l, 2, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    s.record_admitted(l, 1, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(s.ack_atomic(l, filter(), 3, 2, 2, &Access), Ok(2));
    assert_eq!(s.ack_atomic(l, filter(), 3, 1, 3, &Access), Ok(2));
    s.close(l).unwrap_or_else(|error| panic!("{error:?}"));
    let f = HistoryFilter {
        filter_flags: 2,
        record_type: 2,
        ..filter()
    };
    let l = s
        .open(2, f, 4, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    s.record_admitted(l, 1, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(
        s.ack_atomic(l, f, 3, 1, 5, &Access),
        Err(HistoryFailure::Invalid)
    );
}
// Catches expiry checks that only sample time during a previous plan/renew.
#[test]
fn test_should_check_trusted_clock_on_read_and_clear_proof_after_regression() {
    let (s, m) = fresh();
    drop(s);
    let c = clock();
    let mut s =
        Store::open(snap(m), Schema, c.clone(), 10).unwrap_or_else(|error| panic!("{error:?}"));
    s.append(record(1, 100, 1))
        .unwrap_or_else(|error| panic!("{error:?}"));
    let l = s
        .open(1, filter(), 100, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    c.0.set(10_100);
    assert_eq!(
        s.read(l, 1, &mut [0; 16], &Access),
        Err(HistoryFailure::CursorExpired)
    );
    let l = s
        .open(2, filter(), 10_100, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    s.record_admitted(l, 1, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    c.0.set(10_099);
    assert_eq!(
        s.ack_atomic(l, filter(), 3, 1, 10_100, &Access),
        Err(HistoryFailure::Clock)
    );
    c.0.set(10_101);
    assert_eq!(
        s.record_admitted(l, 1, &Access),
        Err(HistoryFailure::CursorExpired)
    );
}
// Catches accepting forged snapshot metadata, stale recovery handles and revoked ACK scope.
#[test]
fn test_should_bind_complete_lease_and_live_ack_authorization() {
    struct ReadOnly;
    impl HistoryAccess for ReadOnly {
        fn module_allowed(&self) -> bool {
            true
        }
        fn allowed(&self, _: [u8; 16], _: Option<u8>, op: HistoryOperation) -> bool {
            op != HistoryOperation::Acknowledge
        }
    }
    let (mut s, _) = fresh();
    s.append(record(1, 100, 1))
        .unwrap_or_else(|error| panic!("{error:?}"));
    let l = s
        .open(1, filter(), 0, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    let mut forged = l;
    forged.snapshot.last_id = 9;
    assert_eq!(
        s.read(forged, 1, &mut [0; 16], &Access),
        Err(HistoryFailure::Identity)
    );
    s.record_admitted(l, 1, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(
        s.ack_atomic(l, filter(), 3, 1, 1, &ReadOnly),
        Err(HistoryFailure::Denied)
    );
    assert_eq!(
        s.ack_atomic(l, filter(), 3, 9, 2, &Access),
        Err(HistoryFailure::Invalid)
    );
    s.recover().unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(
        s.record_admitted(l, 1, &Access),
        Err(HistoryFailure::CursorExpired)
    );
    let new = s
        .open(2, filter(), 3, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_ne!(new.handle, l.handle);
    assert_eq!(new.committed_id, 0);
    assert_eq!(
        s.ack_atomic(new, filter(), 3, 1, 4, &Access),
        Err(HistoryFailure::InvalidState)
    );
}
// Catches automatic formatting, permissive parent/schema fallback and deadline wrap.
#[test]
fn test_should_require_explicit_nonzero_initialization_and_actual_schema() {
    let m = Media(Rc::new(RefCell::new(vec![255; 8192])));
    assert!(matches!(
        Store::open(snap(m.clone()), Schema, clock(), 10),
        Err(HistoryFailure::NotFound)
    ));
    assert!(matches!(
        Store::initialize(
            snap(m.clone()),
            Schema,
            clock(),
            [0; 16],
            &[DomainConfig {
                domain: 1,
                epoch: 3
            }],
            10
        ),
        Err(HistoryFailure::Invalid)
    ));
    let (mut s, _) = fresh();
    let mut r = record(1, 100, 1);
    r.parent_session = 2;
    assert_eq!(s.append(r), Err(HistoryFailure::Invalid));
    assert_eq!(s.append(record(1, 0, 0)), Ok(1));
    let s = s.into_snapshot();
    assert!(matches!(
        Store::initialize(
            s,
            Schema,
            clock(),
            [8; 16],
            &[DomainConfig {
                domain: 1,
                epoch: 3
            }],
            10
        ),
        Err(HistoryFailure::Conflict)
    ));
    let m = Media(Rc::new(RefCell::new(vec![255; 8192])));
    let c = Clock(Rc::new(Cell::new(u64::MAX)));
    let mut s = Store::initialize(
        snap(m),
        Schema,
        c,
        [7; 16],
        &[DomainConfig {
            domain: 1,
            epoch: 3,
        }],
        10,
    )
    .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(
        s.open(1, filter(), u64::MAX, &Access),
        Err(HistoryFailure::Clock)
    );
}
// Catches blanket successful cleanup for forged handles after proof was already cleared.
#[test]
fn test_should_close_only_exact_released_handle_without_acking() {
    let (mut s, _) = fresh();
    s.append(record(1, 100, 1))
        .unwrap_or_else(|error| panic!("{error:?}"));
    let l = s
        .open(1, filter(), 0, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    s.close(l).unwrap_or_else(|error| panic!("{error:?}"));
    s.close(l).unwrap_or_else(|error| panic!("{error:?}"));
    let mut forged = l;
    forged.handle += 1;
    assert_eq!(s.close(forged), Err(HistoryFailure::Identity));
    let l = s
        .open(2, filter(), 1, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(l.committed_id, 0);
}

// Catches renewing from stale request time, or treating renewal as permission to ACK.
#[test]
fn test_should_renew_from_actual_clock_without_advancing_durable_ack() {
    let (s, m) = fresh();
    drop(s);
    let c = clock();
    let mut s =
        Store::open(snap(m), Schema, c.clone(), 10).unwrap_or_else(|error| panic!("{error:?}"));
    s.append(record(1, 100, 1))
        .unwrap_or_else(|error| panic!("{error:?}"));
    let l = s
        .open(1, filter(), 0, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    c.0.set(9_000);
    s.renew(l, 1, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    c.0.set(10_100);
    assert!(s.read(l, 1, &mut [0; 16], &Access).is_ok());
    assert_eq!(
        s.ack_atomic(l, filter(), 3, 1, 2, &Access),
        Err(HistoryFailure::InvalidState)
    );
    c.0.set(19_000);
    assert_eq!(
        s.read(l, 1, &mut [0; 16], &Access),
        Err(HistoryFailure::CursorExpired)
    );
    s.close(l).unwrap_or_else(|error| panic!("{error:?}"));
    let new = s
        .open(2, filter(), 3, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(new.committed_id, 0);
}

// Catches overwriting retained leased bodies, or silently reclaiming them on ACK.
#[test]
fn test_should_return_capacity_and_retain_every_leased_or_acknowledged_record() {
    let (mut s, _) = fresh();
    for id in 1..=8 {
        assert_eq!(s.append(record(1, 100, 1)), Ok(id));
    }
    let l = s
        .open(1, filter(), 0, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(s.append(record(2, 200, 1)), Err(HistoryFailure::Capacity));
    for id in 1..=8 {
        s.record_admitted(l, id, &Access)
            .unwrap_or_else(|error| panic!("{error:?}"));
    }
    assert_eq!(s.ack_atomic(l, filter(), 3, 8, 1, &Access), Ok(8));
    assert_eq!(s.append(record(2, 200, 1)), Err(HistoryFailure::Capacity));
    let mut out = [0; 16];
    assert_eq!(
        s.read(l, 1, &mut out, &Access)
            .unwrap_or_else(|error| panic!("{error:?}"))
            .length,
        2
    );
    assert_eq!(&out[..2], &[1, 11]);
}

// Catches continuing metadata reads after actual persisted content no longer passes its validator.
#[test]
fn test_should_poison_on_actual_stored_schema_failure_until_validated_recovery() {
    #[derive(Debug)]
    struct MutableSchema(Rc<Cell<bool>>);
    impl HistorySchema for MutableSchema {
        fn validate(&self, r: StoredRecord<'_>, parent: u64) -> Result<(), HistoryFailure> {
            if self.0.get() {
                Schema.validate(r, parent)
            } else {
                Err(HistoryFailure::Invalid)
            }
        }
    }
    type Mutable = HistoryStore<AtomicSnapshot<Media, 2048>, MutableSchema, Clock, 2, 8, 16, 2048>;
    let (mut s, _) = fresh();
    s.append(record(1, 100, 1))
        .unwrap_or_else(|error| panic!("{error:?}"));
    let valid = Rc::new(Cell::new(true));
    let mut s = Mutable::open(s.into_snapshot(), MutableSchema(valid.clone()), clock(), 10)
        .unwrap_or_else(|error| panic!("{error:?}"));
    let l = s
        .open(1, filter(), 0, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    valid.set(false);
    assert_eq!(
        s.read(l, 1, &mut [0; 16], &Access),
        Err(HistoryFailure::StoredIntegrity)
    );
    assert!(s.directory().is_err());
    assert!(s.recover().is_err());
    valid.set(true);
    s.recover().unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(s.count(filter(), &Access), Ok((3, 1)));
    assert_eq!(
        s.record_admitted(l, 1, &Access),
        Err(HistoryFailure::CursorExpired)
    );
}

// Catches sharing one sequence counter across domains or rebuilding directory ranges from append order.
#[test]
fn test_should_allocate_independent_domain_ids_and_reopen_sorted_directory() {
    let m = Media(Rc::new(RefCell::new(vec![255; 8192])));
    let mut s = Store::initialize(
        snap(m.clone()),
        Schema,
        clock(),
        [7; 16],
        &[
            DomainConfig {
                domain: 1,
                epoch: 3,
            },
            DomainConfig {
                domain: 2,
                epoch: 9,
            },
        ],
        10,
    )
    .unwrap_or_else(|error| panic!("{error:?}"));
    let second = AppendRecord {
        domain: 2,
        ..record(1, 100, 1)
    };
    assert_eq!(s.append(second), Ok(1));
    assert_eq!(s.append(record(1, 200, 1)), Ok(1));
    assert_eq!(s.append(second), Ok(2));
    drop(s);
    let mut s =
        Store::open(snap(m), Schema, clock(), 10).unwrap_or_else(|error| panic!("{error:?}"));
    let directory = s.directory().unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(directory.domains[0].newest_id, 1);
    assert_eq!(directory.domains[1].newest_id, 2);
    let f = HistoryFilter {
        domain: 2,
        ..filter()
    };
    let l = s
        .open(1, f, 0, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(
        (
            l.snapshot.epoch,
            l.snapshot.first_id,
            l.snapshot.last_id,
            l.snapshot.count
        ),
        (9, 1, 2, 2)
    );
    let mut out = [0; 16];
    assert_eq!(
        s.read(l, 2, &mut out, &Access)
            .unwrap_or_else(|error| panic!("{error:?}"))
            .domain,
        2
    );
    assert_eq!(s.append(record(1, 300, 1)), Ok(2));
}

// Catches forcing record retransmission after an ACK committed but its reply was lost.
#[test]
fn test_should_retry_durable_ack_after_restart_without_readmitting_acknowledged_records() {
    let (mut s, m) = fresh();
    s.append(record(1, 100, 1))
        .unwrap_or_else(|error| panic!("{error:?}"));
    s.append(record(2, 200, 1))
        .unwrap_or_else(|error| panic!("{error:?}"));
    let old = s
        .open(1, filter(), 0, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    s.record_admitted(old, 1, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(s.ack_atomic(old, filter(), 3, 1, 1, &Access), Ok(1));
    drop(s);
    let mut s =
        Store::open(snap(m), Schema, clock(), 10).unwrap_or_else(|error| panic!("{error:?}"));
    let fresh = s
        .open(2, filter(), 2, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(fresh.committed_id, 1);
    assert_eq!(s.ack_atomic(fresh, filter(), 3, 1, 3, &Access), Ok(1));
    assert_eq!(
        s.ack_atomic(fresh, filter(), 3, 2, 4, &Access),
        Err(HistoryFailure::InvalidState)
    );
    let mut forged = fresh;
    forged.snapshot.snapshot = 3;
    assert_eq!(
        s.ack_atomic(forged, filter(), 3, 1, 5, &Access),
        Err(HistoryFailure::Identity)
    );
}

// Catches requiring retransmission of retained records below a durable ACK before resuming export.
#[test]
fn test_should_resume_after_durable_ack_on_restart_and_admit_only_new_records() {
    let (mut s, m) = fresh();
    assert_eq!(s.append(record(1, 100, 1)), Ok(1));
    assert_eq!(s.append(record(2, 200, 1)), Ok(2));
    let old = s
        .open(1, filter(), 0, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    s.record_admitted(old, 1, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(s.ack_atomic(old, filter(), 3, 1, 1, &Access), Ok(1));
    drop(s);
    let mut s =
        Store::open(snap(m), Schema, clock(), 10).unwrap_or_else(|error| panic!("{error:?}"));
    let fresh = s
        .open(2, filter(), 2, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(fresh.committed_id, 1);
    let plan = s
        .plan(fresh, 1, 16, 3, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!((plan.count, plan.ids[0], plan.eof), (1, 2, true));
    assert_eq!(
        s.ack_atomic(fresh, filter(), 3, 2, 4, &Access),
        Err(HistoryFailure::InvalidState)
    );
    assert_eq!(s.record_admitted(fresh, 2, &Access), Ok(()));
    assert_eq!(s.ack_atomic(fresh, filter(), 3, 2, 5, &Access), Ok(2));
}

// Catches forgetting a later actual clock sample when a request timestamp is
// invalid, allowing the next trusted-clock regression to escape detection.
#[test]
fn test_should_keep_actual_clock_floor_after_rejecting_future_request_time() {
    let (s, _) = fresh();
    let c = clock();
    let mut s = Store::open(s.into_snapshot(), Schema, c.clone(), 10)
        .unwrap_or_else(|error| panic!("{error:?}"));
    let lease = s
        .open(1, filter(), 100, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    c.0.set(200);
    assert_eq!(s.renew(lease, 300, &Access), Err(HistoryFailure::Clock));
    c.0.set(150);
    assert_eq!(
        s.open(2, filter(), 100, &Access),
        Err(HistoryFailure::Clock)
    );
    c.0.set(200);
    assert!(s.open(3, filter(), 100, &Access).is_ok());
}
