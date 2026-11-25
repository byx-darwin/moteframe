#![cfg(any(
    feature = "heart-rate",
    feature = "blood-pressure",
    feature = "oxygen",
    feature = "temperature"
))]
//! E83 is a typed ordinary body; no policy engine or event source is registered by this codec.
use device_health::{Error, ThresholdEvent, ThresholdSample, ThresholdTransition, Timestamp};

#[cfg(feature = "heart-rate")]
#[test]
fn test_should_encode_exact_unsigned_threshold_body_without_crc_or_status() {
    let value = ThresholdEvent {
        alarm_id: 1,
        transition: ThresholdTransition::Trigger,
        job: 7,
        timestamp: Timestamp::unknown(),
        sample: ThresholdSample::HeartRate {
            value: 120,
            threshold: 100,
        },
    };
    let mut out = [0; 22];
    assert_eq!(value.encode(&mut out), Ok(22));
    assert_eq!(
        out,
        [
            1, 0, 0, 0, 1, 7, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 120, 0, 100, 0
        ]
    );
    assert_eq!(ThresholdEvent::decode(0x20, &out), Ok(value));
    for (index, bad) in [(0, 0), (4, 0), (4, 4), (17, 2)] {
        let mut bytes = out;
        bytes[index] = bad;
        assert!(ThresholdEvent::decode(0x20, &bytes).is_err());
    }
    let mut bad = out;
    bad[9] = 1;
    assert_eq!(ThresholdEvent::decode(0x20, &bad), Err(Error::InvalidField));
    assert_eq!(value.encode(&mut [0; 21]), Err(Error::Capacity));
    assert!(!format!("{value:?}").contains("120"));
}
#[cfg(feature = "temperature")]
#[test]
fn test_should_preserve_signed_temperature_and_explicit_mode_in23_bytes() {
    let value = ThresholdEvent {
        alarm_id: 2,
        transition: ThresholdTransition::ConditionClear,
        job: 0,
        timestamp: Timestamp::unknown(),
        sample: ThresholdSample::Temperature {
            value: -100,
            threshold: 3800,
            mode: device_health::TemperatureMode::Body,
        },
    };
    let mut out = [0; 23];
    assert_eq!(value.encode(&mut out), Ok(23));
    assert_eq!(&out[18..23], &[156, 255, 216, 14, 3]);
    assert_eq!(ThresholdEvent::decode(0x28, &out), Ok(value));
    out[22] = 0;
    assert_eq!(ThresholdEvent::decode(0x28, &out), Err(Error::InvalidField));
}

