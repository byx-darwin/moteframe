//! Complete body, capacity, recovery and prepared adapter behavior.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod async_paged_support;
use async_paged_support::*;
use device_history_service::{HistoryFailure, Preparation, RecordStore};
use device_history_store::{DomainConfig, async_paged::JobResult};
#[test]
fn test_should_initialize_append_full_body_recover_and_prepare_without_sync_io() {
    let meta = Control::new(8192, 256, 4096);
    let log = Control::new(8192, 256, 4096);
    let clock = Clock::new();
    let s: Store<1, 4, 474, 144> = owner(&meta, &log, clock.clone(), 5);
    initialize(
        &s,
        &meta,
        &log,
        &[DomainConfig {
            domain: 0,
            epoch: 8,
        }],
    );
    let body = [13; 474];
    let t = s.begin_append(input(&body, 0)).unwrap();
    assert_eq!(
        run(&s, t, &[&meta, &log]),
        Ok(JobResult::Appended { id: 1 })
    );
    let t = s.begin_recover().unwrap();
    assert_eq!(
        run(&s, t, &[&meta, &log]),
        Ok(JobResult::Recovered { initialized: true })
    );
    assert_eq!(s.stats().unwrap().retained, [1]);
    let a = Access::yes();
    let mut adapter = s.adapter().unwrap();
    let f = filter(0);
    assert_eq!(adapter.open(1, f, 100, &a), Err(HistoryFailure::Busy));
    prepare(
        &s,
        &mut adapter,
        Preparation::Open {
            snapshot: 1,
            filter: f,
        },
        &a,
        &[&meta, &log],
    );
    let before = meta.calls.get() + log.calls.get();
    let lease = adapter.open(1, f, 100, &a).unwrap();
    assert_eq!(before, meta.calls.get() + log.calls.get());
    assert_eq!(lease.snapshot.count, 1);
    while !adapter.prefetch(lease, 1, &a).unwrap() {
        s.step().unwrap();
    }
    let mut out = [0; 474];
    assert_eq!(adapter.read(lease, 1, &mut out, &a).unwrap().length, 474);
    assert_eq!(out, body);
    adapter.record_admitted(lease, 1, &a).unwrap();
    prepare(
        &s,
        &mut adapter,
        Preparation::Ack {
            lease,
            filter: f,
            epoch: 8,
            upto: 1,
        },
        &a,
        &[&meta, &log],
    );
    assert_eq!(adapter.ack_atomic(lease, f, 8, 1, 100, &a), Ok(1));
}
#[test]
fn test_should_retain_1530_maximum_bodies_recover_and_reverse_all_255_admissions() {
    let meta = Control::new(8192, 256, 4096);
    let log = Control::new(1_593_344, 256, 4096);
    let clock = Clock::new();
    let s: Store<6> = owner(&meta, &log, clock.clone(), 259);
    let keys = [0, 1, 2, 5, 6, 7];
    let domains = keys.map(|domain| DomainConfig {
        domain,
        epoch: u32::from(domain) + 1,
    });
    initialize(&s, &meta, &log, &domains);
    for domain in keys {
        for id in 1_u64..=255 {
            let mut body = [domain; 474];
            body[..8].copy_from_slice(&id.to_le_bytes());
            assert_eq!(
                run(
                    &s,
                    s.begin_append(input(&body, domain)).unwrap(),
                    &[&meta, &log]
                ),
                Ok(JobResult::Appended { id })
            );
        }
        assert_eq!(
            s.begin_append(input(&[1; 474], domain)).unwrap_err(),
            HistoryFailure::Capacity
        );
    }
    assert_eq!(s.stats().unwrap().retained, [255; 6]);
    assert_eq!(
        run(&s, s.begin_recover().unwrap(), &[&meta, &log]),
        Ok(JobResult::Recovered { initialized: true })
    );
    let a = Access::yes();
    let mut adapter = s.adapter().unwrap();
    let f = filter(7);
    prepare(
        &s,
        &mut adapter,
        Preparation::Open {
            snapshot: 1,
            filter: f,
        },
        &a,
        &[&meta, &log],
    );
    let lease = adapter.open(1, f, 100, &a).unwrap();
    assert_eq!(lease.snapshot.count, 255);
    for id in (1..=255).rev() {
        while !adapter.prefetch(lease, id, &a).unwrap() {
            s.step().unwrap();
        }
        adapter.record_admitted(lease, id, &a).unwrap();
        if id > 1 {
            prepare(
                &s,
                &mut adapter,
                Preparation::Ack {
                    lease,
                    filter: f,
                    epoch: 8,
                    upto: 255,
                },
                &a,
                &[&meta, &log],
            );
            assert_eq!(
                adapter.ack_atomic(lease, f, 8, 255, 100, &a),
                Err(HistoryFailure::InvalidState)
            );
        }
    }
    prepare(
        &s,
        &mut adapter,
        Preparation::Ack {
            lease,
            filter: f,
            epoch: 8,
            upto: 255,
        },
        &a,
        &[&meta, &log],
    );
    assert_eq!(adapter.ack_atomic(lease, f, 8, 255, 100, &a), Ok(255));
    assert_eq!(s.stats().unwrap().retained, [255; 6]);
}
#[test]
fn test_should_keep_live_accepted_append_after_detach_and_reject_foreign_tokens() {
    let meta = Control::new(8192, 256, 4096);
    let log = Control::new(4096, 256, 4096);
    let s: Store<1, 4, 16, 144> = owner(&meta, &log, Clock::new(), 5);
    initialize(
        &s,
        &meta,
        &log,
        &[DomainConfig {
            domain: 0,
            epoch: 1,
        }],
    );
    let other: Store<1, 4, 16, 144> = owner(
        &Control::new(8192, 256, 4096),
        &Control::new(4096, 256, 4096),
        Clock::new(),
        5,
    );
    let token = s.begin_append(input(&[4; 16], 0)).unwrap();
    assert_eq!(other.result(token), Err(HistoryFailure::Identity));
    s.detach(token).unwrap();
    assert_eq!(s.result(token), Err(HistoryFailure::Identity));
    for _ in 0..1000 {
        s.step().unwrap();
        if s.stats().is_ok_and(|v| v.retained == [1]) {
            break;
        }
    }
    assert_eq!(s.stats().unwrap().retained, [1]);
    assert_eq!(
        run(&s, s.begin_recover().unwrap(), &[&meta, &log]),
        Ok(JobResult::Recovered { initialized: true })
    );
}
#[test]
fn test_should_reject_dirty_blank_and_committed_corruption_without_autoformat() {
    let meta = Control::new(8192, 256, 4096);
    let log = Control::new(4096, 256, 4096);
    log.bytes.borrow_mut()[100] = 0;
    let s: Store<1, 4, 16, 144> = owner(&meta, &log, Clock::new(), 5);
    assert_eq!(
        run(&s, s.begin_recover().unwrap(), &[&meta, &log]),
        Err(HistoryFailure::StoredIntegrity)
    );
    assert_eq!(
        s.begin_initialize(
            [3; 16],
            &[DomainConfig {
                domain: 0,
                epoch: 1
            }]
        )
        .unwrap_err(),
        HistoryFailure::InvalidState
    );
    log.bytes.borrow_mut()[100] = 255;
    initialize(
        &s,
        &meta,
        &log,
        &[DomainConfig {
            domain: 0,
            epoch: 1,
        }],
    );
    run(
        &s,
        s.begin_append(input(&[4; 16], 0)).unwrap(),
        &[&meta, &log],
    )
    .unwrap();
    log.bytes.borrow_mut()[80] ^= 1;
    assert_eq!(
        run(&s, s.begin_recover().unwrap(), &[&meta, &log]),
        Err(HistoryFailure::StoredIntegrity)
    );
    assert_eq!(s.stats(), Err(HistoryFailure::Storage));
}
#[test]
#[allow(clippy::too_many_lines)] // One explicit byte-for-byte cross-backend lifecycle keeps the compared bytes reviewable.
fn test_should_match_legacy_metadata_and_log_bytes_exactly_and_reopen_both_directions() {
    use device_history_store::paged::PagedHistoryStore;
    use device_storage::persistence::{AtomicSnapshot, Layout};
    let meta = Control::new(8192, 256, 4096);
    let log = Control::new(8192, 256, 4096);
    let clock = Clock::new();
    let s: Store<1, 4, 474, 144> = owner(&meta, &log, clock.clone(), 5);
    let domains = [DomainConfig {
        domain: 0,
        epoch: 8,
    }];
    initialize(&s, &meta, &log, &domains);
    let body = [42; 474];
    run(&s, s.begin_append(input(&body, 0)).unwrap(), &[&meta, &log]).unwrap();
    let syncmeta = Control::new(8192, 256, 4096);
    let synclog = Control::new(8192, 256, 4096);
    let snapshot = AtomicSnapshot::<_, 144>::open(
        SyncMem(syncmeta.clone()),
        Layout {
            start: 0,
            bank_bytes: 4096,
        },
    )
    .unwrap();
    let mut sync = PagedHistoryStore::<_, _, _, _, 1, 4, 474, 144>::initialize(
        snapshot,
        SyncMem(synclog.clone()),
        Schema,
        clock.clone(),
        config(5),
        [3; 16],
        &domains,
        60,
    )
    .unwrap();
    assert_eq!(sync.append(input(&body, 0)), Ok(1));
    assert_eq!(*meta.bytes.borrow(), *syncmeta.bytes.borrow());
    assert_eq!(*log.bytes.borrow(), *synclog.bytes.borrow());
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
    prepare(
        &s,
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
    assert_eq!(adapter.ack_atomic(lease, f, 8, 1, 100, &access), Ok(1));
    let l = sync.open(1, f, 100, &access).unwrap();
    sync.record_admitted(l, 1, &access).unwrap();
    sync.ack_atomic(l, f, 8, 1, 100, &access).unwrap();
    assert_eq!(*meta.bytes.borrow(), *syncmeta.bytes.borrow());
    assert_eq!(*log.bytes.borrow(), *synclog.bytes.borrow());
    drop(adapter);
    let from_sync: Store<1, 4, 474, 144> = owner(&syncmeta, &synclog, clock.clone(), 5);
    assert_eq!(
        run(
            &from_sync,
            from_sync.begin_recover().unwrap(),
            &[&syncmeta, &synclog]
        ),
        Ok(JobResult::Recovered { initialized: true })
    );
    drop(s);
    let snapshot = AtomicSnapshot::<_, 144>::open(
        SyncMem(meta),
        Layout {
            start: 0,
            bank_bytes: 4096,
        },
    )
    .unwrap();
    let old = PagedHistoryStore::<_, _, _, _, 1, 4, 474, 144>::open(
        snapshot,
        SyncMem(log),
        Schema,
        clock,
        config(5),
        60,
    )
    .unwrap();
    assert_eq!(old.stats().unwrap().retained, [1]);
}
#[test]
fn test_should_use_real_partition_handles_with_readonly_recovery_then_explicit_attestation() {
    use device_storage::{
        incremental::IncrementalSnapshot,
        partitions::{AsyncPartitions, Range},
        persistence::Layout,
    };
    let control = Control::new(16_384, 256, 4096);
    let mut broker = AsyncPartitions::new_unrecovered(
        control.medium(),
        [
            Range {
                start: 0,
                length: 8192,
                erase: true,
            },
            Range {
                start: 8192,
                length: 8192,
                erase: false,
            },
        ],
    )
    .unwrap();
    let (mut coordinator, [metadata, log]) = broker.split().unwrap();
    for _ in 0..100 {
        if coordinator.recover_step().is_ready() {
            break;
        }
    }
    let snapshot = IncrementalSnapshot::<_, 144>::new_unrecovered(
        metadata,
        Layout {
            start: 0,
            bank_bytes: 4096,
        },
    )
    .unwrap();
    let s=device_history_store::async_paged::AsyncPagedHistoryStore::<_,_,_,_,1,4,474,144>::new_unrecovered(snapshot,log,Schema,Clock::new(),config(5),60).unwrap();
    let token = s.begin_recover().unwrap();
    for _ in 0..1000 {
        let before = control.calls.get();
        s.step().unwrap();
        assert!(control.calls.get() - before <= 1);
        if s.result(token).unwrap().is_ready() {
            break;
        }
    }
    assert_eq!(
        s.result(token).unwrap(),
        core::task::Poll::Ready(Ok(JobResult::Recovered { initialized: false }))
    );
    coordinator.attest_normal().unwrap();
    let token = s
        .begin_initialize(
            [3; 16],
            &[DomainConfig {
                domain: 0,
                epoch: 8,
            }],
        )
        .unwrap();
    for _ in 0..1000 {
        s.step().unwrap();
        if s.result(token).unwrap().is_ready() {
            break;
        }
    }
    assert_eq!(
        s.result(token).unwrap(),
        core::task::Poll::Ready(Ok(JobResult::Initialized))
    );
    let token = s.begin_append(input(&[42; 474], 0)).unwrap();
    for _ in 0..1000 {
        let before = control.calls.get();
        s.step().unwrap();
        assert!(control.calls.get() - before <= 1);
        if s.result(token).unwrap().is_ready() {
            break;
        }
    }
    assert_eq!(
        s.result(token).unwrap(),
        core::task::Poll::Ready(Ok(JobResult::Appended { id: 1 }))
    );
}

#[test]
fn test_should_interleave_history_jobs_using_one_overwritten_transfer_page() {
    let am = Control::new(8192, 256, 4096);
    let al = Control::new(8192, 256, 4096);
    let bm = Control::new(8192, 256, 4096);
    let bl = Control::new(8192, 256, 4096);
    let a: Store<1, 4, 474, 144> = owner(&am, &al, Clock::new(), 5);
    let b: Store<1, 4, 16, 144> = owner(&bm, &bl, Clock::new(), 5);
    let domains = [DomainConfig {
        domain: 0,
        epoch: 8,
    }];
    initialize(&a, &am, &al, &domains);
    initialize(&b, &bm, &bl, &domains);
    let ta = a.begin_append(input(&[31; 474], 0)).unwrap();
    let tb = b.begin_append(input(&[67; 16], 0)).unwrap();
    let mut page = [0; 256];
    for _ in 0..10000 {
        page.fill(0x38);
        a.step_with_scratch(&mut page).unwrap();
        page.fill(0xcf);
        b.step_with_scratch(&mut page).unwrap();
        if matches!(a.result(ta).unwrap(), core::task::Poll::Ready(_))
            && matches!(b.result(tb).unwrap(), core::task::Poll::Ready(_))
        {
            break;
        }
    }
    assert_eq!(
        a.result(ta).unwrap(),
        core::task::Poll::Ready(Ok(JobResult::Appended { id: 1 }))
    );
    assert_eq!(
        b.result(tb).unwrap(),
        core::task::Poll::Ready(Ok(JobResult::Appended { id: 1 }))
    );
    assert_eq!(
        run(&a, a.begin_recover().unwrap(), &[&am, &al]),
        Ok(JobResult::Recovered { initialized: true })
    );
    assert_eq!(
        run(&b, b.begin_recover().unwrap(), &[&bm, &bl]),
        Ok(JobResult::Recovered { initialized: true })
    );
    assert_eq!(a.stats().unwrap().retained, [1]);
    assert_eq!(b.stats().unwrap().retained, [1]);
}

#[test]
fn test_should_keep_history_owner_within_retained_workspace_budget() {
    // Native provisioning additionally enforces the actual ARM linker budget.
    // The host fixture includes its two asynchronous NOR ports; a recovery-only
    // full-directory copy must not grow back into every resident owner.
    let size = core::mem::size_of::<Store<6>>();
    assert!(size <= 3100, "resident history owner: {size} bytes");
}

#[test]
fn test_should_hide_partial_plan_in_borrowed_and_copied_result_views() {
    use core::task::Poll;
    let meta = Control::new(8192, 256, 4096);
    let log = Control::new(8192, 256, 4096);
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
    for value in [27, 53] {
        run(
            &s,
            s.begin_append(input(&[value; 16], 0)).unwrap(),
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
    let request = Preparation::Plan {
        lease,
        after_id: 0,
        max: 2,
    };
    let token = s.begin_prepare(request, 100, &access).unwrap();
    let mut slices = 0;
    loop {
        if let Poll::Ready(result) = s.result(token).unwrap() {
            let JobResult::Planned(plan) = result.unwrap() else {
                panic!("wrong terminal kind")
            };
            assert_eq!(&plan.ids[..usize::from(plan.count)], &[1, 2]);
            break;
        }
        assert_eq!(
            s.inspect_result(token, |_| panic!("partial callback")),
            Ok(Poll::<Result<(), HistoryFailure>>::Pending)
        );
        assert_eq!(s.preparation_ready(token, 100, &access), Ok(false));
        s.step().unwrap();
        slices += 1;
        assert!(slices < 10000);
    }
    assert!(slices > 2);
    assert!(!adapter.prepare(request, 100, &access).unwrap());
    for _ in 0..10000 {
        assert_eq!(
            adapter.plan(lease, 0, 2, 100, &access),
            Err(HistoryFailure::Busy)
        );
        s.step().unwrap();
        if adapter.prepare(request, 100, &access).unwrap() {
            break;
        }
    }
    let plan = adapter.plan(lease, 0, 2, 100, &access).unwrap();
    assert_eq!(&plan.ids[..usize::from(plan.count)], &[1, 2]);
}
