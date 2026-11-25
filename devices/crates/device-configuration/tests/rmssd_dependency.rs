#![cfg(feature = "heart-rate")]
//! The replacement HR policy must preserve its own RMSSD dependency.
use device_configuration::{Error, HealthCapability, ProductCapabilities, Range};

#[test]
fn test_should_reject_rmssd_when_incoming_policy_disables_heart_rate() {
    let health = [HealthCapability {
        module: 0x20,
        interval_s: Range { min: 1, max: 600 },
        temperature_modes: 0,
    }];
    // The previous HR policy is enabled and has a selected RMSSD backend.
    let caps = ProductCapabilities {
        health: Some(&health),
        rmssd_allowed: true,
        ..ProductCapabilities::NONE
    };
    assert_eq!(
        device_configuration::validate_property(0x20, 1, &[0, 1, 30, 0, 0, 0, 1], &caps),
        Err(Error::Invalid)
    );
    for bytes in [[1, 1, 30, 0, 0, 0, 1], [0, 1, 30, 0, 0, 0, 0]] {
        assert!(device_configuration::validate_property(0x20, 1, &bytes, &caps).is_ok());
    }
}
