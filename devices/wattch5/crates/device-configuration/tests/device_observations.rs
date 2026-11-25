#![cfg(feature = "device")]
use device_configuration::{ProductCapabilities, validate_property_value};
#[test]
fn test_should_reject_charge_elapsed_seconds_while_charging() {
    assert!(
        validate_property_value(
            2,
            1,
            &[80, 1, 0, 5, 0, 0, 0, 15],
            &ProductCapabilities::NONE
        )
        .is_err()
    );
}
#[test]
fn test_should_adopt_exact_reset_dictionary_and_require_unknown_validity_clear() {
    for reason in 1u16..=6 {
        let mut bytes = [0; 15];
        bytes[..2].copy_from_slice(&reason.to_le_bytes());
        bytes[14] = 1;
        assert!(validate_property_value(2, 6, &bytes, &ProductCapabilities::NONE).is_ok());
    }
    for reason in [0u16, 7, 0xffff] {
        let mut bytes = [0; 15];
        bytes[..2].copy_from_slice(&reason.to_le_bytes());
        bytes[14] = 1;
        assert!(validate_property_value(2, 6, &bytes, &ProductCapabilities::NONE).is_err());
    }
    assert!(validate_property_value(2, 6, &[0; 15], &ProductCapabilities::NONE).is_ok());
}

#[test]
fn test_should_require_known_charging_when_elapsed_usage_is_valid() {
    for elapsed in [0, 5] {
        let bytes = [80, 0, 0, elapsed, 0, 0, 0, 9];
        assert!(validate_property_value(2, 1, &bytes, &ProductCapabilities::NONE).is_err());
    }
}
