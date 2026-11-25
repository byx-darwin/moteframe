//! New candidate content routes stay separate from the original catalogue.
#![cfg(feature = "notifications")]
use device_application::ContentSchemas;
use device_configuration::{Error, ProductCapabilities, SchemaProvider};
#[test]
fn test_should_route_app_switch_property_with_actual_module_limits() {
    let schemas = ContentSchemas::new(device_content::Manifest {
        string_max: Some(32),
        object_max: Some(128),
        property_max: Some(128),
        count_max: Some(8),
        ..device_content::Manifest::default()
    });
    let body = [1, 2, 0, b'q', b'q', 1];
    let descriptor = schemas
        .property_descriptor(0x30, 4)
        .unwrap_or_else(|| panic!("missing P4"));
    assert_eq!(descriptor.revision, 1);
    assert_eq!(descriptor.max_length, 128);
    assert!(
        device_configuration::validate_property_with_provider(
            0x30,
            4,
            &body,
            &ProductCapabilities::NONE,
            &schemas
        )
        .is_ok()
    );
    assert!(
        device_configuration::validate_property_value_with_provider(
            0x30,
            4,
            &body,
            &ProductCapabilities::NONE,
            &schemas
        )
        .is_ok()
    );
    let caps = ProductCapabilities {
        limits: Some(device_configuration::ModuleLimits {
            max_objects: 1,
            max_object_bytes: 5,
            max_string_bytes: 1,
        }),
        ..ProductCapabilities::NONE
    };
    assert_eq!(
        device_configuration::validate_property_with_provider(0x30, 4, &body, &caps, &schemas),
        Err(Error::Invalid)
    );
    assert!(!device_configuration::catalog().any(|e| e.id == "30.P0004"));
    assert!(device_configuration::candidate_catalog().any(|e| e.id == "30.P0004"));
}
#[test]
fn test_should_keep_app_property_capacity_separate_from_f000_object_capacity() {
    for objects in [None, Some(0)] {
        let schemas = ContentSchemas::new(device_content::Manifest {
            property_max: Some(6),
            object_max: objects,
            string_max: Some(2),
            count_max: Some(1),
            ..device_content::Manifest::default()
        });
        let caps = ProductCapabilities {
            limits: Some(device_configuration::ModuleLimits {
                max_objects: 0,
                max_object_bytes: 0,
                max_string_bytes: 2,
            }),
            ..ProductCapabilities::NONE
        };
        let body = [1, 2, 0, b'q', b'q', 1];
        assert_eq!(
            schemas.property_descriptor(0x30, 4).map(|d| d.max_length),
            Some(6)
        );
        assert!(
            device_configuration::validate_property_with_provider(0x30, 4, &body, &caps, &schemas)
                .is_ok()
        );
        assert!(
            device_configuration::validate_property_value_with_provider(
                0x30, 4, &body, &caps, &schemas
            )
            .is_ok()
        );
        let mut narrow = caps;
        narrow
            .limits
            .as_mut()
            .unwrap_or_else(|| panic!("limits"))
            .max_string_bytes = 1;
        assert!(
            device_configuration::validate_property_with_provider(
                0x30, 4, &body, &narrow, &schemas
            )
            .is_err()
        );
    }
}
