#![cfg(feature = "blood-pressure")]
//! A BP module version is an actual read-only provider value, with no firmware fallback.
use device_configuration::{Access, Error, ModuleLimits, ProductCapabilities};

#[test]
fn test_should_validate_actual_bp_version_with_product_string_limit() {
    let caps = ProductCapabilities {
        limits: Some(ModuleLimits {
            max_objects: 1,
            max_object_bytes: 64,
            max_string_bytes: 4,
        }),
        ..ProductCapabilities::NONE
    };
    let descriptor =
        device_configuration::validate_property_value(0x25, 0xf0, &[3, 0, b'B', b'P', b'1'], &caps)
            .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(descriptor.access, Access::ReadOnly);
    assert_eq!(descriptor.revision, 1);
    assert_eq!(
        device_configuration::validate_property(0x25, 0xf0, &[1, 0, b'v'], &caps),
        Err(Error::Readonly)
    );
    for bytes in [
        &[0, 0][..],
        &[1, 0, 0],
        &[1, 0, 0xff],
        &[5, 0, b'1', b'2', b'3', b'4', b'5'],
    ] {
        assert!(device_configuration::validate_property_value(0x25, 0xf0, bytes, &caps).is_err());
    }
    assert!(
        device_configuration::validate_property_value(
            0x25,
            0xf0,
            &[1, 0, b'v'],
            &ProductCapabilities::NONE
        )
        .is_err()
    );
}

#[test]
fn test_should_encode_only_the_typed_actual_bp_version() {
    use device_configuration::{Value, decode_property, encode_property};
    let caps = ProductCapabilities {
        limits: Some(ModuleLimits {
            max_objects: 1,
            max_object_bytes: 64,
            max_string_bytes: 4,
        }),
        ..ProductCapabilities::NONE
    };
    let bytes = [3, 0, b'B', b'P', b'1'];
    let value = decode_property(0x25, 0xf0, &bytes, &caps).unwrap_or_else(|e| panic!("{e:?}"));
    assert!(matches!(value, Value::BloodPressureVersion("BP1")));
    let mut out = [0; 5];
    assert_eq!(encode_property(0x25, 0xf0, value, &mut out, &caps), Ok(5));
    assert_eq!(out, bytes);
    assert!(encode_property(0x25, 0xf0, Value::Text("BP1"), &mut out, &caps).is_err());
}
