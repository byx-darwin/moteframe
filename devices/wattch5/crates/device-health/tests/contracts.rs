use device_health::{Error, HealthStart, Latest, Module, Timestamp};

fn enabled_modules() -> Vec<(u8, u16, u8)> {
    let mut out = Vec::new();
    for (enabled, module, record, mode) in [
        (cfg!(feature = "heart-rate"), 0x20, 0x0020, 0),
        (cfg!(feature = "blood-pressure"), 0x25, 0x2501, 0),
        (cfg!(feature = "oxygen"), 0x26, 0x2601, 0),
        (cfg!(feature = "glucose"), 0x27, 0x2701, 0),
        (cfg!(feature = "temperature"), 0x28, 0x2801, 3),
        (cfg!(feature = "ecg"), 0x29, 0x2901, 0),
        (cfg!(feature = "wellbeing"), 0x2a, 0x2a04, 0),
        (cfg!(feature = "microcirculation"), 0x2b, 0x2b01, 0),
    ] {
        assert_eq!(Module::from_wire(module).is_ok(), enabled);
        if enabled {
            out.push((module, record, mode));
        }
    }
    out
}
#[test]
fn test_should_compile_only_selected_modules_and_validate_exact_start() {
    assert_eq!(Module::from_wire(0x21), Err(Error::Unsupported));
    for (module, record, temp) in enabled_modules() {
        let [lo, hi] = record.to_le_bytes();
        let bytes = [1, lo, hi, 0, 0, 0, 0, temp];
        let request = HealthStart::decode(module, &bytes).unwrap_or_else(|e| panic!("{e:?}"));
        let mut output = [0; 8];
        assert_eq!(request.encode(&mut output), Ok(8));
        assert_eq!(output, bytes);
        assert_eq!(
            HealthStart::decode(module, &bytes[..7]),
            Err(Error::InvalidLength)
        );
        assert_eq!(
            HealthStart::decode(module, &[0; 9]),
            Err(Error::InvalidLength)
        );
        let mut bad = bytes;
        bad[0] = 0;
        assert_eq!(HealthStart::decode(module, &bad), Err(Error::InvalidField));
        let mut bad = bytes;
        bad[1] ^= 1;
        if module != 0x2a {
            assert_eq!(HealthStart::decode(module, &bad), Err(Error::InvalidField));
        }
        let mut bad = bytes;
        bad[7] = u8::from(module != 0x28);
        assert_eq!(HealthStart::decode(module, &bad), Err(Error::InvalidField));
        if module != 0x20 {
            let mut bad = bytes;
            bad[3] = 1;
            assert_eq!(HealthStart::decode(module, &bad), Err(Error::InvalidField));
        }
    }
}
#[test]
fn test_should_reject_unknown_time_and_flag_encodings() {
    assert_eq!(Timestamp::new(1, false), Err(Error::InvalidField));
    assert_eq!(Timestamp::new(0, true).map(Timestamp::utc_ms), Ok(0));
    for (module, record, _) in enabled_modules() {
        let [lo, hi] = record.to_le_bytes();
        let mut body = [0; 18];
        body[0] = lo;
        body[1] = hi;
        body[2] = 1;
        body[13] = 3;
        assert!(Latest::decode(module, &body).is_ok());
        body[12] = 2;
        assert_eq!(Latest::decode(module, &body), Err(Error::InvalidField));
        body[12] = 0;
        body[4] = 1;
        assert_eq!(Latest::decode(module, &body), Err(Error::InvalidField));
        body[4] = 0;
        body[13] = 2;
        assert_eq!(Latest::decode(module, &body), Err(Error::InvalidLength));
    }
}
#[test]
fn test_should_encode_latest_without_status_and_hide_payload() {
    for (module, record, _) in enabled_modules() {
        let value = Latest::new(module, record, 1, Timestamp::unknown(), b"secret")
            .unwrap_or_else(|e| panic!("{e:?}"));
        let mut output = [0; 21];
        assert_eq!(value.encode(&mut output), Ok(21));
        assert_eq!(&output[15..], b"secret");
        assert_eq!(Latest::decode(module, &output), Ok(value));
        assert!(!format!("{value:?}").contains("secret"));
        assert!(!format!("{value:?}").contains("115, 101"));
        assert_eq!(value.encode(&mut [0; 20]), Err(Error::Capacity));
    }
}
#[test]
fn test_should_validate_exact_stop_query_job_parameter_without_zero_or_trailing_bytes() {
    use device_health::{JobId, ResultSource};
    assert_eq!(JobId::decode(&[0, 0, 0, 0]), Err(Error::InvalidField));
    assert_eq!(JobId::decode(&[1, 0, 0]), Err(Error::InvalidLength));
    assert_eq!(JobId::decode(&[1, 0, 0, 0, 0]), Err(Error::InvalidLength));
    let id = JobId::decode(&[0x78, 0x56, 0x34, 0x12]).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(id.get(), 0x1234_5678);
    let mut out = [0; 4];
    assert_eq!(id.encode(&mut out), Ok(4));
    assert_eq!(out, [0x78, 0x56, 0x34, 0x12]);
    assert_eq!(id.encode(&mut [0; 3]), Err(Error::Capacity));
    assert_eq!(ResultSource::from_wire(0), Err(Error::InvalidField));
    assert_eq!(ResultSource::from_wire(3), Err(Error::InvalidField));
    assert_eq!(
        ResultSource::from_wire(1),
        Ok(ResultSource::DeviceAlgorithm)
    );
    assert_eq!(ResultSource::from_wire(2), Ok(ResultSource::PhoneImport));
}
#[test]
fn test_should_enforce_each_actual_carrier_limit_without_truncating_body() {
    use device_health::ResultCarrier;
    for (carrier, body_max) in [
        (ResultCarrier::EventLatest, 497),
        (ResultCarrier::ResponseLatest, 493),
        (ResultCarrier::HistoryRecord, 474),
    ] {
        assert_eq!(carrier.checked_length(body_max, 512), Ok(512));
        assert_eq!(
            carrier.checked_length(body_max + 1, 512),
            Err(Error::Capacity)
        );
    }
    for (carrier, body_max) in [
        (ResultCarrier::EventLatest, 113),
        (ResultCarrier::ResponseLatest, 109),
        (ResultCarrier::HistoryRecord, 90),
    ] {
        assert_eq!(carrier.checked_length(body_max, 128), Ok(128));
        assert_eq!(
            carrier.checked_length(body_max + 1, 128),
            Err(Error::Capacity)
        );
    }
    assert_eq!(
        ResultCarrier::EventLatest.checked_length(0, 0),
        Err(Error::Unsupported)
    );
    assert_eq!(
        ResultCarrier::EventLatest.checked_length(0, 127),
        Err(Error::InvalidField)
    );
    assert_eq!(
        ResultCarrier::EventLatest.checked_length(0, 513),
        Err(Error::InvalidField)
    );
    assert_eq!(
        ResultCarrier::EventLatest.checked_length(usize::MAX, 512),
        Err(Error::InvalidLength)
    );
}
