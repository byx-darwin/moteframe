//! V0.6 candidate field and canonical-value tests; no algorithms or hardware.
#[cfg(feature = "profile")]
#[test]
fn test_should_preserve_valid_profile_and_reject_unknown_masks_or_false_missing_values() {
    use device_models::{ActivityGoals, UserProfile};
    let b = [0x70, 0x11, 1, 0, 0xd6, 6, 1, 35, 0xbc, 2, 0x84, 3, 0x3f, 0];
    let p = UserProfile::decode(&b).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(p.weight_g, 70000);
    assert_eq!(p.height_mm, 1750);
    let mut out = [0; 14];
    assert_eq!(p.encode(&mut out), Ok(14));
    assert_eq!(out, b);
    let mut bad = b;
    bad[12] = 0x80;
    assert!(UserProfile::decode(&bad).is_err());
    let mut bad = b;
    bad[12] &= !1;
    assert!(UserProfile::decode(&bad).is_err());
    let mut bad = b;
    bad[6] = 4;
    assert!(UserProfile::decode(&bad).is_err());
    assert!(!format!("{p:?}").contains("70000"));
    let b = [16, 39, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0];
    assert!(ActivityGoals::decode(&b).is_ok());
    let mut bad = b;
    bad[17] = 1;
    assert!(ActivityGoals::decode(&bad).is_err());
}
#[cfg(feature = "settings")]
#[test]
fn test_should_validate_unit_enums_and_minute_windows() {
    use device_models::{HourFormat, TimeWindow, Units};
    assert!(Units::decode(&[0, 1]).is_ok());
    assert!(Units::decode(&[0, 2]).is_err());
    assert!(HourFormat::decode(&[1]).is_ok());
    assert!(HourFormat::decode(&[2]).is_err());
    assert!(TimeWindow::decode(&[1, 1, 0x28, 5, 0x2c, 1]).is_ok()); //22:00 ->05:00
    assert!(TimeWindow::decode(&[1, 128, 0, 0, 0, 0]).is_err());
    assert!(TimeWindow::decode(&[1, 0, 0, 0, 0, 0]).is_ok());
    assert!(TimeWindow::decode(&[0, 1, 0, 0, 0, 0]).is_ok());
}
#[cfg(feature = "device")]
#[test]
fn test_should_distinguish_unknown_battery_from_a_real_zero_level() {
    use device_models::BatteryState;
    assert!(BatteryState::decode(&[0; 8]).is_ok());
    assert!(BatteryState::decode(&[0, 0, 0, 0, 0, 0, 0, 1]).is_ok());
    assert!(BatteryState::decode(&[101, 0, 0, 0, 0, 0, 0, 1]).is_err());
    assert!(BatteryState::decode(&[55, 0, 0, 0, 0, 0, 0, 0]).is_err());
}
#[cfg(feature = "profile")]
#[test]
fn test_should_reject_sex_enum_not_defined_in_candidate_contract() {
    assert!(
        device_models::UserProfile::decode(&[0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 0, 0, 4, 0]).is_err()
    );
}
#[cfg(feature = "settings")]
#[test]
fn test_should_preserve_valid_window_configuration_when_temporarily_disabled() {
    assert!(device_models::TimeWindow::decode(&[0, 0x7f, 0x28, 5, 0xa4, 1]).is_ok());
    assert!(device_models::TimeWindow::decode(&[1, 0, 0, 0, 0, 0]).is_ok()); // Empty weekday set is a wire-valid empty window.
}
