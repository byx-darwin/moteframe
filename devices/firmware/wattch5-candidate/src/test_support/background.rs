//! Production paged history under an explicit one-millisecond scheduling model.
#![allow(clippy::unwrap_used, reason = "Test fixture assertions.")]
use crate::{
    background::advance_storage,
    history_support::{Clock, Control, Mem},
    schema::StrictSchema,
};
use core::task::Poll;
use device_history_service::{HistoryAccess, HistoryOperation, Preparation};
use device_history_store::{
    AppendRecord, DomainConfig,
    async_paged::{AsyncPagedHistoryStore, JobResult, JobToken},
    paged::LogConfig,
};
use device_protocol::HistoryFilter;
use device_storage::{incremental::IncrementalSnapshot, persistence::Layout};
type Store = AsyncPagedHistoryStore<Mem, Mem, StrictSchema, Clock, 1, 255, 474, 144>;
struct Access;
impl HistoryAccess for Access {
    fn module_allowed(&self) -> bool {
        true
    }
    fn allowed(&self, _: [u8; 16], _: Option<u8>, _: HistoryOperation) -> bool {
        true
    }
}
fn settle(s: &Store, token: JobToken<'_>) -> JobResult {
    for _ in 0..100_000 {
        if let Poll::Ready(result) = s.result(token).unwrap() {
            return result.unwrap();
        }
        s.step().unwrap();
    }
    panic!("fixture stalled")
}
#[test]
fn full_quota_count_keeps_property_slices_and_finishes_in_bounded_schedule_model() {
    let meta = Control::new(8192, 256, 4096);
    let log = Control::new(0x41000, 256, 4096);
    let s = Store::new_unrecovered(
        IncrementalSnapshot::new_unrecovered(
            meta.medium(),
            Layout {
                start: 0,
                bank_bytes: 4096,
            },
        )
        .unwrap(),
        log.medium(),
        StrictSchema,
        Clock::new(),
        LogConfig {
            namespace: [9; 16],
            log_start: 0x202000,
            slots_per_domain: 259,
        },
        300,
    )
    .unwrap();
    assert_eq!(
        settle(&s, s.begin_recover().unwrap()),
        JobResult::Recovered { initialized: false }
    );
    settle(
        &s,
        s.begin_initialize(
            [3; 16],
            &[DomainConfig {
                domain: 1,
                epoch: 1,
            }],
        )
        .unwrap(),
    );
    let body = [1, 0, 0, 0, 1, 80, 0];
    for id in 1..=255 {
        assert_eq!(
            settle(
                &s,
                s.begin_append(AppendRecord {
                    domain: 1,
                    record_type: 0x21,
                    schema: 1,
                    utc_ms: 0,
                    flags: 0,
                    parent_session: 0,
                    payload: &body
                })
                .unwrap()
            ),
            JobResult::Appended { id }
        );
    }
    let request = Preparation::Count {
        filter: HistoryFilter {
            domain: 1,
            filter_flags: 0,
            start_utc_ms: 0,
            end_utc_ms: 0,
            record_type: 0,
            parent_session: 0,
        },
    };
    let token = s.begin_prepare(request, 0, &Access).unwrap();
    let mut old_ticks = 0;
    loop {
        if let Poll::Ready(result) = s.result(token).unwrap() {
            assert_eq!(
                result.unwrap(),
                JobResult::Counted {
                    epoch: 1,
                    count: 255
                }
            );
            break;
        }
        old_ticks += 1;
        if old_ticks % 2 == 0 {
            s.step().unwrap();
        }
        assert!(old_ticks < 100_000);
    }
    // Contiguous scanning now needs fewer phases; this is a work-allocation
    // comparison, not a reproduction of the old hardware ten-second failure.
    let token = s.begin_prepare(request, 0, &Access).unwrap();
    let mut properties = 0;
    let mut ticks = 0;
    loop {
        if let Poll::Ready(result) = s.result(token).unwrap() {
            assert_eq!(
                result.unwrap(),
                JobResult::Counted {
                    epoch: 1,
                    count: 255
                }
            );
            break;
        }
        ticks += 1;
        advance_storage(
            || s.step(),
            || {
                properties += 1;
                Ok(())
            },
        )
        .unwrap();
        assert!(
            ticks < 10_000,
            "bounded work must finish before the model response budget"
        );
    }
    assert_eq!(
        properties, ticks,
        "settings work remains serviced on every pass"
    );
    assert!(ticks * 8 >= old_ticks && ticks * 8 < old_ticks + 8);
}
#[test]
fn accepted_property_work_is_not_starved_by_history_failure() {
    let mut properties = 0;
    assert_eq!(
        advance_storage(
            || Err(7),
            || {
                properties += 1;
                Ok(())
            }
        ),
        Err(7)
    );
    assert_eq!(properties, 1);
}
