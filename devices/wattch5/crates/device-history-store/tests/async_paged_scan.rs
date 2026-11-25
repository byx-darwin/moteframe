//! Coalesced scans retain complete byte coverage and corruption detection.
#![allow(clippy::unwrap_used)]
mod async_paged_support;
use async_paged_support::*;
use device_history_service::{HistoryFailure, Preparation, RecordStore};
use device_history_store::{DomainConfig, async_paged::JobResult};
fn seeded<const B: usize>(w: u32, body: &[u8]) -> (Store<1, 4, B, 144>, Control, Control) {
    let meta = Control::new(8192, w, 4096);
    let log = Control::new(8192, w, 4096);
    let s = owner(&meta, &log, Clock::new(), 4);
    initialize(
        &s,
        &meta,
        &log,
        &[DomainConfig {
            domain: 0,
            epoch: 8,
        }],
    );
    for _ in 0..4 {
        run(&s, s.begin_append(input(body, 0)).unwrap(), &[&meta, &log]).unwrap();
    }
    (s, meta, log)
}
#[test]
fn test_should_scan_four_complete_slots_with_four_reads_each_and_no_byte_omission() {
    for length in [1, 176, 177, 432, 433, 474] {
        let (s, meta, log) = seeded::<474>(256, &vec![42; length]);
        log.reads.borrow_mut().clear();
        assert_eq!(
            run(
                &s,
                s.begin_prepare(
                    Preparation::Count { filter: filter(0) },
                    100,
                    &Access::yes()
                )
                .unwrap(),
                &[&meta, &log]
            ),
            Ok(JobResult::Counted { epoch: 8, count: 4 })
        );
        let reads = log.reads.borrow();
        assert_eq!(
            reads.len(),
            16,
            "each complete slot needs four contiguous reads, payload length {length}"
        );
        let mut coverage = vec![0u8; 4096];
        for &(offset, n) in reads.iter() {
            for byte in &mut coverage[offset as usize..offset as usize + n] {
                *byte += 1;
            }
        }
        assert!(
            coverage.iter().all(|v| *v == 1),
            "every header/body/padding/marker byte exactly once"
        );
    }
}
#[test]
fn test_should_preserve_corruption_rejection_and_torn_marker_exclusion() {
    for offset in [0, 77, 80, 111, 255, 256, 511, 512, 767, 768, 1023] {
        let (s, meta, log) = seeded::<474>(256, &[42; 31]);
        log.bytes.borrow_mut()[offset] ^= 1;
        assert_eq!(
            run(
                &s,
                s.begin_prepare(
                    Preparation::Count { filter: filter(0) },
                    100,
                    &Access::yes()
                )
                .unwrap(),
                &[&meta, &log]
            ),
            if offset < 768 {
                Err(HistoryFailure::StoredIntegrity)
            } else {
                Ok(JobResult::Counted { epoch: 8, count: 3 })
            },
            "offset {offset}"
        );
    }
}
#[test]
fn test_should_read_short_slots_and_partial_body_pages_with_small_program_units() {
    for w in [1, 16, 256] {
        let (s, meta, log) = seeded::<1>(w, &[42]);
        assert_eq!(
            run(
                &s,
                s.begin_prepare(
                    Preparation::Count { filter: filter(0) },
                    100,
                    &Access::yes()
                )
                .unwrap(),
                &[&meta, &log]
            ),
            Ok(JobResult::Counted { epoch: 8, count: 4 })
        );
    }
}

#[test]
fn test_should_preserve_patterned_maximum_body_across_page_boundaries() {
    let body: Vec<u8> = (0..474)
        .map(|i| u8::try_from((i * 73 + 11) % 251).unwrap())
        .collect();
    let (s, meta, log) = seeded::<474>(256, &body);
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
    let mut out = [0; 474];
    let record = adapter.read(lease, 1, &mut out, &access).unwrap();
    assert_eq!(record.length, 474);
    assert_eq!(out.as_slice(), body);
    for offset in [80, 255, 256, 511, 512, 553] {
        let (s, meta, log) = seeded::<474>(256, &body);
        log.bytes.borrow_mut()[offset] ^= 1;
        assert_eq!(
            run(
                &s,
                s.begin_prepare(Preparation::Count { filter: filter(0) }, 100, &access)
                    .unwrap(),
                &[&meta, &log]
            ),
            Err(HistoryFailure::StoredIntegrity),
            "body offset {offset}"
        );
    }
}
