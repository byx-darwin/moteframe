//! A property-only product must not need object storage to configure app preferences.
#![cfg(feature = "notifications")]
use device_content::*;
#[test]
fn test_should_bound_app_rules_with_property_capacity_independently_of_object_storage() {
    let body = [1, 2, 0, b'q', b'q', 1];
    for objects in [None, Some(0)] {
        let manifest = Manifest {
            property_max: Some(6),
            object_max: objects,
            string_max: Some(2),
            count_max: Some(1),
            ..Manifest::default()
        };
        let value = decode_property(0x30, 4, &body, &manifest)
            .unwrap_or_else(|e| panic!("property-only product rejected: {e:?}"));
        let mut out = [0; 6];
        assert_eq!(encode_property(value, &mut out, &manifest), Ok(6));
        assert_eq!(out, body);
        let too_small = Manifest {
            property_max: Some(5),
            ..manifest
        };
        assert!(decode_property(0x30, 4, &body, &too_small).is_err());
        assert!(encode_property(value, &mut out, &too_small).is_err());
    }
}
#[test]
fn test_should_require_actual_p4_property_capacity_without_changing_old_properties() {
    let absent = Manifest {
        object_max: Some(1024),
        string_max: Some(32),
        count_max: Some(8),
        ..Manifest::default()
    };
    assert_eq!(
        decode_property(0x30, 4, &[0], &absent),
        Err(Error::Unavailable)
    );
    assert!(decode_property(0x30, 2, &[1], &absent).is_ok());
    assert_eq!(
        decode_property(
            0x30,
            4,
            &[0],
            &Manifest {
                property_max: Some(0),
                ..absent
            }
        ),
        Err(Error::Unavailable)
    );
    for capacity in [0, 513, u16::MAX] {
        let unsupported = Manifest {
            property_max: Some(capacity),
            ..absent
        };
        assert!(decode_property(0x30, 4, &[0], &unsupported).is_err());
    }
}
