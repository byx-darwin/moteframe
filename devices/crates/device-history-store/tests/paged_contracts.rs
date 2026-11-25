//! Independent paged `RecordStore` contracts using actual durable metadata/log.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod paged_service_support;
use device_history_service::{
    HistoryAccess, HistoryFailure, HistoryOperation, RecordStore, StoreLease,
};
use device_protocol::HistoryFilter;
use device_storage::StoredRecord;
use paged_service_support::*;

// Catches bit0/7/8/254 aliasing, cumulative-cursor proof, and duplicate toggles.
#[test]
fn test_should_preserve_all_255_ordinal_admission_bits_across_reverse_and_duplicate_delivery() {
    let control = MediumControl::blank();
    let mut media = control.media();
    let mut store = initialize(&mut media, Clock::new());
    for value in 1..=255u8 {
        assert_eq!(
            store.append(input(&[value, 0, 0, 1])).unwrap(),
            u64::from(value)
        );
    }
    let lease = store.open(1, filter(), 0, &Access).unwrap();
    assert_eq!(lease.snapshot.count, 255);
    let holes = [1, 8, 9, 255];
    for id in (1..=255).rev().filter(|id| !holes.contains(id)) {
        store.record_admitted(lease, id, &Access).unwrap();
        store.record_admitted(lease, id, &Access).unwrap();
    }
    for hole in [255, 9, 8] {
        store.record_admitted(lease, hole, &Access).unwrap();
        assert_eq!(
            store.ack_atomic(lease, filter(), 3, 255, 0, &Access),
            Err(HistoryFailure::InvalidState)
        );
    }
    store.record_admitted(lease, 1, &Access).unwrap();
    assert_eq!(
        store
            .ack_atomic(lease, filter(), 3, 255, 0, &Access)
            .unwrap(),
        255
    );
    assert_eq!(
        store.append(input(&[1, 0, 0, 1])),
        Err(HistoryFailure::Capacity)
    );
    store.close(lease).unwrap();
    store.recover().unwrap();
    assert_eq!(store.count(filter(), &Access).unwrap(), (3, 255));
    let fresh = store.open(2, filter(), 0, &Access).unwrap();
    assert_eq!(fresh.committed_id, 255);
    let writes = control.writes.get();
    assert_eq!(
        store
            .ack_atomic(fresh, filter(), 3, 255, 0, &Access)
            .unwrap(),
        255
    );
    assert_eq!(control.writes.get(), writes);
}

fn forge(lease: StoreLease, field: usize) -> StoreLease {
    let mut forged = lease;
    match field {
        0 => forged.handle += 1,
        1 => forged.snapshot.snapshot += 1,
        2 => forged.snapshot.epoch += 1,
        3 => forged.snapshot.first_id += 1,
        4 => forged.snapshot.last_id += 1,
        5 => forged.snapshot.count += 1,
        6 => forged.snapshot.lease_s += 1,
        7 => forged.committed_id += 1,
        _ => unreachable!(),
    }
    forged
}
// Catches checking only handle, snapshot, or range instead of the whole lease.
#[test]
fn test_should_bind_every_lease_field_on_read_plan_validation_admission_renew_ack_and_close() {
    let control = MediumControl::blank();
    let mut media = control.media();
    let mut store = initialize(&mut media, Clock::new());
    store.append(input(&[60, 0, 0, 1])).unwrap();
    let lease = store.open(1, filter(), 0, &Access).unwrap();
    store.record_admitted(lease, 1, &Access).unwrap();
    let record = StoredRecord {
        domain: 0x20,
        epoch: 3,
        record_id: 1,
        record_type: 0x20,
        schema: 1,
        utc_ms: 1000,
        flags: 1,
        payload: &[60, 0, 0, 1],
    };
    for field in 0..8 {
        let forged = forge(lease, field);
        assert_eq!(
            store.plan(forged, 0, 1, 0, &Access),
            Err(HistoryFailure::Identity),
            "plan field {field}"
        );
        assert_eq!(
            store.read(forged, 1, &mut [0; 16], &Access),
            Err(HistoryFailure::Identity),
            "read field {field}"
        );
        assert_eq!(
            store.validate_record(forged, record),
            Err(HistoryFailure::Identity),
            "validate field {field}"
        );
        assert_eq!(
            store.record_admitted(forged, 1, &Access),
            Err(HistoryFailure::Identity),
            "admission field {field}"
        );
        assert_eq!(
            store.renew(forged, 0, &Access),
            Err(HistoryFailure::Identity),
            "renew field {field}"
        );
        assert_eq!(
            store.ack_atomic(forged, filter(), 3, 1, 0, &Access),
            Err(HistoryFailure::Identity),
            "ack field {field}"
        );
        assert_eq!(
            store.close(forged),
            Err(HistoryFailure::Identity),
            "close field {field}"
        );
    }
    assert_eq!(
        store.ack_atomic(lease, filter(), 3, 1, 0, &Access).unwrap(),
        1
    );
    store.close(lease).unwrap();
    assert_eq!(store.close(lease), Ok(()));
    for field in 0..8 {
        assert_eq!(
            store.close(forge(lease, field)),
            Err(HistoryFailure::Identity)
        );
    }
}

