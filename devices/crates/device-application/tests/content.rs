//! Every external property/object route is backed by an actual typed content validator.
#![cfg(all(
    feature = "phone",
    feature = "companion-radio",
    feature = "notifications",
    feature = "watchface",
    feature = "weather",
    feature = "contacts",
    feature = "wallet",
    feature = "memo"
))]
use device_application::ContentSchemas;
use device_configuration::{Access, Error, ProductCapabilities, SchemaProvider};
#[test]
fn test_should_connect_all_sixteen_external_schema_routes_and_keep_classic_status_readonly() {
    let schemas = ContentSchemas::new(device_content::Manifest {
        object_max: Some(512),
        ..device_content::Manifest::default()
    });
    for (module, key) in [
        (7, 1),
        (7, 2),
        (7, 3),
        (7, 4),
        (7, 5),
        (8, 1),
        (0x30, 1),
        (0x30, 2),
        (0x30, 3),
        (0x31, 1),
    ] {
        assert!(
            schemas.property_descriptor(module, key).is_some(),
            "{module:X}.{key}"
        );
    }
    for module in [0x30, 0x31, 0x32, 0x34, 0x35, 0x36] {
        assert!(schemas.object_descriptor(module, 1).is_some());
    }
    assert_eq!(
        schemas.property_descriptor(8, 1).map(|d| d.access),
        Some(Access::ReadOnly)
    );
    assert_eq!(
        device_configuration::validate_property_with_provider(
            8,
            1,
            &[255],
            &ProductCapabilities::NONE,
            &schemas
        ),
        Err(Error::Readonly)
    );
    assert!(schemas.property_descriptor(0x40, 1).is_none());
}
#[test]
fn test_should_validate_phone_fields_and_apply_actual_module_string_limit() {
    let schemas = ContentSchemas::new(device_content::Manifest {
        string_max: Some(20),
        object_max: Some(512),
        count_max: Some(10),
        ..device_content::Manifest::default()
    });
    // Phone status has a strict unlocked boolean and validity mask.
    assert!(
        schemas
            .validate_property_value(7, 2, &[1, 1, 1, 7], &ProductCapabilities::NONE)
            .is_ok()
    );
    assert!(
        schemas
            .validate_property_value(7, 2, &[1, 1, 2, 7], &ProductCapabilities::NONE)
            .is_err()
    );
    // Frozen contact two strings. The module's actual F000 string ceiling is stricter than global text limits.
    let value = [3, 0, b'a', b'b', b'c', 1, 0, b'1'];
    assert!(
        schemas
            .validate_object_value(0x34, 1, &value, &ProductCapabilities::NONE)
            .is_ok()
    );
    let caps = ProductCapabilities {
        limits: Some(device_configuration::ModuleLimits {
            max_objects: 1,
            max_object_bytes: 512,
            max_string_bytes: 2,
        }),
        ..ProductCapabilities::NONE
    };
    assert!(
        schemas
            .validate_object_value(0x34, 1, &value, &caps)
            .is_err()
    );
}
#[test]
fn test_should_preserve_unknown_wallet_provider_on_read_but_reject_it_on_new_write() {
    let schemas = ContentSchemas::new(device_content::Manifest {
        string_max: Some(20),
        object_max: Some(512),
        ..device_content::Manifest::default()
    });
    let body = [1, 99, 0, 0, 0, 0, 0];
    assert!(
        device_configuration::validate_object_value_with_provider(
            0x35,
            1,
            &body,
            &ProductCapabilities::NONE,
            &schemas
        )
        .is_ok()
    );
    assert!(
        device_configuration::validate_object_with_provider(
            0x35,
            1,
            &body,
            &ProductCapabilities::NONE,
            &schemas
        )
        .is_err()
    );
}
