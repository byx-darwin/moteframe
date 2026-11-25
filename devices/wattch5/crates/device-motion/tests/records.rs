//! Literal schema vectors, independent of algorithms/Flash.
#[cfg(feature = "sleep")]
#[test]
fn test_should_keep_final_sleep_schema_separate_from_provisional_schema() {
    use device_motion::{SleepDetection, SleepInterval, SleepProvisional};
    let b = [0xc0, 0x27, 9, 0, 2, 100, 1];
    let r = SleepInterval::decode(&b).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(r.duration_ms, 600_000);
    let mut out = [0; 7];
    r.encode(&mut out).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(out, b);
    let mut bad = b;
    bad[4] = 6;
    assert!(SleepInterval::decode(&bad).is_err());
    let mut bad = b;
    bad[5] = 101;
    assert!(SleepInterval::decode(&bad).is_err());
    assert!(SleepProvisional::decode(&[0, 0, 0, 0, 2, 255, 1, 1]).is_ok());
    assert!(SleepProvisional::decode(&[0, 0, 0, 0, 2, 255, 1, 0]).is_err());
    let mut b = [0; 18];
    b[0] = 3;
    b[6] = 10;
    b[10] = 8;
    b[14] = 3;
    assert!(SleepDetection::decode(&b).is_err());
}
#[cfg(feature = "activity")]
#[test]
fn test_should_keep_absent_activity_values_distinct_from_measured_zero() {
    use device_motion::ActivityRecord;
    let mut b = [0; 31];
    b[0] = 1;
    assert!(ActivityRecord::decode(&b).is_ok());
    b[17] = 7;
    assert!(ActivityRecord::decode(&b).is_err());
    b[29] = 4;
    assert!(ActivityRecord::decode(&b).is_ok());
    b[30] = 128;
    assert!(ActivityRecord::decode(&b).is_err());
}
#[cfg(feature = "wear")]
#[test]
fn test_should_validate_wear_and_posture_intervals_without_inferring_them_from_sleep() {
    use device_motion::{PostureInterval, WearInterval};
    let mut b = [0; 19];
    b[16] = 1;
    b[17] = 4;
    b[18] = 1;
    assert!(WearInterval::decode(&b).is_ok());
    b[8] = 1;
    assert!(WearInterval::decode(&b).is_err());
    let mut b = [0; 19];
    b[16] = 255;
    b[17] = 4;
    assert!(PostureInterval::decode(&b).is_ok());
    b[16] = 2;
    assert!(PostureInterval::decode(&b).is_err());
}
#[cfg(feature = "sport")]
#[test]
fn test_should_validate_sport_masks_and_gps_without_truncating_session_ids() {
    use device_motion::{SportGpsSample, SportHeartSample, SportSummary};
    let mut b = [0; 46];
    b[0] = 1;
    b[4] = 1;
    b[28] = 1;
    b[31] = 1;
    let s = SportSummary::decode(&b).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(s.session, 0x1_0000_0001);
    b[32] = 1;
    assert!(SportSummary::decode(&b).is_err());
    b[44] = 8;
    assert!(SportSummary::decode(&b).is_ok());
    let mut b = [0; 31];
    b[0] = 1;
    b[25] = 1;
    assert!(SportGpsSample::decode(&b).is_ok());
    b[24] = 1;
    assert!(SportGpsSample::decode(&b).is_err());
    b[30] = 1;
    assert!(SportGpsSample::decode(&b).is_ok());
    let mut b = [0; 18];
    b[0] = 1;
    b[16] = 60;
    assert!(SportHeartSample::decode(&b).is_ok());
    assert!(SportHeartSample::decode(&b[..17]).is_err());
}
#[cfg(feature = "sport")]
#[test]
fn test_should_keep_respiration_in_hundredths_of_a_breath_per_minute() {
    let bytes = [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xdc, 5];
    assert_eq!(
        device_motion::SportRespirationSample::decode(&bytes)
            .unwrap_or_else(|e| panic!("{e:?}"))
            .respiration_x100,
        1500
    );
}
