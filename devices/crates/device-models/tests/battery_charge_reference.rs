//! Known charging cannot carry elapsed noncharging usage seconds.
#![cfg(feature = "device")]
use device_models::BatteryState;
#[test]
fn test_should_reject_nonzero_charge_usage_seconds_during_known_charging() {
    assert!(BatteryState::decode(&[80, 1, 0, 5, 0, 0, 0, 15]).is_err());
    for bytes in [[80, 1, 0, 0, 0, 0, 0, 15], [80, 0, 0, 5, 0, 0, 0, 15]] {
        let value = BatteryState::decode(&bytes).unwrap_or_else(|e| panic!("{e:?}"));
        let mut out = [0; 8];
        assert_eq!(value.encode(&mut out), Ok(8));
        assert_eq!(out, bytes);
    }
}

#[test]
fn test_should_not_claim_elapsed_charge_usage_with_unknown_charging_state() {
    assert!(BatteryState::decode(&[80, 0, 0, 5, 0, 0, 0, 9]).is_err());
    assert!(BatteryState::decode(&[80, 0, 0, 0, 0, 0, 0, 9]).is_err());
    assert!(BatteryState::decode(&[80, 0, 0, 0, 0, 0, 0, 1]).is_ok());
}
