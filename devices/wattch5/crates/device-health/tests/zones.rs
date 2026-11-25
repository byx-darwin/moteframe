#![cfg(feature = "heart-rate")]
use device_health::{Error, ZoneCounter, ZoneThresholds};
#[test]
fn test_should_require_positive_strictly_increasing_five_upper_bounds() {
    assert_eq!(
        ZoneThresholds::new([0, 90, 120, 150, 180]),
        Err(Error::InvalidField)
    );
    assert_eq!(
        ZoneThresholds::new([90, 90, 120, 150, 180]),
        Err(Error::InvalidField)
    );
    assert_eq!(
        ZoneThresholds::new([90, 120, 119, 150, 180]),
        Err(Error::InvalidField)
    );
    assert!(ZoneThresholds::new([1, 2, 3, 4, u16::MAX]).is_ok());
}
#[test]
fn test_should_count_each_actual_sample_once_using_inclusive_upper_bounds() {
    let thresholds =
        ZoneThresholds::new([90, 120, 150, 180, 220]).unwrap_or_else(|e| panic!("{e:?}"));
    let mut zones = ZoneCounter::new(12, thresholds).unwrap_or_else(|e| panic!("{e:?}"));
    for (index, value) in [1, 90, 91, 120, 121, 150, 151, 180, 181, 220]
        .into_iter()
        .enumerate()
    {
        let id = u64::try_from(index).unwrap_or_else(|e| panic!("{e:?}")) + 1;
        assert_eq!(zones.observe(id, Some(value)), Ok(true));
        assert_eq!(zones.observe(id, Some(value)), Ok(false));
    }
    assert_eq!(zones.counts(), [2, 2, 2, 2, 2]);
    assert_eq!(zones.context_id(), 12);
    assert_eq!(zones.observe(11, None), Ok(false));
    assert_eq!(zones.observe(12, Some(0)), Ok(false));
    assert_eq!(zones.observe(13, Some(221)), Ok(false));
    assert_eq!(zones.counts(), [2, 2, 2, 2, 2]);
    assert_eq!(zones.observe(12, Some(100)), Err(Error::InvalidState));
    assert_eq!(zones.observe(0, Some(100)), Err(Error::InvalidField));
    let mut out = [0; 35];
    assert_eq!(zones.encode_event(&mut out), Ok(35));
    assert_eq!(&out[..5], &[12, 0, 0, 0, 31]);
    assert_eq!(&out[5..11], &[90, 0, 2, 0, 0, 0]);
    assert_eq!(zones.encode_event(&mut [0; 34]), Err(Error::Capacity));
    let mut summary = [0; 31];
    assert_eq!(zones.encode_summary(&mut summary), Ok(31));
    assert_eq!(&summary[..], &out[4..]);
}
#[test]
fn test_should_not_accept_zero_context_or_reassign_existing_threshold_snapshot() {
    let thresholds =
        ZoneThresholds::new([90, 120, 150, 180, 220]).unwrap_or_else(|e| panic!("{e:?}"));
    assert!(matches!(
        ZoneCounter::new(0, thresholds),
        Err(Error::InvalidField)
    ));
    let mut zones = ZoneCounter::new(1, thresholds).unwrap_or_else(|e| panic!("{e:?}"));
    zones
        .observe(1, Some(100))
        .unwrap_or_else(|e| panic!("{e:?}"));
    let other = ZoneThresholds::new([50, 60, 70, 80, 90]).unwrap_or_else(|e| panic!("{e:?}"));
    let second = ZoneCounter::new(2, other).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(zones.thresholds(), [90, 120, 150, 180, 220]);
    assert_eq!(zones.counts(), [0, 1, 0, 0, 0]);
    assert_eq!(second.counts(), [0; 5]);
}
