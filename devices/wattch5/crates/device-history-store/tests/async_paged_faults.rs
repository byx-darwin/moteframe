//! Failure cuts exercise the real asynchronous log and DSNAP metadata owner.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod async_paged_support;
use async_paged_support::*;
use device_history_service::{HistoryFailure, Preparation, RecordStore};
use device_history_store::{DomainConfig, async_paged::JobResult};
#[test]
fn test_should_recover_old_or_new_at_every_append_program_byte() {
    for cut in 0..=512 {
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
        log.cut.set(Some(cut));
        let outcome = run(
            &s,
            s.begin_append(input(&[4; 16], 0)).unwrap(),
            &[&meta, &log],
        );
        if cut < 512 {
            assert_eq!(outcome, Err(HistoryFailure::Storage));
            assert!(s.stats().is_err());
        } else {
            assert_eq!(outcome, Ok(JobResult::Appended { id: 1 }));
        }
        log.cut.set(None);
        assert_eq!(
            run(&s, s.begin_recover().unwrap(), &[&meta, &log]),
            Ok(JobResult::Recovered { initialized: true })
        );
        let stats = s.stats().unwrap();
        assert!(stats.retained[0] <= 1);
        assert_eq!(stats.retained[0], u32::from(cut == 512));
        assert_eq!(stats.consumed[0], u32::from(cut != 0));
        assert_eq!(
            run(
                &s,
                s.begin_append(input(&[9; 16], 0)).unwrap(),
                &[&meta, &log]
            ),
            Ok(JobResult::Appended {
                id: if cut == 512 { 2 } else { 1 }
            })
        );
    }
}
#[test]
fn test_should_settle_old_or_new_at_every_buffered_log_flush_byte() {
    for cut in 0..=352 {
        let meta = Control::new(8192, 256, 4096);
        let log = Control::new(4096, 256, 4096);
        log.buffered.set(true);
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
        log.sync_cut.set(Some(cut));
        let _ = run(
            &s,
            s.begin_append(input(&[4; 16], 0)).unwrap(),
            &[&meta, &log],
        );
        log.sync_cut.set(None);
        log.power_loss();
        let reopened: Store<1, 4, 16, 144> = owner(&meta, &log, Clock::new(), 5);
        assert_eq!(
            run(&reopened, reopened.begin_recover().unwrap(), &[&meta, &log]),
            Ok(JobResult::Recovered { initialized: true })
        );
        assert!(reopened.stats().unwrap().retained[0] <= 1);
    }
}
#[test]
fn test_should_poison_every_failed_append_operation_and_require_recovery() {
    for at in 0..40 {
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
        log.fail.set(Some(log.calls.get() + at));
        let result = run(
            &s,
            s.begin_append(input(&[4; 16], 0)).unwrap(),
            &[&meta, &log],
        );
        log.fail.set(None);
        if result.is_err() {
            assert_eq!(s.stats(), Err(HistoryFailure::Storage));
        }
        assert_eq!(
            run(&s, s.begin_recover().unwrap(), &[&meta, &log]),
            Ok(JobResult::Recovered { initialized: true })
        );
        assert!(s.stats().unwrap().retained[0] <= 1);
    }
}
#[test]
fn test_should_keep_durable_ack_work_after_disconnect_at_each_metadata_program_byte() {
    ack_program_cuts::<16, 144>();
}
#[test]
fn test_should_keep_durable_ack_work_after_disconnect_at_each_metadata_program_byte_with_record_metadata_workspace()
 {
    ack_program_cuts::<474, 464>();
}
fn ack_program_cuts<const B: usize, const N: usize>() {
    for cut in 0..=768 {
        let meta = Control::new(8192, 256, 4096);
        let log = Control::new(if B > 16 { 8192 } else { 4096 }, 256, 4096);
        let clock = Clock::new();
        let s: Store<1, 4, B, N> = owner(&meta, &log, clock.clone(), 5);
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
        let a = Access::yes();
        let mut adapter = s.adapter().unwrap();
        let f = filter(0);
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
        while !adapter.prefetch(lease, 1, &a).unwrap() {
            s.step().unwrap();
        }
        adapter.record_admitted(lease, 1, &a).unwrap();
        let request = Preparation::Ack {
            lease,
            filter: f,
            epoch: 1,
            upto: 1,
        };
        let before = meta.calls.get();
        meta.cut.set(Some(cut));
        assert!(!adapter.prepare(request, 100, &a).unwrap());
        for _ in 0..1000 {
            s.step().unwrap();
            let _ = adapter.prepare(request, 100, &a).unwrap();
            if meta.calls.get() > before {
                break;
            }
        }
        assert!(meta.calls.get() > before, "ACK metadata was never admitted");
        drop(adapter);
        for _ in 0..1000 {
            s.step().unwrap();
        }
        meta.cut.set(None);
        assert_eq!(
            run(&s, s.begin_recover().unwrap(), &[&meta, &log]),
            Ok(JobResult::Recovered { initialized: true })
        );
        let mut fresh = s.adapter().unwrap();
        prepare(
            &s,
            &mut fresh,
            Preparation::Open {
                snapshot: 2,
                filter: f,
            },
            &a,
            &[&meta, &log],
        );
        assert!(fresh.open(2, f, 100, &a).unwrap().committed_id <= 1);
    }
}
#[test]
fn test_should_reject_wrong_completion_and_retry_busy_without_restarting_admitted_work() {
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
    log.busy.set(5);
    let t = s.begin_append(input(&[4; 16], 0)).unwrap();
    assert_eq!(
        s.begin_append(input(&[4; 16], 0)).unwrap_err(),
        HistoryFailure::Busy
    );
    assert_eq!(
        run(&s, t, &[&meta, &log]),
        Ok(JobResult::Appended { id: 1 })
    );
    assert_eq!(
        s.result(t).unwrap(),
        core::task::Poll::Ready(Ok(JobResult::Appended { id: 1 }))
    );
    let mut invalid = input(&[4; 16], 0);
    invalid.schema = 0;
    assert_eq!(
        s.begin_append(invalid).unwrap_err(),
        HistoryFailure::Invalid
    );
    assert_eq!(
        s.result(t).unwrap(),
        core::task::Poll::Ready(Ok(JobResult::Appended { id: 1 }))
    );
    log.wrong.set(true);
    let t = s.begin_append(input(&[4; 16], 0)).unwrap();
    assert_eq!(run(&s, t, &[&meta, &log]), Err(HistoryFailure::Storage));
    assert!(s.stats().is_err());
    assert_eq!(
        run(&s, s.begin_recover().unwrap(), &[&meta, &log]),
        Ok(JobResult::Recovered { initialized: true })
    );
    assert_eq!(s.stats().unwrap().retained, [1]);
}
#[test]
fn test_should_separate_visible_and_durable_ack_at_each_flush_cut() {
    ack_flush_cuts::<16, 144>();
}
#[test]
fn test_should_separate_visible_and_durable_ack_at_each_flush_cut_with_record_metadata_workspace() {
    ack_flush_cuts::<474, 464>();
}
fn ack_flush_cuts<const B: usize, const N: usize>() {
    for cut in 0..=464 {
        let meta = Control::new(8192, 256, 4096);
        let log = Control::new(if B > 16 { 8192 } else { 4096 }, 256, 4096);
        let s: Store<1, 4, B, N> = owner(&meta, &log, Clock::new(), 5);
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
        let a = Access::yes();
        let f = filter(0);
        let mut adapter = s.adapter().unwrap();
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
        while !adapter.prefetch(lease, 1, &a).unwrap() {
            s.step().unwrap();
        }
        adapter.record_admitted(lease, 1, &a).unwrap();
        meta.buffered.set(true);
        meta.sync_cut.set(Some(cut));
        prepare(
            &s,
            &mut adapter,
            Preparation::Ack {
                lease,
                filter: f,
                epoch: 1,
                upto: 1,
            },
            &a,
            &[&meta, &log],
        );
        let outcome = adapter.ack_atomic(lease, f, 1, 1, 100, &a);
        drop(adapter);
        meta.sync_cut.set(None);
        meta.power_loss();
        assert_eq!(
            run(&s, s.begin_recover().unwrap(), &[&meta, &log]),
            Ok(JobResult::Recovered { initialized: true })
        );
        let mut fresh = s.adapter().unwrap();
        prepare(
            &s,
            &mut fresh,
            Preparation::Open {
                snapshot: 2,
                filter: f,
            },
            &a,
            &[&meta, &log],
        );
        let actual = fresh.open(2, f, 100, &a).unwrap().committed_id;
        assert!(actual <= 1);
        if outcome.is_ok() {
            assert_eq!(actual, 1);
        }
    }
}
