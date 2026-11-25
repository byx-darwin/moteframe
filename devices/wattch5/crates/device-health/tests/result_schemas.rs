//! Independent literal wire vectors, not generated from the implementation's schema table.
#![allow(clippy::similar_names)]
#[cfg(feature = "heart-rate")]
#[test]
fn test_should_preserve_frozen_four_byte_hr_and_reject_bad_validity_quality_or_tail() {
    use device_health::{Error, HeartRate};
    let value = HeartRate {
        bpm: Some(60),
        quality: 100,
    };
    assert_eq!(HeartRate::decode(&[60, 0, 100, 1]), Ok(value));
    let mut out = [0; 4];
    assert_eq!(value.encode(&mut out), Ok(4));
    assert_eq!(out, [60, 0, 100, 1]);
    assert_eq!(
        HeartRate::decode(&[0, 0, 255, 0]),
        Ok(HeartRate {
            bpm: None,
            quality: 255
        })
    );
    for bad in [[60, 0, 100, 2], [60, 0, 100, 0], [60, 0, 101, 1]] {
        assert_eq!(HeartRate::decode(&bad), Err(Error::InvalidField));
    }
    assert_eq!(HeartRate::decode(&[60, 0, 100]), Err(Error::InvalidLength));
    assert_eq!(
        HeartRate::decode(&[60, 0, 100, 1, 2]),
        Err(Error::InvalidLength)
    );
    assert_eq!(value.encode(&mut [0; 3]), Err(Error::Capacity));
    assert!(!format!("{value:?}").contains("60"));
}
#[cfg(feature = "blood-pressure")]
#[test]
fn test_should_decode_bp_partial_fields_and_reject_reserved_or_noncanonical_mask() {
    use device_health::{BloodPressure, Error};
    let value = BloodPressure {
        systolic: Some(120),
        diastolic: Some(80),
        pulse: Some(60),
    };
    let literal = [120, 0, 80, 0, 60, 0, 7, 0];
    assert_eq!(BloodPressure::decode(&literal), Ok(value));
    let mut out = [0; 8];
    assert_eq!(value.encode(&mut out), Ok(8));
    assert_eq!(out, literal);
    assert_eq!(
        BloodPressure::decode(&[120, 0, 0, 0, 0, 0, 1, 0]),
        Ok(BloodPressure {
            systolic: Some(120),
            diastolic: None,
            pulse: None
        })
    );
    for bad in [[120, 0, 80, 0, 60, 0, 8, 0], [120, 0, 80, 0, 0, 0, 1, 0]] {
        assert_eq!(BloodPressure::decode(&bad), Err(Error::InvalidField));
    }
    assert_eq!(BloodPressure::decode(&[0; 7]), Err(Error::InvalidLength));
    assert_eq!(BloodPressure::decode(&[0; 9]), Err(Error::InvalidLength));
}
#[cfg(feature = "oxygen")]
#[test]
fn test_should_decode_percent_times100_and_reject_overflow_or_missing_pi() {
    use device_health::{Error, Oxygen};
    let value = Oxygen {
        spo2_x100: Some(9812),
        pi_x100: Some(123),
    };
    assert_eq!(Oxygen::decode(&[0x54, 0x26, 123, 0, 3, 0]), Ok(value));
    let mut out = [0; 6];
    assert_eq!(value.encode(&mut out), Ok(6));
    assert_eq!(out, [0x54, 0x26, 123, 0, 3, 0]);
    assert_eq!(
        Oxygen::decode(&[0x10, 0x27, 0, 0, 1, 0]),
        Ok(Oxygen {
            spo2_x100: Some(10000),
            pi_x100: None
        })
    );
    for bad in [
        [0x11, 0x27, 0, 0, 1, 0],
        [0x54, 0x26, 1, 0, 1, 0],
        [0, 0, 0, 0, 4, 0],
    ] {
        assert_eq!(Oxygen::decode(&bad), Err(Error::InvalidField));
    }
    assert_eq!(Oxygen::decode(&[0; 5]), Err(Error::InvalidLength));
    assert_eq!(Oxygen::decode(&[0; 7]), Err(Error::InvalidLength));
}
#[cfg(feature = "glucose")]
#[test]
fn test_should_decode_glucose_without_creating_value_for_invalid_result() {
    use device_health::{Error, Glucose};
    let value = Glucose {
        mmol_l_x100: Some(680),
    };
    assert_eq!(Glucose::decode(&[0xa8, 2, 1]), Ok(value));
    let mut out = [0; 3];
    assert_eq!(value.encode(&mut out), Ok(3));
    assert_eq!(out, [0xa8, 2, 1]);
    assert_eq!(
        Glucose::decode(&[0, 0, 0]),
        Ok(Glucose { mmol_l_x100: None })
    );
    for bad in [[0xa8, 2, 0], [0, 0, 2]] {
        assert_eq!(Glucose::decode(&bad), Err(Error::InvalidField));
    }
    assert_eq!(Glucose::decode(&[0; 2]), Err(Error::InvalidLength));
    assert_eq!(Glucose::decode(&[0; 4]), Err(Error::InvalidLength));
}
#[cfg(feature = "temperature")]
#[test]
fn test_should_preserve_four_byte_signed_temperature_with_explicit_mode() {
    use device_health::{Error, Temperature, TemperatureMode};
    let value = Temperature {
        centi_c: Some(3650),
        mode: TemperatureMode::Body,
    };
    assert_eq!(Temperature::decode(&[0x42, 0x0e, 3, 1]), Ok(value));
    let mut out = [0; 4];
    assert_eq!(value.encode(&mut out), Ok(4));
    assert_eq!(out, [0x42, 0x0e, 3, 1]);
    assert_eq!(
        Temperature::decode(&[0x0c, 0xfe, 1, 1]),
        Ok(Temperature {
            centi_c: Some(-500),
            mode: TemperatureMode::Object
        })
    );
    assert_eq!(
        Temperature::decode(&[0, 0, 3, 0]),
        Ok(Temperature {
            centi_c: None,
            mode: TemperatureMode::Body
        })
    );
    for bad in [[0, 0, 0, 1], [0, 0, 5, 1], [1, 0, 3, 0], [0, 0, 3, 2]] {
        assert_eq!(Temperature::decode(&bad), Err(Error::InvalidField));
    }
    assert_eq!(Temperature::decode(&[0; 3]), Err(Error::InvalidLength));
    assert_eq!(Temperature::decode(&[0; 5]), Err(Error::InvalidLength));
    assert!(!format!("{value:?}").contains("3650"));
}
#[cfg(feature = "ecg")]
#[test]
fn test_should_decode_ecg_window_statistics_and_reject_bad_identity_mask_and_order() {
    use device_health::{Ecg, Error};
    let value = Ecg {
        measurement: 1,
        user_slot: 0,
        ecg_bpm: Some(60),
        ppg_bpm: Some(61),
        avg_bpm: Some(62),
        min_bpm: Some(55),
        max_bpm: Some(80),
    };
    let literal = [
        1, 0, 0, 0, 0, 0, 0, 0, 0, 60, 0, 61, 0, 62, 0, 55, 0, 80, 0, 31, 0,
    ];
    assert_eq!(Ecg::decode(&literal), Ok(value));
    let mut out = [0; 21];
    assert_eq!(value.encode(&mut out), Ok(21));
    assert_eq!(out, literal);
    for index in [0, 8, 13, 19] {
        let mut bad = literal;
        match index {
            0 => bad[0] = 0,
            8 => bad[8] = 255,
            13 => bad[13] = 90,
            _ => bad[19] = 32,
        }
        assert_eq!(Ecg::decode(&bad), Err(Error::InvalidField));
    }
    let mut absent = literal;
    absent[19] = 30;
    assert_eq!(Ecg::decode(&absent), Err(Error::InvalidField));
    assert_eq!(Ecg::decode(&[0; 20]), Err(Error::InvalidLength));
    assert_eq!(Ecg::decode(&[0; 22]), Err(Error::InvalidLength));
}
#[cfg(feature = "wellbeing")]
#[test]
fn test_should_decode_independent_wellbeing_units_sources_and_scale_exponents() {
    use device_health::{Error, ResultSource, Wellbeing, WellbeingKind};
    let cases = [
        (
            0x2a01,
            [4, 0, 0, 0, 0, 2, 1, 1],
            Wellbeing {
                kind: WellbeingKind::Fatigue,
                value: Some(4),
                scale_exp: 0,
                source: Some(ResultSource::DeviceAlgorithm),
            },
        ),
        (
            0x2a02,
            [100, 0, 0, 0, 0, 1, 2, 1],
            Wellbeing {
                kind: WellbeingKind::Stress,
                value: Some(100),
                scale_exp: 0,
                source: Some(ResultSource::PhoneImport),
            },
        ),
        (
            0x2a03,
            [0xfd, 0xff, 0xff, 0xff, 0, 3, 2, 1],
            Wellbeing {
                kind: WellbeingKind::Mood,
                value: Some(-3),
                scale_exp: 0,
                source: Some(ResultSource::PhoneImport),
            },
        ),
        (
            0x2a04,
            [0x50, 0xc3, 0, 0, 0xfd, 4, 1, 1],
            Wellbeing {
                kind: WellbeingKind::Rmssd,
                value: Some(50000),
                scale_exp: -3,
                source: Some(ResultSource::DeviceAlgorithm),
            },
        ),
        (
            0x2a05,
            [150, 0, 0, 0, 0xfe, 5, 1, 1],
            Wellbeing {
                kind: WellbeingKind::Met,
                value: Some(150),
                scale_exp: -2,
                source: Some(ResultSource::DeviceAlgorithm),
            },
        ),
    ];
    for (kind, literal, value) in cases {
        assert_eq!(Wellbeing::decode(kind, &literal), Ok(value));
        let mut out = [0; 8];
        assert_eq!(value.encode(&mut out), Ok(8));
        assert_eq!(out, literal);
    }
    for (kind, bad) in [
        (0x2a01, [5, 0, 0, 0, 0, 2, 1, 1]),
        (0x2a02, [101, 0, 0, 0, 0, 1, 1, 1]),
        (0x2a01, [1, 0, 0, 0, 0, 1, 1, 1]),
        (0x2a01, [1, 0, 0, 0, 0, 2, 0, 1]),
        (0x2a01, [1, 0, 0, 0, 0, 2, 1, 0]),
        (0x2a04, [1, 0, 0, 0, 1, 4, 1, 1]),
        (0x2a05, [1, 0, 0, 0, 0, 5, 1, 1]),
        (0x2a01, [0, 0, 0, 0, 0, 2, 3, 0]),
    ] {
        assert_eq!(Wellbeing::decode(kind, &bad), Err(Error::InvalidField));
    }
    assert_eq!(
        Wellbeing::decode(0x2a01, &[0, 0, 0, 0, 0, 2, 0, 0]),
        Ok(Wellbeing {
            kind: WellbeingKind::Fatigue,
            value: None,
            scale_exp: 0,
            source: None
        })
    );
    assert_eq!(Wellbeing::decode(0x2a06, &[0; 8]), Err(Error::InvalidField));
    assert_eq!(
        Wellbeing::decode(0x2a01, &[0; 7]),
        Err(Error::InvalidLength)
    );
    assert_eq!(
        Wellbeing::decode(0x2a01, &[0; 9]),
        Err(Error::InvalidLength)
    );
}
#[cfg(feature = "microcirculation")]
#[test]
fn test_should_decode_finite_positive_alk_with_real_algorithm_strings_and_exact_utf8() {
    use device_health::{Alk, Error, ResultSource};
    let literal = [0, 0, 0, 0x40, 1, 1, 1, 0, b'a', 1, 0, b'1'];
    let value = Alk {
        per_s: Some(2.0),
        source: Some(ResultSource::DeviceAlgorithm),
        algorithm_id: "a",
        algorithm_version: "1",
    };
    assert_eq!(Alk::decode(&literal), Ok(value));
    let mut out = [0; 12];
    assert_eq!(value.encode(&mut out), Ok(12));
    assert_eq!(out, literal);
    assert_eq!(
        Alk::decode(&[0; 10]),
        Ok(Alk {
            per_s: None,
            source: None,
            algorithm_id: "",
            algorithm_version: ""
        })
    );
    for (index, byte) in [(3, 0xc0), (5, 3), (4, 2), (5, 2), (8, 0xff), (8, 0)] {
        let mut bad = literal;
        bad[index] = byte;
        assert!(Alk::decode(&bad).is_err(), "index={index}");
    }
    let mut infinity = literal;
    infinity[..4].copy_from_slice(&[0, 0, 0x80, 0x7f]);
    assert_eq!(Alk::decode(&infinity), Err(Error::InvalidField));
    let mut nan = literal;
    nan[..4].copy_from_slice(&[0, 0, 0xc0, 0x7f]);
    assert_eq!(Alk::decode(&nan), Err(Error::InvalidField));
    let mut negative_zero = [0; 10];
    negative_zero[3] = 0x80;
    assert_eq!(Alk::decode(&negative_zero), Err(Error::InvalidField));
    assert_eq!(Alk::decode(&literal[..11]), Err(Error::InvalidLength));
    let mut extra = literal.to_vec();
    extra.push(0);
    assert_eq!(Alk::decode(&extra), Err(Error::InvalidLength));
    assert_eq!(value.encode(&mut [0; 11]), Err(Error::Capacity));
}
#[test]
fn test_should_dispatch_only_selected_exact_schemas_and_embedded_sources() {
    use device_health::{Error, ResultSource, validate_result_body};
    let cases: &[(u8, u16, u16, &[u8], ResultSource)] = &[
        #[cfg(feature = "heart-rate")]
        (
            0x20,
            0x0020,
            1,
            &[60, 0, 100, 1],
            ResultSource::DeviceAlgorithm,
        ),
        #[cfg(feature = "blood-pressure")]
        (
            0x25,
            0x2501,
            1,
            &[120, 0, 80, 0, 60, 0, 7, 0],
            ResultSource::DeviceAlgorithm,
        ),
        #[cfg(feature = "oxygen")]
        (
            0x26,
            0x2601,
            1,
            &[0x54, 0x26, 123, 0, 3, 0],
            ResultSource::DeviceAlgorithm,
        ),
        #[cfg(feature = "glucose")]
        (0x27, 0x2701, 1, &[0xa8, 2, 1], ResultSource::PhoneImport),
        #[cfg(feature = "temperature")]
        (
            0x28,
            0x2801,
            1,
            &[0x42, 0x0e, 3, 1],
            ResultSource::DeviceAlgorithm,
        ),
        #[cfg(feature = "ecg")]
        (
            0x29,
            0x2901,
            2,
            &[
                1, 0, 0, 0, 0, 0, 0, 0, 0, 60, 0, 61, 0, 62, 0, 55, 0, 80, 0, 31, 0,
            ],
            ResultSource::DeviceAlgorithm,
        ),
        #[cfg(feature = "wellbeing")]
        (
            0x2a,
            0x2a02,
            1,
            &[100, 0, 0, 0, 0, 1, 2, 1],
            ResultSource::PhoneImport,
        ),
        #[cfg(feature = "microcirculation")]
        (
            0x2b,
            0x2b01,
            2,
            &[0, 0, 0, 0x40, 1, 1, 1, 0, b'a', 1, 0, b'1'],
            ResultSource::DeviceAlgorithm,
        ),
    ];
    for &(module, record, schema, literal, source) in cases {
        assert_eq!(
            validate_result_body(module, record, schema, literal, source),
            Ok(())
        );
        // The three selected legacy results now also support explicit-origin schema2.
        // Their schema1 bodies still cannot be reused as schema2 without that byte.
        let unsupported = if matches!(module, 0x25..=0x27) {
            assert!(validate_result_body(module, record, 2, literal, source).is_err());
            3
        } else {
            schema + 1
        };
        assert_eq!(
            validate_result_body(module, record, unsupported, literal, source),
            Err(Error::Unsupported)
        );
        let mut extra = literal.to_vec();
        extra.push(0);
        assert_eq!(
            validate_result_body(module, record, schema, &extra, source),
            Err(Error::InvalidLength)
        );
        if module == 0x2a || module == 0x2b {
            let wrong = if source == ResultSource::DeviceAlgorithm {
                ResultSource::PhoneImport
            } else {
                ResultSource::DeviceAlgorithm
            };
            assert_eq!(
                validate_result_body(module, record, schema, literal, wrong),
                Err(Error::InvalidField)
            );
        }
    }
    assert_eq!(
        validate_result_body(0x7f, 0, 1, &[], ResultSource::DeviceAlgorithm),
        Err(Error::Unsupported)
    );
}
#[cfg(feature = "microcirculation")]
#[test]
fn test_should_bound_alk_lengths_and_reject_truncated_or_excessive_string_prefixes() {
    use device_health::{Alk, Error};
    let mut huge_prefix = [0; 10];
    huge_prefix[6] = 255;
    huge_prefix[7] = 255;
    assert_eq!(Alk::decode(&huge_prefix), Err(Error::InvalidLength));
    assert_eq!(Alk::decode(&[0; 498]), Err(Error::InvalidLength));
    let text = "a".repeat(488);
    let value = Alk {
        per_s: None,
        source: None,
        algorithm_id: &text,
        algorithm_version: "",
    };
    let mut output = [0xa5; 512];
    assert_eq!(value.encode(&mut output), Err(Error::InvalidLength));
    assert_eq!(output, [0xa5; 512]);
}
#[cfg(feature = "microcirculation")]
#[test]
fn test_should_distinguish_alk_coding_ceiling_from_response_and_record_carrier_capacity() {
    use device_health::{Alk, Error, Latest, ResultCarrier, ResultSource, Timestamp};
    let id = "a".repeat(486);
    let body = Alk {
        per_s: Some(2.0),
        source: Some(ResultSource::DeviceAlgorithm),
        algorithm_id: &id,
        algorithm_version: "1",
    };
    let mut bytes = [0; 497];
    assert_eq!(body.encode(&mut bytes), Ok(497));
    let latest = Latest::new(0x2b, 0x2b01, 2, Timestamp::unknown(), &bytes)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        latest.check_carrier(ResultCarrier::EventLatest, 512),
        Ok(512)
    );
    assert_eq!(
        latest.check_carrier(ResultCarrier::ResponseLatest, 512),
        Err(Error::Capacity)
    );
    assert_eq!(
        latest.check_carrier(ResultCarrier::HistoryRecord, 512),
        Err(Error::Capacity)
    );
    assert_eq!(
        latest.check_carrier(ResultCarrier::EventLatest, 128),
        Err(Error::Capacity)
    );
    assert_eq!(latest.payload().len(), 497);
}
