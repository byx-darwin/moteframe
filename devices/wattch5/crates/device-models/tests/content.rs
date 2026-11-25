//! Optional phone-provided content schemas. No displayed-data claim or hardware action.
#[cfg(feature = "notifications")]
#[test]
fn test_should_roundtrip_notification_and_reject_unavailable_call_actions() {
    use device_models::Notification;
    let mut b = vec![1, 0, 0, 0, 1, 0];
    for text in ["app", "caller", "123", "title", "body"] {
        b.extend_from_slice(
            &u16::try_from(text.len())
                .unwrap_or_else(|e| panic!("{e:?}"))
                .to_le_bytes(),
        );
        b.extend_from_slice(text.as_bytes());
    }
    b.extend_from_slice(&[7, 0]);
    b.extend_from_slice(&[0; 9]);
    let n = Notification::decode(&b, 64).unwrap_or_else(|e| panic!("{e:?}"));
    let mut out = [0; 128];
    let len = n.encode(&mut out, 64).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(&out[..len], b);
    assert!(!format!("{n:?}").contains("caller"));
    let mut bad = b.clone();
    bad[4] = 3;
    assert!(Notification::decode(&bad, 64).is_err());
    let mut bad = b.clone();
    let index = bad.len() - 9;
    bad[index] = 1;
    assert!(Notification::decode(&bad, 64).is_err());
    assert!(Notification::decode(&b, 2).is_err());
}
#[cfg(feature = "weather")]
#[test]
fn test_should_validate_weather_dictionary_dates_and_absent_temperatures() {
    use device_models::Weather;
    let mut b = vec![0; 11];
    b[10] = 1;
    b.extend_from_slice(&[1, 0, 0, 0, 1, 2, 0, 0xc4, 9, 0xd0, 7, 0xdc, 5, 7]);
    let w = Weather::decode(&b, 7).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        w.day(0).unwrap_or_else(|e| panic!("{e:?}")).high_centi_c,
        2500
    );
    let mut out = [0; 64];
    let n = w.encode(&mut out, 7).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(&out[..n], b);
    let mut bad = b.clone();
    bad[16] = 6;
    assert!(Weather::decode(&bad, 7).is_err());
    let mut bad = b.clone();
    bad[24] = 0;
    assert!(Weather::decode(&bad, 7).is_err());
    let mut bad = b.clone();
    bad[10] = 2;
    bad.extend_from_slice(&b[11..]);
    assert!(Weather::decode(&bad, 7).is_err());
    assert!(Weather::decode(&b, 0).is_err());
}
