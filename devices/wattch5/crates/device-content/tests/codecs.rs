//! Exact content layouts, manifest boundaries, canonical invalid data and object round trips.
#![cfg(any(
    feature = "phone",
    feature = "companion-radio",
    feature = "weather",
    feature = "contacts",
    feature = "memo",
    feature = "find",
    feature = "notifications",
    feature = "wallet",
    feature = "watchface"
))]
use device_content::*;
fn manifest() -> Manifest<'static> {
    Manifest {
        string_max: Some(32),
        object_max: Some(1024),
        property_max: Some(512),
        count_max: Some(8),
        heartbeat_timeout_s: Some(Range { min: 1, max: 120 }),
        heartbeat_grace_s: Some(Range { min: 0, max: 20 }),
        find_delay_s: Some(Range { min: 0, max: 60 }),
        independent_call_bluetooth: true,
        utc_ms: Some(TimeRange {
            min: 0,
            max: 4_102_444_800_000,
        }),
        watchface: Some(WatchfaceManifest {
            revision: 1,
            width: 240,
            height: 240,
            dials: &[1, 2],
            groups: &[1],
            elements: &[1, 2],
            styles: &[0, 1],
            verified_resources: false,
        }),
    }
}
#[cfg(feature = "phone")]
#[test]
fn test_should_round_trip_all_phone_properties_and_reject_malformed() -> Result<(), Error> {
    let selected = manifest();
    let mut output = [0; 128];
    let fixtures: [(u16, &[u8]); 5] = [
        (1, &[1, 0, 0, 0, 0, 0, 0]),
        (2, &[2, 1, 1, 7]),
        (3, &[0, 0, 0, 0, 0, 0, 0, 0, 1]),
        (4, &[1, 0, b'A']),
        (5, &[1, 60, 0, 10, 0]),
    ];
    for (key, bytes) in fixtures {
        let value = decode_property(7, key, bytes, &selected)?;
        let length = encode_property(value, &mut output, &selected)?;
        assert_eq!(&output[..length], bytes);
        for end in 0..bytes.len() {
            assert!(decode_property(7, key, &bytes[..end], &selected).is_err());
        }
        let mut extra = bytes.to_vec();
        extra.push(0);
        assert!(decode_property(7, key, &extra, &selected).is_err());
    }
    assert_eq!(validate_phone_property(1, &[0; 7]), Err(Error::Invalid));
    assert_eq!(
        decode_property(7, 2, &[2, 0, 0, 0], &selected),
        Err(Error::Invalid)
    );
    assert_eq!(
        decode_property(7, 3, &[1, 0, 0, 0, 0, 0, 0, 0, 0], &selected),
        Err(Error::Invalid)
    );
    assert_eq!(
        decode_property(7, 4, &[1, 0, 0], &selected),
        Err(Error::Invalid)
    );
    assert_eq!(
        decode_property(7, 5, &[1, 60, 0, 10, 0], &Manifest::default()),
        Err(Error::Unavailable)
    );
    Ok(())
}
#[cfg(feature = "find")]
#[test]
fn test_should_require_selected_find_delay_range() -> Result<(), Error> {
    let value = decode_property(6, 1, &[1, 10, 0], &manifest())?;
    let mut out = [0; 3];
    assert_eq!(encode_property(value, &mut out, &manifest())?, 3);
    assert_eq!(out, [1, 10, 0]);
    assert_eq!(
        decode_property(6, 1, &[1, 61, 0], &manifest()),
        Err(Error::Invalid)
    );
    assert_eq!(
        decode_property(6, 1, &[1, 10, 0], &Manifest::default()),
        Err(Error::Unavailable)
    );
    Ok(())
}
#[cfg(feature = "notifications")]
#[test]
fn test_should_validate_unique_categories_and_case_sensitive_app_ids() -> Result<(), Error> {
    let selected = manifest();
    let Property::CategorySwitches(value) =
        decode_property(0x30, 1, &[2, 1, 0, 1, 3, 0, 0], &selected)?
    else {
        return Err(Error::State);
    };
    assert!(value.allows(1));
    assert!(!value.allows(3));
    assert!(!value.allows(2));
    assert_eq!(
        decode_property(0x30, 1, &[2, 1, 0, 1, 1, 0, 0], &selected),
        Err(Error::Invalid)
    );
    let raw = [2, 0, 1, 0, b'A', 1, 0, b'a'];
    let Property::AppIdentifiers(ids) = decode_property(0x30, 3, &raw, &selected)? else {
        return Err(Error::State);
    };
    assert!(ids.contains("A"));
    assert!(ids.contains("a"));
    assert!(!ids.contains("B"));
    assert_eq!(ids.iter().collect::<Vec<_>>(), vec!["A", "a"]);
    let mut out = [0; 32];
    assert_eq!(
        encode_property(Property::AppIdentifiers(ids), &mut out, &selected)?,
        8
    );
    assert_eq!(&out[..8], &raw);
    assert_eq!(
        decode_property(0x30, 3, &[1, 0, 0, 0], &selected),
        Err(Error::Invalid)
    );
    assert_eq!(
        decode_property(0x30, 3, &[2, 0, 1, 0, b'A', 1, 0, b'A'], &selected),
        Err(Error::Invalid)
    );
    Ok(())
}
#[cfg(feature = "notifications")]
#[test]
fn test_should_reuse_shared_notification_schema_and_hide_contents_in_debug() -> Result<(), Error> {
    let value = device_models::Notification {
        notification: 7,
        category: 1,
        source_app: "secret-app",
        sender: "secret-sender",
        number: "secret-number",
        title: "secret-title",
        body: "secret-body",
        action_mask: 7,
        created_utc_ms: 0,
        created_time_valid: false,
    };
    let mut out = [0; 256];
    let length = encode_object(Object::Notification(value), &mut out, &manifest())?;
    assert_eq!(
        decode_object(0x30, 1, &out[..length], &manifest())?,
        Object::Notification(value)
    );
    let debug = format!("{:?}", Object::Notification(value));
    assert!(!debug.contains("secret"));
    Ok(())
}
#[cfg(feature = "wallet")]
#[test]
fn test_should_preserve_future_wallet_provider_on_read_and_reject_unknown_write()
-> Result<(), Error> {
    let selected = manifest();
    let bytes = [1, 18, 0, 1, 0, b'A', 1, 0, b'B'];
    assert_eq!(
        decode_object(0x35, 1, &bytes, &selected),
        Err(Error::Invalid)
    );
    let value = decode_object_read(0x35, 1, &bytes, &selected)?;
    let mut out = [0; 32];
    assert_eq!(encode_object(value, &mut out, &selected)?, bytes.len());
    assert_eq!(&out[..bytes.len()], &bytes);
    assert_eq!(
        decode_object(0x35, 1, &[1, 0, 0, 1, 0, 0, 0, 0], &selected),
        Err(Error::Invalid)
    );
    Ok(())
}
#[cfg(feature = "watchface")]
#[test]
fn test_should_validate_watchface_canvas_manifest_and_actual_resource_readiness()
-> Result<(), Error> {
    let selected = manifest();
    let bytes = [
        1, 0, 0, 0, 1, 0, 1, 1, 0, b'v', 1, 1, 0, 1, 20, 0, 30, 0, 255, 255, 0, 0,
    ];
    let value = decode_object(0x31, 1, &bytes, &selected)?;
    let Object::Watchface(face) = value else {
        return Err(Error::State);
    };
    assert_eq!(face.element(0)?.x, 20);
    assert_eq!(face.element(1), Err(Error::NotFound));
    let mut out = [0; 64];
    assert_eq!(encode_object(value, &mut out, &selected)?, bytes.len());
    assert_eq!(&out[..bytes.len()], &bytes);
    let mut invalid = bytes;
    invalid[6] = 2;
    assert_eq!(
        decode_object(0x31, 1, &invalid, &selected),
        Err(Error::Unavailable)
    );
    invalid = bytes;
    invalid[14] = 240;
    assert_eq!(
        decode_object(0x31, 1, &invalid, &selected),
        Err(Error::Invalid)
    );
    assert_eq!(
        decode_property(0x31, 1, &[9, 0, 0, 0], &selected),
        Err(Error::Invalid)
    );
    Ok(())
}
#[cfg(feature = "contacts")]
#[test]
fn test_should_cover_frozen_contact_route() -> Result<(), Error> {
    let selected = manifest();
    let contact = [1, 0, b'A', 3, 0, b'1', b'2', b'3'];
    let value = decode_object(0x34, 1, &contact, &selected)?;
    let mut output = [0; 128];
    assert_eq!(encode_object(value, &mut output, &selected)?, contact.len());
    assert_eq!(&output[..contact.len()], &contact);
    Ok(())
}
#[cfg(feature = "memo")]
#[test]
fn test_should_cover_frozen_memo_route_and_require_calendar_range() -> Result<(), Error> {
    let selected = manifest();
    let mut output = [0; 128];
    let mut memo = vec![0; 16];
    memo[8] = 1;
    memo.extend_from_slice(&[1, 0, b'A']);
    let value = decode_object(0x36, 1, &memo, &selected)?;
    assert_eq!(encode_object(value, &mut output, &selected)?, memo.len());
    assert_eq!(&output[..memo.len()], &memo);
    let missing = Manifest {
        utc_ms: None,
        ..selected
    };
    assert_eq!(
        decode_object(0x36, 1, &memo, &missing),
        Err(Error::Unavailable)
    );
    memo[0] = 2;
    assert_eq!(
        decode_object(0x36, 1, &memo, &selected),
        Err(Error::Invalid)
    );
    Ok(())
}
#[cfg(feature = "companion-radio")]
#[test]
fn test_should_require_independent_classic_backend_and_decode_actual_state() -> Result<(), Error> {
    let selected = manifest();
    let bytes = [1, 4, 1, 2, 3, 4, 5, 6, 1];
    let value = decode_property(8, 1, &bytes, &selected)?;
    let mut out = [0; 9];
    assert_eq!(encode_property(value, &mut out, &selected)?, 9);
    assert_eq!(out, bytes);
    let missing = Manifest {
        independent_call_bluetooth: false,
        ..selected
    };
    assert_eq!(
        decode_property(8, 1, &bytes, &missing),
        Err(Error::Unavailable)
    );
    let mut invalid = bytes;
    invalid[8] = 0;
    assert_eq!(
        decode_property(8, 1, &invalid, &selected),
        Err(Error::Invalid)
    );
    invalid = bytes;
    invalid[1] = 7;
    assert_eq!(
        decode_property(8, 1, &invalid, &selected),
        Err(Error::Invalid)
    );
    Ok(())
}
#[cfg(feature = "weather")]
#[test]
fn test_should_reuse_strict_weather_dictionary_and_not_invent_freshness() -> Result<(), Error> {
    let selected = manifest();
    let mut bytes = [0; 25];
    bytes[10] = 1;
    bytes[15] = 1;
    let value = decode_object(0x32, 1, &bytes, &selected)?;
    let Object::Weather(weather) = value else {
        return Err(Error::State);
    };
    assert_eq!(weather.day_count(), 1);
    assert_eq!(weather.day(0).map_err(|_| Error::Invalid)?.valid_mask, 0);
    let mut out = [0; 64];
    assert_eq!(encode_object(value, &mut out, &selected)?, 25);
    assert_eq!(&out[..25], &bytes);
    let mut invalid = bytes;
    invalid[16] = 6;
    assert_eq!(
        decode_object(0x32, 1, &invalid, &selected),
        Err(Error::Invalid)
    );
    invalid = bytes;
    invalid[18] = 1;
    assert_eq!(
        decode_object(0x32, 1, &invalid, &selected),
        Err(Error::Invalid)
    );
    Ok(())
}
#[cfg(all(
    feature = "phone",
    feature = "companion-radio",
    feature = "notifications",
    feature = "watchface",
    feature = "weather",
    feature = "contacts",
    feature = "wallet",
    feature = "memo"
))]
#[test]
fn test_should_cover_all_sixteen_external_catalog_routes() -> Result<(), Error> {
    let properties = [
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
    ];
    let objects = [0x30, 0x31, 0x32, 0x34, 0x35, 0x36];
    for (module, key) in properties {
        let schema = descriptor(module, key, false)?;
        assert_eq!(schema.module, module);
        assert!(!schema.object);
    }
    for module in objects {
        assert!(descriptor(module, 1, true)?.object);
    }
    Ok(())
}