#[derive(Debug)]
struct Restricted {
    module: bool,
    domain: Option<u8>,
    operation: HistoryOperation,
}
impl HistoryAccess for Restricted {
    fn module_allowed(&self) -> bool {
        self.module
    }
    fn allowed(&self, _: [u8; 16], domain: Option<u8>, operation: HistoryOperation) -> bool {
        !(self.domain == domain && self.operation == operation)
    }
}
// Catches bypassing fresh authorization/identity/filter checks for durable retries.
#[test]
fn test_should_guard_durable_ack_retry_with_live_module_dataset_domain_and_canonical_filter() {
    let control = MediumControl::blank();
    let mut media = control.media();
    let mut store = initialize(&mut media, Clock::new());
    store.append(input(&[60, 0, 0, 1])).unwrap();
    let lease = store.open(1, filter(), 0, &Access).unwrap();
    store.record_admitted(lease, 1, &Access).unwrap();
    store.ack_atomic(lease, filter(), 3, 1, 0, &Access).unwrap();
    let writes = control.writes.get();
    for domain in [None, Some(0x20)] {
        for operation in [HistoryOperation::Read, HistoryOperation::Acknowledge] {
            let access = Restricted {
                module: true,
                domain,
                operation,
            };
            assert_eq!(
                store.ack_atomic(lease, filter(), 3, 1, 0, &access),
                Err(HistoryFailure::Denied)
            );
        }
    }
    assert_eq!(
        store.ack_atomic(
            lease,
            filter(),
            3,
            1,
            0,
            &Restricted {
                module: false,
                domain: None,
                operation: HistoryOperation::Identity
            }
        ),
        Err(HistoryFailure::Denied)
    );
    assert_eq!(
        store.ack_atomic(lease, filter(), 4, 1, 0, &Access),
        Err(HistoryFailure::Conflict)
    );
    assert_eq!(
        store.ack_atomic(lease, filter(), 3, 0, 0, &Access),
        Err(HistoryFailure::Invalid)
    );
    assert_eq!(
        store.ack_atomic(lease, filter(), 3, 2, 0, &Access),
        Err(HistoryFailure::Invalid)
    );
    for changed in [
        HistoryFilter {
            filter_flags: 1,
            ..filter()
        },
        HistoryFilter {
            start_utc_ms: 1,
            ..filter()
        },
        HistoryFilter {
            end_utc_ms: 1,
            ..filter()
        },
        HistoryFilter {
            record_type: 0x20,
            ..filter()
        },
        HistoryFilter {
            parent_session: 1,
            ..filter()
        },
    ] {
        assert_eq!(
            store.ack_atomic(lease, changed, 3, 1, 0, &Access),
            Err(HistoryFailure::Invalid)
        );
    }
    assert_eq!(control.writes.get(), writes);
    assert_eq!(
        store.ack_atomic(lease, filter(), 3, 1, 0, &Access).unwrap(),
        1
    );
}