fn declared(
    module: u8,
    record: u16,
    acquisition: device_health::AcquisitionKind,
) -> device_health::ProductDeclaration<'static> {
    device_health::ProductDeclaration {
        module: device_health::Module::from_wire(module).unwrap_or_else(|e| panic!("{e:?}")),
        record_type: record,
        acquisition,
        algorithm_id: "actual-provider",
        algorithm_version: "1",
        temperature_modes: if module == 0x28 { 4 } else { 0 },
        mood_labels: &[],
    }
}
fn provenance(
    acquisition: device_health::AcquisitionKind,
    source: device_health::ResultSource,
) -> device_health::Provenance<'static> {
    device_health::Provenance {
        source,
        acquisition,
        algorithm_id: "actual-provider",
        algorithm_version: "1",
    }
}
fn actual(
    record: u16,
    length: usize,
    source: device_health::ResultSource,
) -> device_health::ResultInfo {
    device_health::ResultInfo {
        sample_id: 1,
        record_type: record,
        schema: 1,
        timestamp: Timestamp::unknown(),
        length,
        source,
    }
}
#[cfg(feature = "heart-rate")]
#[test]
fn test_should_validate_actual_sample_source_without_promoting_imports_or_estimates() {
    use device_health::{AcquisitionKind, ResultSource};
    let event = ThresholdEvent {
        alarm_id: 1,
        transition: ThresholdTransition::Trigger,
        job: 1,
        timestamp: Timestamp::unknown(),
        sample: ThresholdSample::HeartRate {
            value: 120,
            threshold: 100,
        },
    };
    for (acquisition, source) in [
        (
            AcquisitionKind::SensorMeasurement,
            ResultSource::DeviceAlgorithm,
        ),
        (
            AcquisitionKind::DerivedEstimate,
            ResultSource::DeviceAlgorithm,
        ),
        (AcquisitionKind::PhoneImport, ResultSource::PhoneImport),
    ] {
        let d = declared(0x20, 0x20, acquisition);
        let p = provenance(acquisition, source);
        let info = actual(0x20, 4, source);
        assert_eq!(
            event.validate_observation(info, &[120, 0, 100, 1], &d, &p),
            Ok(())
        );
        assert_eq!(
            event.validate_observation(info, &[0, 0, 255, 0], &d, &p),
            Err(Error::NotFound)
        );
        if acquisition != AcquisitionKind::SensorMeasurement {
            let body = device_health::HeartRate::decode(&[120, 0, 100, 1])
                .unwrap_or_else(|e| panic!("{e:?}"));
            assert!(d.validate_sensor_measurement(&body, &p).is_err());
        }
        let wrong = provenance(
            acquisition,
            if source == ResultSource::PhoneImport {
                ResultSource::DeviceAlgorithm
            } else {
                ResultSource::PhoneImport
            },
        );
        assert!(
            event
                .validate_observation(info, &[120, 0, 100, 1], &d, &wrong)
                .is_err()
        );
        let mut wrong_time = info;
        wrong_time.timestamp = Timestamp::new(1, true).unwrap_or_else(|e| panic!("{e:?}"));
        assert!(
            event
                .validate_observation(wrong_time, &[120, 0, 100, 1], &d, &p)
                .is_err()
        );
    }
}
#[cfg(feature = "blood-pressure")]
#[test]
fn test_should_require_actual_systolic_and_not_substitute_pulse_or_diastolic() {
    use device_health::{AcquisitionKind, ResultSource};
    let source = ResultSource::DeviceAlgorithm;
    let event = ThresholdEvent {
        alarm_id: 1,
        transition: ThresholdTransition::Trigger,
        job: 7,
        timestamp: Timestamp::unknown(),
        sample: ThresholdSample::BloodPressure {
            value: 160,
            threshold: 140,
        },
    };
    let d = declared(0x25, 0x2501, AcquisitionKind::SensorMeasurement);
    let p = provenance(AcquisitionKind::SensorMeasurement, source);
    let info = actual(0x2501, 8, source);
    assert_eq!(
        event.validate_observation(info, &[160, 0, 80, 0, 60, 0, 7, 0], &d, &p),
        Ok(())
    );
    assert_eq!(
        event.validate_observation(info, &[0, 0, 80, 0, 60, 0, 6, 0], &d, &p),
        Err(Error::NotFound)
    );
    let mut out = [0; 22];
    assert_eq!(event.encode(&mut out), Ok(22));
    assert_eq!(ThresholdEvent::decode(0x25, &out), Ok(event));
}
#[cfg(feature = "oxygen")]
#[test]
fn test_should_require_actual_oxygen_and_never_use_pi_as_the_alarm_value() {
    use device_health::{AcquisitionKind, ResultSource};
    let source = ResultSource::DeviceAlgorithm;
    let event = ThresholdEvent {
        alarm_id: 1,
        transition: ThresholdTransition::Trigger,
        job: 7,
        timestamp: Timestamp::unknown(),
        sample: ThresholdSample::Oxygen {
            value: 9000,
            threshold: 9200,
        },
    };
    let d = declared(0x26, 0x2601, AcquisitionKind::SensorMeasurement);
    let p = provenance(AcquisitionKind::SensorMeasurement, source);
    let info = actual(0x2601, 6, source);
    assert_eq!(
        event.validate_observation(info, &[40, 35, 100, 0, 3, 0], &d, &p),
        Ok(())
    );
    assert_eq!(
        event.validate_observation(info, &[0, 0, 100, 0, 2, 0], &d, &p),
        Err(Error::NotFound)
    );
    let mut out = [0; 22];
    assert_eq!(event.encode(&mut out), Ok(22));
    assert_eq!(ThresholdEvent::decode(0x26, &out), Ok(event));
    let invalid = ThresholdEvent {
        sample: ThresholdSample::Oxygen {
            value: 10001,
            threshold: 9200,
        },
        ..event
    };
    assert!(invalid.encode(&mut out).is_err());
}
#[cfg(feature = "temperature")]
#[test]
fn test_should_require_matching_actual_supported_temperature_mode() {
    use device_health::{AcquisitionKind, ResultSource, TemperatureMode};
    let source = ResultSource::DeviceAlgorithm;
    let event = ThresholdEvent {
        alarm_id: 1,
        transition: ThresholdTransition::Trigger,
        job: 7,
        timestamp: Timestamp::unknown(),
        sample: ThresholdSample::Temperature {
            value: 3900,
            threshold: 3800,
            mode: TemperatureMode::Body,
        },
    };
    let d = declared(0x28, 0x2801, AcquisitionKind::SensorMeasurement);
    let p = provenance(AcquisitionKind::SensorMeasurement, source);
    let info = actual(0x2801, 4, source);
    assert_eq!(
        event.validate_observation(info, &[60, 15, 3, 1], &d, &p),
        Ok(())
    );
    assert_eq!(
        event.validate_observation(info, &[0, 0, 3, 0], &d, &p),
        Err(Error::NotFound)
    );
    assert!(
        event
            .validate_observation(info, &[60, 15, 2, 1], &d, &p)
            .is_err()
    );
}
