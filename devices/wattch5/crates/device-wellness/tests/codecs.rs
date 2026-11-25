#[test]
fn test_should_select_only_explicit_features_and_validate_timestamp() {
    use device_wellness::{Error, Timestamp, module_selected};
    assert_eq!(module_selected(0x2c), cfg!(feature = "rri"));
    assert_eq!(module_selected(0x2d), cfg!(feature = "womens-health"));
    assert_eq!(module_selected(0x2e), cfg!(feature = "uv"));
    assert!(!module_selected(0x20));
    assert_eq!(Timestamp::new(1, false), Err(Error::InvalidField));
    assert_eq!(Timestamp::new(0, true).map(Timestamp::is_valid), Ok(true));
}
#[cfg(feature = "rri")]
#[test]
fn test_should_encode_exact_rri_summary_and_relative_event_without_latest_wrapper() {
    use device_wellness::{Error, RriEvent, RriSummary};
    let brief = RriSummary {
        measurement: 0x1_0000_0001,
        duration_ms: 1000,
        valid_count: 1,
    };
    let summary = [1, 0, 0, 0, 1, 0, 0, 0, 0xe8, 3, 0, 0, 1, 0, 0, 0];
    assert_eq!(RriSummary::decode(&summary), Ok(brief));
    let mut out = [0; 16];
    assert_eq!(brief.encode(&mut out), Ok(16));
    assert_eq!(out, summary);
    let event = RriEvent {
        measurement: 0x1_0000_0001,
        index: 1,
        cumulative_us: 800_000,
        interval_us: Some(800_000),
    };
    let literal = [
        1, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0, 0, 0x35, 0x0c, 0, 0, 0, 0, 0, 0, 0x35, 0x0c, 0, 1,
    ];
    assert_eq!(RriEvent::decode(&literal), Ok(event));
    let mut out = [0; 25];
    assert_eq!(event.encode(&mut out), Ok(25));
    assert_eq!(out, literal);
    let mut bad = literal;
    bad[24] = 2;
    assert_eq!(RriEvent::decode(&bad), Err(Error::InvalidField));
    let mut bad = literal;
    bad[24] = 0;
    assert_eq!(RriEvent::decode(&bad), Err(Error::InvalidField));
    let mut bad = literal;
    bad[20..24].fill(0);
    assert_eq!(RriEvent::decode(&bad), Err(Error::InvalidField));
    assert_eq!(RriEvent::decode(&[0; 24]), Err(Error::InvalidLength));
    assert_eq!(RriEvent::decode(&[0; 26]), Err(Error::InvalidLength));
    assert_eq!(RriSummary::decode(&[0; 16]), Err(Error::InvalidField));
    assert_eq!(RriSummary::decode(&[0; 17]), Err(Error::InvalidLength));
    assert!(!format!("{event:?}").contains("800000"));
}
#[cfg(feature = "womens-health")]
#[test]
fn test_should_validate_gregorian_civil_days_and_config_mode_exclusion() {
    use device_wellness::{DayNumber, Error, WomensConfig, WomensMode};
    assert_eq!(DayNumber::from_date(1970, 1, 1).map(DayNumber::get), Ok(0));
    assert_eq!(
        DayNumber::from_date(1969, 12, 31).map(DayNumber::get),
        Ok(-1)
    );
    assert_eq!(
        DayNumber::from_date(2024, 2, 29).map(DayNumber::get),
        Ok(19782)
    );
    assert!(DayNumber::from_date(2000, 2, 29).is_ok());
    assert_eq!(DayNumber::from_date(1900, 2, 29), Err(Error::InvalidField));
    assert_eq!(DayNumber::from_date(2100, 2, 29), Err(Error::InvalidField));
    assert_eq!(DayNumber::from_date(2024, 4, 31), Err(Error::InvalidField));
    assert_eq!(DayNumber::new(19782).to_date(), Ok((2024, 2, 29)));
    for day in [i32::MIN, -1, 0, 19782, i32::MAX] {
        let date = DayNumber::new(day)
            .to_date()
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(
            DayNumber::from_date(date.0, date.1, date.2).map(DayNumber::get),
            Ok(day)
        );
    }
    let config = WomensConfig {
        enabled: true,
        mode: WomensMode::Cycle {
            anchor: DayNumber::new(19782),
            duration_days: 5,
            cycle_days: 28,
        },
    };
    let literal = [1, 1, 0x46, 0x4d, 0, 0, 5, 0, 28, 0];
    assert_eq!(WomensConfig::decode(&literal), Ok(config));
    let mut out = [0; 10];
    assert_eq!(config.encode(&mut out), Ok(10));
    assert_eq!(out, literal);
    let pregnancy = [1, 2, 0x46, 0x4d, 0, 0, 0, 0, 0, 0];
    assert_eq!(
        WomensConfig::decode(&pregnancy),
        Ok(WomensConfig {
            enabled: true,
            mode: WomensMode::Pregnancy {
                due: DayNumber::new(19782)
            }
        })
    );
    for (index, value) in [(0, 2), (1, 0), (6, 29)] {
        let mut bad = literal;
        bad[index] = value;
        assert_eq!(WomensConfig::decode(&bad), Err(Error::InvalidField));
    }
    let mut bad = pregnancy;
    bad[6] = 1;
    assert_eq!(WomensConfig::decode(&bad), Err(Error::InvalidField));
    let mut bad = literal;
    bad[0] = 0;
    bad[6] = 0;
    assert_eq!(WomensConfig::decode(&bad), Err(Error::InvalidField));
    assert_eq!(WomensConfig::decode(&[0; 9]), Err(Error::InvalidLength));
    assert_eq!(WomensConfig::decode(&[0; 11]), Err(Error::InvalidLength));
    assert!(!format!("{config:?}").contains("19782"));
}
#[cfg(feature = "uv")]
#[test]
fn test_should_encode_uv_reading_policy_and_explicit_time_source_events() {
    use device_wellness::{
        AlarmTransition, Error, Timestamp, UvAlarmEvent, UvPolicy, UvReading, UvReportEvent,
        UvSource,
    };
    let reading = UvReading {
        index_x100: Some(750),
        intensity_uw_cm2: Some(1000),
    };
    let literal = [0xee, 2, 0xe8, 3, 0, 0, 3];
    assert_eq!(UvReading::decode(&literal), Ok(reading));
    let mut out = [0; 7];
    assert_eq!(reading.encode(&mut out), Ok(7));
    assert_eq!(out, literal);
    for bad in [[0xee, 2, 0xe8, 3, 0, 0, 4], [0xee, 2, 0xe8, 3, 0, 0, 1]] {
        assert_eq!(UvReading::decode(&bad), Err(Error::InvalidField));
    }
    assert_eq!(UvReading::decode(&[0; 6]), Err(Error::InvalidLength));
    assert_eq!(UvReading::decode(&[0; 8]), Err(Error::InvalidLength));
    let policy = UvPolicy {
        report_enabled: true,
        alarm_enabled: true,
        threshold_x100: 700,
        interval_s: 60,
    };
    let literal = [1, 1, 0xbc, 2, 60, 0, 0, 0];
    assert_eq!(UvPolicy::decode(&literal), Ok(policy));
    let mut out = [0; 8];
    assert_eq!(policy.encode(&mut out), Ok(8));
    assert_eq!(out, literal);
    assert_eq!(
        UvPolicy::decode(&[1, 1, 0, 0, 0, 0, 0, 0]),
        Err(Error::InvalidField)
    );
    let event = UvReportEvent {
        event_id: 1,
        source: UvSource::DeviceSampling,
        timestamp: Timestamp::unknown(),
        reading,
    };
    let literal = [
        1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xee, 2, 0xe8, 3, 0, 0, 3,
    ];
    assert_eq!(UvReportEvent::decode(&literal), Ok(event));
    let mut out = [0; 21];
    assert_eq!(event.encode(&mut out), Ok(21));
    assert_eq!(out, literal);
    let alarm = UvAlarmEvent {
        event_id: 1,
        source: UvSource::AppRequest,
        transition: AlarmTransition::AppCancel,
        timestamp: Timestamp::unknown(),
        reading,
    };
    let literal = [
        1, 0, 0, 0, 3, 3, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xee, 2, 0xe8, 3, 0, 0, 3,
    ];
    assert_eq!(UvAlarmEvent::decode(&literal), Ok(alarm));
    let mut out = [0; 22];
    assert_eq!(alarm.encode(&mut out), Ok(22));
    assert_eq!(out, literal);
    let mut bad = literal;
    bad[5] = 5;
    assert_eq!(UvAlarmEvent::decode(&bad), Err(Error::InvalidField));
    let mut bad = literal;
    bad[14] = 2;
    assert_eq!(UvAlarmEvent::decode(&bad), Err(Error::InvalidField));
    let mut bad = literal;
    bad[6] = 1;
    assert_eq!(UvAlarmEvent::decode(&bad), Err(Error::InvalidField));
    let mut bad = literal;
    bad[0] = 0;
    assert_eq!(UvAlarmEvent::decode(&bad), Err(Error::InvalidField));
    assert_eq!(UvAlarmEvent::decode(&[0; 23]), Err(Error::InvalidLength));
    assert_eq!(UvReportEvent::decode(&[0; 20]), Err(Error::InvalidLength));
    assert!(!format!("{alarm:?}").contains("750"));
}
#[cfg(feature = "uv")]
#[test]
fn test_should_decode_exact_nonzero_cancel_id_without_inventing_event_or_success_body() {
    use device_wellness::{Error, UvCancelRequest};
    let request = UvCancelRequest {
        event_id: 0x1234_5678,
    };
    assert_eq!(
        UvCancelRequest::decode(&[0x78, 0x56, 0x34, 0x12]),
        Ok(request)
    );
    let mut out = [0; 4];
    assert_eq!(request.encode(&mut out), Ok(4));
    assert_eq!(out, [0x78, 0x56, 0x34, 0x12]);
    assert_eq!(UvCancelRequest::decode(&[0; 4]), Err(Error::InvalidField));
    assert_eq!(UvCancelRequest::decode(&[1; 3]), Err(Error::InvalidLength));
    assert_eq!(UvCancelRequest::decode(&[1; 5]), Err(Error::InvalidLength));
}