// Catches ordinal-based cursors, UTC sorting, changed filter bounds, and live append leakage.
#[test]
fn test_should_scan_frozen_filter_with_half_open_utc_parent_type_and_arbitrary_cursors() {
    let control = MediumControl::blank();
    let mut media = control.media();
    let mut store = initialize(&mut media, Clock::new());
    let rows = [
        (100, 1, 0x20, 7),
        (300, 1, 0x20, 7),
        (200, 1, 0x20, 7),
        (0, 0, 0x20, 7),
        (200, 1, 0x21, 7),
        (200, 1, 0x20, 8),
    ];
    for (value, &(utc_ms, flags, record_type, parent)) in rows.iter().enumerate() {
        let body = [u8::try_from(value).unwrap(), parent, 0, 1];
        store
            .append(device_history_store::AppendRecord {
                utc_ms,
                flags,
                record_type,
                ..input(&body)
            })
            .unwrap();
    }
    let selected = HistoryFilter {
        filter_flags: 7,
        start_utc_ms: 100,
        end_utc_ms: 300,
        record_type: 0x20,
        parent_session: 7,
        ..filter()
    };
    let lease = store.open(1, selected, 0, &Access).unwrap();
    assert_eq!(
        (
            lease.snapshot.first_id,
            lease.snapshot.last_id,
            lease.snapshot.count
        ),
        (1, 3, 2)
    );
    let late = store.plan(lease, 1, 16, 0, &Access).unwrap();
    assert_eq!(&late.ids[..usize::from(late.count)], &[3]);
    let early = store.plan(lease, 0, 16, 0, &Access).unwrap();
    assert_eq!(&early.ids[..usize::from(early.count)], &[1, 3]);
    assert!(early.eof);
    let empty = store.plan(lease, 3, 1, 0, &Access).unwrap();
    assert_eq!((empty.count, empty.end_after_id, empty.eof), (0, 3, true));
    assert_eq!(
        store.plan(lease, 4, 1, 0, &Access),
        Err(HistoryFailure::Invalid)
    );
    assert_eq!(
        store.read(lease, 2, &mut [0; 16], &Access),
        Err(HistoryFailure::NotFound)
    );
    assert_eq!(
        store.record_admitted(lease, 2, &Access),
        Err(HistoryFailure::InvalidState)
    );
    store
        .append(device_history_store::AppendRecord {
            utc_ms: 200,
            ..input(&[99, 7, 0, 1])
        })
        .unwrap();
    let frozen = store.plan(lease, 0, 16, 0, &Access).unwrap();
    assert_eq!(&frozen.ids[..usize::from(frozen.count)], &[1, 3]);
    assert_eq!(store.count(selected, &Access).unwrap(), (3, 3));
    assert_eq!(
        store.read(lease, 1, &mut [0; 3], &Access),
        Err(HistoryFailure::Capacity)
    );
    let mut body = [0; 16];
    let content = store.read(lease, 3, &mut body, &Access).unwrap();
    assert_eq!(&body[..content.length], &[2, 7, 0, 1]);
    store.record_admitted(lease, 1, &Access).unwrap();
    store.record_admitted(lease, 3, &Access).unwrap();
    assert_eq!(
        store.ack_atomic(lease, selected, 3, 3, 0, &Access),
        Err(HistoryFailure::Invalid)
    );
}

// Catches trusting supplied time on direct read and resurrecting admission on recover.
#[test]
fn test_should_expire_direct_reads_and_recover_without_proof_while_preserving_time_floors() {
    let control = MediumControl::blank();
    let clock = Clock::new();
    let mut media = control.media();
    let mut store = initialize(&mut media, clock.clone());
    store.append(input(&[60, 0, 0, 1])).unwrap();
    let lease = store.open(1, filter(), 0, &Access).unwrap();
    store.record_admitted(lease, 1, &Access).unwrap();
    clock.set(60_000);
    assert_eq!(
        store.read(lease, 1, &mut [0; 16], &Access),
        Err(HistoryFailure::CursorExpired)
    );
    assert!(store.record_admitted(lease, 1, &Access).is_err());
    assert_eq!(store.close(lease), Ok(()));
    let fresh = store.open(2, filter(), 60_000, &Access).unwrap();
    assert!(fresh.handle > lease.handle);
    store.record_admitted(fresh, 1, &Access).unwrap();
    clock.set(60_001);
    store.plan(fresh, 0, 1, 60_001, &Access).unwrap();
    store.recover().unwrap();
    assert!(store.record_admitted(fresh, 1, &Access).is_err());
    assert_eq!(
        store.open(3, filter(), 60_000, &Access),
        Err(HistoryFailure::Clock)
    );
    let current = store.open(4, filter(), 60_001, &Access).unwrap();
    assert!(current.handle > fresh.handle);
    assert_eq!(
        store.ack_atomic(current, filter(), 3, 1, 60_001, &Access),
        Err(HistoryFailure::InvalidState)
    );
    clock.set(60_000);
    assert_eq!(
        store.read(current, 1, &mut [0; 16], &Access),
        Err(HistoryFailure::Clock)
    );
    clock.set(60_001);
    assert!(store.record_admitted(current, 1, &Access).is_err());
}

