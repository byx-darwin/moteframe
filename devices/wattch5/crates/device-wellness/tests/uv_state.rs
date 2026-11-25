#![cfg(feature = "uv")]
use device_wellness::{
    AlarmTransition, Error, Timestamp, UvEngine, UvPolicy, UvReading, UvSample, UvSource,
};
fn sample(id: u64, time: u64, index: Option<u16>) -> UvSample {
    UvSample {
        sample_id: id,
        monotonic_ms: time,
        timestamp: Timestamp::unknown(),
        source: UvSource::DeviceSampling,
        reading: UvReading {
            index_x100: index,
            intensity_uw_cm2: Some(1000),
        },
    }
}
fn policy(report: bool) -> UvPolicy {
    UvPolicy {
        report_enabled: report,
        alarm_enabled: true,
        threshold_x100: 700,
        interval_s: 2,
    }
}
#[test]
fn test_should_trigger_once_silence_cancel_cycle_and_rearm_only_on_real_clearance() {
    let mut e = UvEngine::<2>::new(policy(false), 1).unwrap_or_else(|x| panic!("{x:?}"));
    assert_eq!(e.latest(), Err(Error::NotFound));
    assert_eq!(e.ingest(sample(1, 0, Some(800))), Ok(true));
    let trigger = e.pending_alarm().unwrap_or_else(|| panic!("missing"));
    assert_eq!(
        (trigger.event_id, trigger.transition),
        (1, AlarmTransition::Triggered)
    );
    e.confirm_alarm(1, AlarmTransition::Triggered)
        .unwrap_or_else(|x| panic!("{x:?}"));
    assert_eq!(
        e.request_cancel(1, UvSource::AppRequest, Timestamp::unknown()),
        Ok(true)
    );
    assert_eq!(e.pending_alarm(), None);
    assert_eq!(
        e.request_cancel(1, UvSource::AppRequest, Timestamp::unknown()),
        Ok(false)
    );
    assert_eq!(
        e.confirm_alarm(1, AlarmTransition::AppCancel),
        Err(Error::InvalidState)
    );
    e.confirm_cancel_response(1)
        .unwrap_or_else(|x| panic!("{x:?}"));
    assert_eq!(
        e.pending_alarm().map(|v| v.transition),
        Some(AlarmTransition::AppCancel)
    );
    e.confirm_alarm(1, AlarmTransition::AppCancel)
        .unwrap_or_else(|x| panic!("{x:?}"));
    assert_eq!(e.pending_alarm(), None);
    e.ingest(sample(2, 1, Some(900)))
        .unwrap_or_else(|x| panic!("{x:?}"));
    assert_eq!(e.pending_alarm(), None);
    e.ingest(sample(3, 2, None))
        .unwrap_or_else(|x| panic!("{x:?}"));
    assert_eq!(e.pending_alarm(), None);
    e.ingest(sample(4, 3, Some(600)))
        .unwrap_or_else(|x| panic!("{x:?}"));
    assert_eq!(
        e.pending_alarm().map(|v| (v.event_id, v.transition)),
        Some((1, AlarmTransition::ConditionClear))
    );
    e.confirm_alarm(1, AlarmTransition::ConditionClear)
        .unwrap_or_else(|x| panic!("{x:?}"));
    e.ingest(sample(5, 4, Some(800)))
        .unwrap_or_else(|x| panic!("{x:?}"));
    assert_eq!(e.pending_alarm().map(|v| v.event_id), Some(2));
    assert_eq!(
        e.request_cancel(1, UvSource::AppRequest, Timestamp::unknown()),
        Ok(false)
    );
    assert_eq!(
        e.request_cancel(999, UvSource::AppRequest, Timestamp::unknown()),
        Err(Error::NotFound)
    );
}
#[test]
fn test_should_keep_events_immutable_on_backpressure_and_rate_reports_not_detection() {
    let mut e = UvEngine::<2>::new(policy(true), 1).unwrap_or_else(|x| panic!("{x:?}"));
    e.ingest(sample(1, 0, Some(800)))
        .unwrap_or_else(|x| panic!("{x:?}"));
    assert_eq!(e.pending_report().map(|r| r.event_id), Some(1));
    assert_eq!(e.pending_alarm().map(|r| r.event_id), Some(2));
    let report = e.pending_report();
    assert_eq!(e.ingest(sample(1, 0, Some(800))), Ok(false));
    assert_eq!(e.ingest(sample(2, 500, Some(900))), Err(Error::Busy));
    assert_eq!(e.pending_report(), report);
    e.confirm_report(1).unwrap_or_else(|x| panic!("{x:?}"));
    e.confirm_alarm(2, AlarmTransition::Triggered)
        .unwrap_or_else(|x| panic!("{x:?}"));
    e.ingest(sample(2, 500, Some(900)))
        .unwrap_or_else(|x| panic!("{x:?}"));
    assert_eq!(e.pending_report(), None);
    e.ingest(sample(3, 1000, Some(600)))
        .unwrap_or_else(|x| panic!("{x:?}"));
    assert_eq!(
        e.pending_alarm().map(|v| v.transition),
        Some(AlarmTransition::ConditionClear)
    );
    e.confirm_alarm(2, AlarmTransition::ConditionClear)
        .unwrap_or_else(|x| panic!("{x:?}"));
    e.ingest(sample(4, 2000, Some(800)))
        .unwrap_or_else(|x| panic!("{x:?}"));
    assert_eq!(e.pending_report().map(|v| v.event_id), Some(3));
    assert_eq!(e.pending_alarm().map(|v| v.event_id), Some(4));
}
#[test]
fn test_should_reject_missing_actual_data_replay_changes_time_reverse_and_id_exhaustion() {
    let mut e = UvEngine::<2>::new(
        UvPolicy {
            alarm_enabled: false,
            ..policy(false)
        },
        1,
    )
    .unwrap_or_else(|x| panic!("{x:?}"));
    let mut missing = sample(1, 0, None);
    missing.reading.intensity_uw_cm2 = None;
    assert_eq!(e.ingest(missing), Err(Error::NotFound));
    assert_eq!(e.latest(), Err(Error::NotFound));
    e.ingest(sample(1, 100, Some(0)))
        .unwrap_or_else(|x| panic!("{x:?}"));
    assert_eq!(e.latest().map(|v| v.reading.index_x100), Ok(Some(0)));
    assert_eq!(e.ingest(sample(1, 100, Some(1))), Err(Error::Sequence));
    assert_eq!(e.ingest(sample(2, 99, Some(1))), Err(Error::Time));
    let mut e = UvEngine::<2>::new(policy(true), u32::MAX).unwrap_or_else(|x| panic!("{x:?}"));
    assert_eq!(e.ingest(sample(1, 0, Some(800))), Err(Error::Exhausted));
    assert_eq!(e.latest(), Err(Error::NotFound));
}
#[test]
fn test_should_preserve_known_cancelled_ids_and_reject_full_retention_without_fake_success() {
    let mut e = UvEngine::<1>::new(policy(false), 1).unwrap_or_else(|x| panic!("{x:?}"));
    e.ingest(sample(1, 0, Some(800)))
        .unwrap_or_else(|x| panic!("{x:?}"));
    e.confirm_alarm(1, AlarmTransition::Triggered)
        .unwrap_or_else(|x| panic!("{x:?}"));
    e.request_cancel(1, UvSource::DeviceUser, Timestamp::unknown())
        .unwrap_or_else(|x| panic!("{x:?}"));
    e.confirm_cancel_response(1)
        .unwrap_or_else(|x| panic!("{x:?}"));
    e.confirm_alarm(1, AlarmTransition::DeviceCancel)
        .unwrap_or_else(|x| panic!("{x:?}"));
    e.ingest(sample(2, 1, Some(600)))
        .unwrap_or_else(|x| panic!("{x:?}"));
    e.confirm_alarm(1, AlarmTransition::ConditionClear)
        .unwrap_or_else(|x| panic!("{x:?}"));
    assert_eq!(e.ingest(sample(3, 2, Some(800))), Err(Error::Capacity));
    assert_eq!(
        e.request_cancel(1, UvSource::DeviceUser, Timestamp::unknown()),
        Ok(false)
    );
}
