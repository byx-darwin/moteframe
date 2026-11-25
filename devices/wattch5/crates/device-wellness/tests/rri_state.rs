#![cfg(feature = "rri")]
use device_wellness::{BeatClass, Error, RriEvent, RriObservation, RriTracker};
fn observation(index: u32, time: u64, interval: Option<u32>, class: BeatClass) -> RriObservation {
    RriObservation {
        event: RriEvent {
            measurement: 0x1_0000_0001,
            index,
            cumulative_us: time,
            interval_us: interval,
        },
        class,
    }
}
fn tracker() -> RriTracker {
    RriTracker::new(0x1_0000_0001, Some(0x2_0000_0001), 100).unwrap_or_else(|e| panic!("{e:?}"))
}
#[test]
fn test_should_preserve_u64_parent_and_require_actual_baseline_before_intervals() {
    let mut t = tracker();
    assert_eq!(t.parent_sport(), Some(0x2_0000_0001));
    let rejected = observation(0, 10, None, BeatClass::Rejected);
    assert_eq!(t.accept(rejected, 110), Ok(true));
    t.confirm(0x1_0000_0001, 0)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        t.accept(observation(1, 100, Some(90), BeatClass::Interval), 200),
        Err(Error::InvalidField)
    );
    let base = observation(1, 100, None, BeatClass::Baseline);
    t.accept(base, 200).unwrap_or_else(|e| panic!("{e:?}"));
    t.confirm(0x1_0000_0001, 1)
        .unwrap_or_else(|e| panic!("{e:?}"));
    t.accept(observation(2, 200, None, BeatClass::Rejected), 300)
        .unwrap_or_else(|e| panic!("{e:?}"));
    t.confirm(0x1_0000_0001, 2)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        t.accept(
            observation(3, 800_100, Some(800_000), BeatClass::Interval),
            800_200
        ),
        Ok(true)
    );
    assert_eq!(t.summary_at(800_200).map(|s| s.valid_count), Ok(1));
}
#[test]
fn test_should_deduplicate_retries_hold_pending_before_finish_and_reject_changed_or_wrong_identity()
{
    let mut t = tracker();
    let base = observation(0, 0, None, BeatClass::Baseline);
    assert_eq!(t.accept(base, 100), Ok(true));
    assert_eq!(t.accept(base, 100), Ok(false));
    assert_eq!(t.finish(200), Err(Error::Busy));
    assert_eq!(
        t.accept(observation(1, 1, None, BeatClass::Rejected), 101),
        Err(Error::Busy)
    );
    t.confirm(0x1_0000_0001, 0)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(t.confirm(0x1_0000_0001, 0), Ok(()));
    assert_eq!(
        t.accept(observation(0, 1, None, BeatClass::Baseline), 101),
        Err(Error::Sequence)
    );
    let mut wrong = observation(1, 100, Some(100), BeatClass::Interval);
    wrong.event.measurement = 1;
    assert_eq!(t.accept(wrong, 200), Err(Error::Sequence));
    assert_eq!(
        t.accept(observation(2, 100, Some(100), BeatClass::Interval), 200),
        Err(Error::Sequence)
    );
    assert_eq!(
        t.accept(observation(1, 100, Some(99), BeatClass::Interval), 200),
        Err(Error::InvalidField)
    );
    assert_eq!(
        t.accept(observation(1, 100, Some(100), BeatClass::Interval), 199),
        Err(Error::Time)
    );
}
#[test]
fn test_should_pause_without_samples_and_reset_interval_anchor_on_resume() {
    let mut t = tracker();
    t.accept(observation(0, 0, None, BeatClass::Baseline), 100)
        .unwrap_or_else(|e| panic!("{e:?}"));
    t.confirm(0x1_0000_0001, 0)
        .unwrap_or_else(|e| panic!("{e:?}"));
    t.pause(200).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        t.accept(observation(1, 200, Some(200), BeatClass::Interval), 300),
        Err(Error::InvalidState)
    );
    t.resume(1000).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        t.accept(observation(1, 1000, Some(1000), BeatClass::Interval), 1100),
        Err(Error::InvalidField)
    );
    t.accept(observation(1, 1000, None, BeatClass::Baseline), 1100)
        .unwrap_or_else(|e| panic!("{e:?}"));
    t.confirm(0x1_0000_0001, 1)
        .unwrap_or_else(|e| panic!("{e:?}"));
    let final_summary = t.finish(2100).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(final_summary.duration_ms, 2);
    assert_eq!(final_summary.valid_count, 0);
    assert_eq!(t.finish(3100), Ok(final_summary));
    assert_eq!(
        t.accept(observation(2, 2000, None, BeatClass::Rejected), 3100),
        Err(Error::InvalidState)
    );
}
#[test]
fn test_should_reject_invalid_identity_backwards_clock_and_duration_wrap() {
    assert!(matches!(
        RriTracker::new(0, None, 0),
        Err(Error::InvalidField)
    ));
    assert!(matches!(
        RriTracker::new(1, Some(0), 0),
        Err(Error::InvalidField)
    ));
    let mut t = tracker();
    assert_eq!(t.pause(99), Err(Error::Time));
    assert_eq!(
        t.finish((u64::from(u32::MAX) + 1) * 1000 + 100),
        Err(Error::Exhausted)
    );
}
#[test]
fn test_should_keep_finished_summary_stable_long_after_measurement_duration_limit() {
    let mut t = tracker();
    let summary = t.finish(1100).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(t.summary_at(u64::MAX), Ok(summary));
    assert_eq!(t.finish(u64::MAX), Ok(summary));
}