// Catches losing actual-clock observations when rejecting a future client timestamp.
#[test]
fn test_should_keep_actual_clock_floor_on_rejected_future_time_and_release_on_clock_failure() {
    let control = MediumControl::blank();
    let clock = Clock::new();
    let mut media = control.media();
    let mut store = initialize(&mut media, clock.clone());
    store.append(input(&[60, 0, 0, 1])).unwrap();
    let lease = store.open(1, filter(), 0, &Access).unwrap();
    clock.set(100);
    assert_eq!(
        store.plan(lease, 0, 1, 101, &Access),
        Err(HistoryFailure::Clock)
    );
    clock.set(99);
    assert_eq!(
        store.read(lease, 1, &mut [0; 16], &Access),
        Err(HistoryFailure::Clock)
    );
    clock.set(100);
    let fresh = store.open(2, filter(), 100, &Access).unwrap();
    store.record_admitted(fresh, 1, &Access).unwrap();
    clock.0.set(None);
    assert_eq!(store.close(fresh), Err(HistoryFailure::Clock));
    clock.set(100);
    assert_eq!(store.close(fresh), Ok(()));
    let next = store.open(3, filter(), 100, &Access).unwrap();
    assert_eq!(
        store.ack_atomic(next, filter(), 3, 1, 100, &Access),
        Err(HistoryFailure::InvalidState)
    );
}

// Catches losing complete bodies or reusing a retained identity after restart.
#[test]
fn test_should_reopen_full_actual_bodies_and_preserve_next_identity() {
    let control = MediumControl::blank();
    let clock = Clock::new();
    {
        let mut media = control.media();
        let mut store = initialize(&mut media, clock.clone());
        store.append(input(&[60, 7, 0, 1])).unwrap();
        store.append(input(&[61, 8, 0, 1])).unwrap();
    }
    let mut media = control.media();
    let mut store = reopen(&mut media, clock);
    assert_eq!(store.directory().unwrap().dataset, [7; 16]);
    let lease = store.open(8, filter(), 0, &Access).unwrap();
    for (id, expected) in [(1, [60, 7, 0, 1]), (2, [61, 8, 0, 1])] {
        let mut body = [0; 16];
        let content = store.read(lease, id, &mut body, &Access).unwrap();
        assert_eq!(&body[..content.length], &expected);
    }
    store.close(lease).unwrap();
    assert_eq!(store.append(input(&[62, 9, 0, 1])).unwrap(), 3);
}

