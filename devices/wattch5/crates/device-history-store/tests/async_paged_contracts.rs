//! Frozen filters, permission/clock rechecks, schema and antirollback floors.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod async_paged_support;
use async_paged_support::*;
use device_history_service::{HistoryFailure, Preparation, RecordStore};
use device_history_store::DomainConfig;
#[test]
fn test_should_keep_frozen_filter_actual_order_and_disallow_all_filtered_ack() {
    let meta = Control::new(8192, 256, 4096);
    let log = Control::new(8192, 256, 4096);
    let s: Store<1, 8, 16, 144> = owner(&meta, &log, Clock::new(), 9);
    initialize(
        &s,
        &meta,
        &log,
        &[DomainConfig {
            domain: 0,
            epoch: 8,
        }],
    );
    for utc in [300, 100, 200, 400] {
        let mut r = input(&[42; 16], 0);
        r.flags = 1;
        r.utc_ms = utc;
        run(&s, s.begin_append(r).unwrap(), &[&meta, &log]).unwrap();
    }
    let access = Access::yes();
    let mut adapter = s.adapter().unwrap();
    let mut f = filter(0);
    f.filter_flags = 1;
    f.start_utc_ms = 100;
    f.end_utc_ms = 400;
    prepare(
        &s,
        &mut adapter,
        Preparation::Open {
            snapshot: 1,
            filter: f,
        },
        &access,
        &[&meta, &log],
    );
    let lease = adapter.open(1, f, 100, &access).unwrap();
    assert_eq!(lease.snapshot.count, 3);
    assert_eq!(lease.snapshot.first_id, 1);
    assert_eq!(lease.snapshot.last_id, 3);
    prepare(
        &s,
        &mut adapter,
        Preparation::Plan {
            lease,
            after_id: 0,
            max: 16,
        },
        &access,
        &[&meta, &log],
    );
    let plan = adapter.plan(lease, 0, 16, 100, &access).unwrap();
    assert_eq!(plan.ids[..3], [1, 2, 3]);
    assert!(plan.eof);
    for id in [3, 1, 2] {
        while !adapter.prefetch(lease, id, &access).unwrap() {
            s.step().unwrap();
        }
        adapter.record_admitted(lease, id, &access).unwrap();
        adapter.record_admitted(lease, id, &access).unwrap();
    }
    assert!(
        adapter
            .prepare(
                Preparation::Ack {
                    lease,
                    filter: f,
                    epoch: 8,
                    upto: 3
                },
                100,
                &access
            )
            .unwrap()
    );
    assert_eq!(
        adapter.ack_atomic(lease, f, 8, 3, 100, &access),
        Err(HistoryFailure::Invalid)
    );
}
#[test]
fn test_should_recheck_scope_immediately_before_metadata_admission_and_preserve_clock_floor() {
    let meta = Control::new(8192, 256, 4096);
    let log = Control::new(4096, 256, 4096);
    let clock = Clock::new();
    let s: Store<1, 4, 16, 144> = owner(&meta, &log, clock.clone(), 5);
    initialize(
        &s,
        &meta,
        &log,
        &[DomainConfig {
            domain: 0,
            epoch: 8,
        }],
    );
    run(
        &s,
        s.begin_append(input(&[42; 16], 0)).unwrap(),
        &[&meta, &log],
    )
    .unwrap();
    let access = Access::yes();
    let mut adapter = s.adapter().unwrap();
    let f = filter(0);
    prepare(
        &s,
        &mut adapter,
        Preparation::Open {
            snapshot: 1,
            filter: f,
        },
        &access,
        &[&meta, &log],
    );
    let lease = adapter.open(1, f, 100, &access).unwrap();
    while !adapter.prefetch(lease, 1, &access).unwrap() {
        s.step().unwrap();
    }
    adapter.record_admitted(lease, 1, &access).unwrap();
    let request = Preparation::Ack {
        lease,
        filter: f,
        epoch: 8,
        upto: 1,
    };
    let token = s.begin_prepare(request, 100, &access).unwrap();
    for _ in 0..100 {
        s.step().unwrap();
    }
    let before = meta.calls.get();
    access.0.set(false);
    assert!(s.preparation_ready(token, 100, &access).unwrap());
    assert_eq!(
        s.result(token).unwrap(),
        core::task::Poll::Ready(Err(HistoryFailure::Denied))
    );
    assert_eq!(meta.calls.get(), before);
    access.0.set(true);
    adapter.close(lease).unwrap();
    clock.0.set(Some(200));
    assert_eq!(
        s.begin_prepare(
            Preparation::Open {
                snapshot: 2,
                filter: f
            },
            300,
            &access
        )
        .unwrap_err(),
        HistoryFailure::Clock
    );
    clock.0.set(Some(150));
    assert_eq!(
        s.begin_prepare(
            Preparation::Open {
                snapshot: 2,
                filter: f
            },
            150,
            &access
        )
        .unwrap_err(),
        HistoryFailure::Clock
    );
}
#[test]
fn test_should_reject_schema_parent_and_same_owner_log_or_metadata_rollback() {
    let meta = Control::new(8192, 256, 4096);
    let log = Control::new(4096, 256, 4096);
    let s: Store<1, 4, 16, 144> = owner(&meta, &log, Clock::new(), 5);
    initialize(
        &s,
        &meta,
        &log,
        &[DomainConfig {
            domain: 0,
            epoch: 8,
        }],
    );
    let old_meta = meta.bytes.borrow().clone();
    let old_log = log.bytes.borrow().clone();
    let mut wrong = input(&[42; 16], 0);
    wrong.parent_session = 0;
    assert_eq!(s.begin_append(wrong).unwrap_err(), HistoryFailure::Invalid);
    wrong.parent_session = 7;
    wrong.schema = 2;
    assert_eq!(s.begin_append(wrong).unwrap_err(), HistoryFailure::Invalid);
    run(
        &s,
        s.begin_append(input(&[42; 16], 0)).unwrap(),
        &[&meta, &log],
    )
    .unwrap();
    log.bytes.borrow_mut().clone_from(&old_log);
    assert_eq!(
        run(&s, s.begin_recover().unwrap(), &[&meta, &log]),
        Err(HistoryFailure::StoredIntegrity)
    );
    // A fresh owner has no previous live floor, then publishing a durable ACK pins revision/root.
    let fresh: Store<1, 4, 16, 144> = owner(&meta, &log, Clock::new(), 5);
    run(&fresh, fresh.begin_recover().unwrap(), &[&meta, &log]).unwrap();
    run(
        &fresh,
        fresh.begin_append(input(&[42; 16], 0)).unwrap(),
        &[&meta, &log],
    )
    .unwrap();
    let access = Access::yes();
    let mut adapter = fresh.adapter().unwrap();
    let f = filter(0);
    prepare(
        &fresh,
        &mut adapter,
        Preparation::Open {
            snapshot: 1,
            filter: f,
        },
        &access,
        &[&meta, &log],
    );
    let lease = adapter.open(1, f, 100, &access).unwrap();
    while !adapter.prefetch(lease, 1, &access).unwrap() {
        fresh.step().unwrap();
    }
    adapter.record_admitted(lease, 1, &access).unwrap();
    prepare(
        &fresh,
        &mut adapter,
        Preparation::Ack {
            lease,
            filter: f,
            epoch: 8,
            upto: 1,
        },
        &access,
        &[&meta, &log],
    );
    adapter.ack_atomic(lease, f, 8, 1, 100, &access).unwrap();
    drop(adapter);
    meta.bytes.borrow_mut().clone_from(&old_meta);
    assert_eq!(
        run(&fresh, fresh.begin_recover().unwrap(), &[&meta, &log]),
        Err(HistoryFailure::StoredIntegrity)
    );
    assert_eq!(fresh.stats(), Err(HistoryFailure::Storage));
}
#[test]
fn test_should_reject_cross_connection_lease_cache_and_preparation_consumption() {
    let meta = Control::new(8192, 256, 4096);
    let log = Control::new(4096, 256, 4096);
    let s: Store<1, 4, 16, 144> = owner(&meta, &log, Clock::new(), 5);
    initialize(
        &s,
        &meta,
        &log,
        &[DomainConfig {
            domain: 0,
            epoch: 8,
        }],
    );
    run(
        &s,
        s.begin_append(input(&[42; 16], 0)).unwrap(),
        &[&meta, &log],
    )
    .unwrap();
    let access = Access::yes();
    let mut first = s.adapter().unwrap();
    let f = filter(0);
    prepare(
        &s,
        &mut first,
        Preparation::Open {
            snapshot: 1,
            filter: f,
        },
        &access,
        &[&meta, &log],
    );
    let lease = first.open(1, f, 100, &access).unwrap();
    while !first.prefetch(lease, 1, &access).unwrap() {
        s.step().unwrap();
    }
    let mut second = s.adapter().unwrap();
    assert_eq!(
        second.read(lease, 1, &mut [0; 16], &access),
        Err(HistoryFailure::Identity)
    );
    assert!(
        second
            .prepare(
                Preparation::Plan {
                    lease,
                    after_id: 0,
                    max: 1
                },
                100,
                &access
            )
            .unwrap()
    );
    assert_eq!(
        second.plan(lease, 0, 1, 100, &access),
        Err(HistoryFailure::Identity)
    );
    assert_eq!(
        second.renew(lease, 100, &access),
        Err(HistoryFailure::Identity)
    );
    assert_eq!(second.close(lease), Err(HistoryFailure::Identity));
    first.record_admitted(lease, 1, &access).unwrap();
    first.close(lease).unwrap();
}
#[test]
fn test_should_retire_only_record_work_preserving_prior_admission_and_accepted_ack() {
    let meta = Control::new(8192, 256, 4096);
    let log = Control::new(4096, 256, 4096);
    let s: Store<1, 4, 16, 144> = owner(&meta, &log, Clock::new(), 5);
    initialize(
        &s,
        &meta,
        &log,
        &[DomainConfig {
            domain: 0,
            epoch: 8,
        }],
    );
    for byte in [42, 43] {
        run(
            &s,
            s.begin_append(input(&[byte; 16], 0)).unwrap(),
            &[&meta, &log],
        )
        .unwrap();
    }
    let access = Access::yes();
    let mut adapter = s.adapter().unwrap();
    let f = filter(0);
    prepare(
        &s,
        &mut adapter,
        Preparation::Open {
            snapshot: 1,
            filter: f,
        },
        &access,
        &[&meta, &log],
    );
    let lease = adapter.open(1, f, 100, &access).unwrap();
    while !adapter.prefetch(lease, 1, &access).unwrap() {
        s.step().unwrap();
    }
    adapter.record_admitted(lease, 1, &access).unwrap();
    assert!(!adapter.prefetch(lease, 2, &access).unwrap());
    s.step().unwrap();
    let calls = log.calls.get();
    assert!(!adapter.retire_record(lease, 2, &access).unwrap());
    assert_eq!(log.calls.get(), calls);
    assert_eq!(
        s.begin_append(input(&[44; 16], 0)).unwrap_err(),
        HistoryFailure::Busy
    );
    while !adapter.retire_record(lease, 2, &access).unwrap() {
        let before = log.calls.get();
        s.step().unwrap();
        assert!(log.calls.get() - before <= 1);
    }
    prepare(
        &s,
        &mut adapter,
        Preparation::Ack {
            lease,
            filter: f,
            epoch: 8,
            upto: 2,
        },
        &access,
        &[&meta, &log],
    );
    assert_eq!(
        adapter.ack_atomic(lease, f, 8, 2, 100, &access),
        Err(HistoryFailure::InvalidState)
    );
    let ack = Preparation::Ack {
        lease,
        filter: f,
        epoch: 8,
        upto: 1,
    };
    let before = meta.calls.get();
    assert!(!adapter.prepare(ack, 100, &access).unwrap());
    while meta.calls.get() == before {
        s.step().unwrap();
        adapter.prepare(ack, 100, &access).unwrap();
    }
    assert!(adapter.retire_record(lease, 2, &access).unwrap());
    while !adapter.prepare(ack, 100, &access).unwrap() {
        s.step().unwrap();
    }
    assert_eq!(adapter.ack_atomic(lease, f, 8, 1, 100, &access), Ok(1));
}
#[test]
fn test_should_clean_matching_terminal_prefetch_error_without_masking_storage_failure() {
    let meta = Control::new(8192, 256, 4096);
    let log = Control::new(4096, 256, 4096);
    let s: Store<1, 4, 16, 144> = owner(&meta, &log, Clock::new(), 5);
    initialize(
        &s,
        &meta,
        &log,
        &[DomainConfig {
            domain: 0,
            epoch: 8,
        }],
    );
    run(
        &s,
        s.begin_append(input(&[42; 16], 0)).unwrap(),
        &[&meta, &log],
    )
    .unwrap();
    let access = Access::yes();
    let mut adapter = s.adapter().unwrap();
    let f = filter(0);
    prepare(
        &s,
        &mut adapter,
        Preparation::Open {
            snapshot: 1,
            filter: f,
        },
        &access,
        &[&meta, &log],
    );
    let lease = adapter.open(1, f, 100, &access).unwrap();
    assert!(!adapter.prefetch(lease, 1, &access).unwrap());
    log.wrong.set(true);
    for _ in 0..10 {
        s.step().unwrap();
    }
    assert_eq!(
        adapter.retire_record(lease, 1, &access),
        Err(HistoryFailure::Storage)
    );
    assert!(adapter.retire_record(lease, 1, &access).unwrap());
    assert_eq!(s.stats(), Err(HistoryFailure::Storage));
    drop(adapter);
    run(&s, s.begin_recover().unwrap(), &[&meta, &log]).unwrap();
    assert_eq!(s.stats().unwrap().retained, [1]);
}
