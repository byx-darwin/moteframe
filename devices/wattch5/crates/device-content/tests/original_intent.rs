//! Original protocol subfeatures that were lost by broad-row mapping.
#![cfg(any(feature = "phone", feature = "wallet", feature = "notifications"))]
use device_content::*;
fn manifest() -> Manifest<'static> {
    Manifest {
        string_max: Some(32),
        object_max: Some(512),
        property_max: Some(512),
        count_max: Some(8),
        ..Manifest::default()
    }
}
#[cfg(feature = "phone")]
#[test]
fn test_should_accept_system_camera_and_preserve_application_class() -> Result<(), Error> {
    let body = [2, 3, 1, 7];
    let value = decode_property(7, 2, &body, &manifest())?;
    let mut output = [0; 4];
    assert_eq!(encode_property(value, &mut output, &manifest())?, 4);
    assert_eq!(output, body);
    assert!(decode_property(7, 2, &[2, 4, 1, 7], &manifest()).is_err());
    assert!(decode_property(7, 2, &[2, 3, 1, 5], &manifest()).is_err());
    Ok(())
}
#[cfg(feature = "wallet")]
#[test]
fn test_should_distinguish_all_original_card_and_unionpay_providers() -> Result<(), Error> {
    for provider in 9..=17 {
        let body = [1, provider, 0, 1, 0, b'A', 1, 0, b'B'];
        let value = decode_object(0x35, 1, &body, &manifest())?;
        let mut output = [0; 9];
        assert_eq!(encode_object(value, &mut output, &manifest())?, 9);
        assert_eq!(output, body);
    }
    let unknown = [2, 18, 0, 0, 0, 0, 0];
    assert!(decode_object(0x35, 1, &unknown, &manifest()).is_err());
    assert!(decode_object_read(0x35, 1, &unknown, &manifest()).is_ok());
    Ok(())
}
#[cfg(feature = "notifications")]
#[test]
fn test_should_round_trip_independent_source_app_switches() -> Result<(), Error> {
    // QQ and WeChat may have different rules despite sharing category5 social.
    let body = [2, 2, 0, b'q', b'q', 1, 2, 0, b'w', b'x', 0];
    let value = decode_property(0x30, 4, &body, &manifest())?;
    let mut output = [0; 11];
    assert_eq!(encode_property(value, &mut output, &manifest())?, 11);
    assert_eq!(output, body);
    assert_eq!(descriptor(0x30, 4, false)?.revision, 1);
    Ok(())
}
#[cfg(feature = "notifications")]
#[test]
fn test_should_apply_app_overrides_then_independent_ios_policy() -> Result<(), Error> {
    let selected = manifest();
    let Property::CategorySwitches(categories) =
        decode_property(0x30, 1, &[1, 5, 0, 0], &selected)?
    else {
        return Err(Error::State);
    };
    let Property::AppSwitches(apps) = decode_property(
        0x30,
        4,
        &[2, 2, 0, b'q', b'q', 1, 2, 0, b'w', b'x', 0],
        &selected,
    )?
    else {
        return Err(Error::State);
    };
    let Property::AppIdentifiers(ios) =
        decode_property(0x30, 3, &[1, 0, 2, 0, b'q', b'q'], &selected)?
    else {
        return Err(Error::State);
    };
    assert!(notification_allowed(true, categories, apps, "qq", 5, None));
    assert!(!notification_allowed(true, categories, apps, "wx", 5, None));
    assert!(!notification_allowed(true, categories, apps, "QQ", 5, None));
    assert!(!notification_allowed(
        false, categories, apps, "qq", 5, None
    ));
    assert!(notification_allowed(
        true,
        categories,
        apps,
        "qq",
        5,
        Some(ios)
    ));
    assert!(!notification_allowed(
        true,
        categories,
        apps,
        "wx",
        5,
        Some(ios)
    ));
    let Property::CategorySwitches(allowed) = decode_property(0x30, 1, &[1, 5, 0, 1], &selected)?
    else {
        return Err(Error::State);
    };
    assert!(notification_allowed(true, allowed, apps, "other", 5, None));
    assert!(!notification_allowed(true, allowed, apps, "wx", 5, None));
    assert!(!notification_allowed(
        true,
        allowed,
        apps,
        "other",
        5,
        Some(ios)
    ));
    assert_eq!(apps.rule_for("qq"), Some(true));
    assert_eq!(apps.rule_for("wx"), Some(false));
    assert_eq!(apps.rule_for("other"), None);
    assert!(!format!("{apps:?}").contains("qq"));
    Ok(())
}
#[cfg(feature = "notifications")]
#[test]
fn test_should_reject_noncanonical_app_rules_and_enforce_actual_limits() -> Result<(), Error> {
    let selected = manifest();
    let invalid: &[&[u8]] = &[
        &[1, 0, 0, 1],                      // empty key
        &[1, 1, 0, 0, 1],                   // NUL key
        &[1, 1, 0, 0xff, 1],                // invalid UTF-8
        &[1, 1, 0, b'a', 2],                // nonboolean
        &[2, 1, 0, b'a', 1, 1, 0, b'a', 0], // duplicate
        &[2, 1, 0, b'b', 1, 1, 0, b'a', 0], // reversed bytes
        &[0, 0],                            // trailing byte
        &[1, 1, 0, b'a'],                   // truncated boolean
    ];
    for bytes in invalid {
        assert!(decode_property(0x30, 4, bytes, &selected).is_err());
    }
    let body = [1, 2, 0, b'q', b'q', 1];
    for missing in [
        Manifest {
            string_max: None,
            ..selected
        },
        Manifest {
            count_max: None,
            ..selected
        },
        Manifest {
            property_max: None,
            ..selected
        },
    ] {
        assert_eq!(
            decode_property(0x30, 4, &body, &missing),
            Err(Error::Unavailable)
        );
    }
    for bounded in [
        Manifest {
            string_max: Some(1),
            ..selected
        },
        Manifest {
            count_max: Some(0),
            ..selected
        },
        Manifest {
            property_max: Some(5),
            ..selected
        },
    ] {
        assert!(decode_property(0x30, 4, &body, &bounded).is_err());
    }
    let value = decode_property(0x30, 4, &body, &selected)?;
    assert_eq!(
        encode_property(value, &mut [0; 5], &selected),
        Err(Error::Capacity)
    );
    assert!(decode_property(0x30, 4, &[0], &selected).is_ok());
    // Ordinal UTF-8 bytes, not locale order or length-prefix order: "aa" precedes "b".
    assert!(decode_property(0x30, 4, &[2, 2, 0, b'a', b'a', 1, 1, 0, b'b', 0], &selected).is_ok());
    let mut large = vec![1, 254, 1]; //510-byte string plus3-byte length/count and enabled=514B
    large.extend(core::iter::repeat_n(b'x', 510));
    large.push(1);
    let wide = Manifest {
        string_max: Some(510),
        object_max: Some(1024),
        ..selected
    };
    assert!(decode_property(0x30, 4, &large, &wide).is_err());
    Ok(())
}