// Catches a noncanonical/oversize producer attempt destroying a live export proof.
#[test]
fn test_should_preserve_late_admission_after_prevalidation_failure_and_reject_forged_record() {
    let control = MediumControl::blank();
    let mut media = control.media();
    let mut store = initialize(&mut media, Clock::new());
    for value in [60, 61] {
        store.append(input(&[value, 0, 0, 1])).unwrap();
    }
    let lease = store.open(1, filter(), 0, &Access).unwrap();
    store.record_admitted(lease, 2, &Access).unwrap();
    let writes = control.writes.get();
    assert_eq!(store.append(input(&[0; 17])), Err(HistoryFailure::Capacity));
    assert_eq!(
        store.append(input(&[60, 0, 1, 1])),
        Err(HistoryFailure::StoredIntegrity)
    );
    assert_eq!(control.writes.get(), writes);
    let record = StoredRecord {
        domain: 0x20,
        epoch: 3,
        record_id: 1,
        record_type: 0x20,
        schema: 1,
        utc_ms: 1000,
        flags: 1,
        payload: &[60, 0, 0, 1],
    };
    for forged in [
        StoredRecord {
            payload: &[99, 0, 0, 1],
            ..record
        },
        StoredRecord {
            utc_ms: 2000,
            ..record
        },
        StoredRecord {
            schema: 2,
            ..record
        },
        StoredRecord {
            record_type: 0x21,
            ..record
        },
        StoredRecord {
            flags: 0,
            utc_ms: 0,
            ..record
        },
    ] {
        assert_eq!(
            store.validate_record(lease, forged),
            Err(HistoryFailure::StoredIntegrity)
        );
    }
    assert_eq!(store.validate_record(lease, record), Ok(()));
    store.record_admitted(lease, 1, &Access).unwrap();
    assert_eq!(
        store.ack_atomic(lease, filter(), 3, 2, 0, &Access).unwrap(),
        2
    );
}

#[derive(Debug)]
struct RevokeAtCommit {
    operation: HistoryOperation,
    calls: core::cell::Cell<u32>,
}
impl HistoryAccess for RevokeAtCommit {
    fn module_allowed(&self) -> bool {
        true
    }
    fn allowed(&self, _: [u8; 16], _: Option<u8>, operation: HistoryOperation) -> bool {
        if operation != self.operation {
            return true;
        }
        let calls = self.calls.get() + 1;
        self.calls.set(calls);
        calls < 3
    }
}
// Catches checking READ/ACK scope only at transaction entry, before record scans.
#[test]
fn test_should_recheck_read_and_ack_scopes_immediately_before_durable_ack() {
    for operation in [HistoryOperation::Read, HistoryOperation::Acknowledge] {
        let control = MediumControl::blank();
        let mut media = control.media();
        let mut store = initialize(&mut media, Clock::new());
        store.append(input(&[60, 0, 0, 1])).unwrap();
        let lease = store.open(1, filter(), 0, &Access).unwrap();
        store.record_admitted(lease, 1, &Access).unwrap();
        let writes = control.writes.get();
        let access = RevokeAtCommit {
            operation,
            calls: core::cell::Cell::new(0),
        };
        assert_eq!(
            store.ack_atomic(lease, filter(), 3, 1, 0, &access),
            Err(HistoryFailure::Denied)
        );
        assert_eq!(control.writes.get(), writes);
        store.close(lease).unwrap();
        let fresh = store.open(2, filter(), 0, &Access).unwrap();
        assert_eq!(fresh.committed_id, 0);
    }
}

// Catches treating an all-matching filter bit as canonical ACK authority.
#[test]
fn test_should_reject_ack_for_each_actual_filtered_lease_even_when_every_record_matches() {
    let control = MediumControl::blank();
    let mut media = control.media();
    let mut store = initialize(&mut media, Clock::new());
    for value in [60, 61] {
        store.append(input(&[value, 7, 0, 1])).unwrap();
    }
    for bits in 1..=7 {
        let selected = HistoryFilter {
            filter_flags: bits,
            start_utc_ms: if bits & 1 != 0 { 999 } else { 0 },
            end_utc_ms: if bits & 1 != 0 { 1001 } else { 0 },
            record_type: if bits & 2 != 0 { 0x20 } else { 0 },
            parent_session: if bits & 4 != 0 { 7 } else { 0 },
            ..filter()
        };
        let lease = store.open(u32::from(bits), selected, 0, &Access).unwrap();
        assert_eq!(lease.snapshot.count, 2);
        store.record_admitted(lease, 2, &Access).unwrap();
        store.record_admitted(lease, 1, &Access).unwrap();
        assert_eq!(
            store.ack_atomic(lease, selected, 3, 2, 0, &Access),
            Err(HistoryFailure::Invalid)
        );
        store.close(lease).unwrap();
    }
    let lease = store.open(8, filter(), 0, &Access).unwrap();
    assert_eq!(lease.committed_id, 0);
}
