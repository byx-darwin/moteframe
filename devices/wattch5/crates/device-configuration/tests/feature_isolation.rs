//! Selecting one health policy must not expose another product module's configuration.
#[test]
#[cfg(all(
    feature = "heart-rate",
    not(feature = "blood-pressure"),
    not(feature = "oxygen"),
    not(feature = "glucose"),
    not(feature = "temperature"),
    not(feature = "wellbeing")
))]
fn test_should_compile_only_the_selected_heart_rate_policy_routes() {
    assert!(device_configuration::property_descriptor(0x20, 1).is_ok());
    for module in [0x25, 0x26, 0x27, 0x28, 0x2a] {
        assert_eq!(
            device_configuration::property_descriptor(module, 1),
            Err(device_configuration::Error::Unknown)
        );
    }
}
