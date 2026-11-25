#![cfg(feature = "wellbeing")]
//! Schema2 extends only fatigue; schema1 and other wellbeing types retain their body.
use device_health::{ResultSource, validate_result_body};

#[test]
fn test_should_accept_exact_fatigue_snapshot_and_keep_schema1_compatible() {
    let bytes = [2, 0, 0, 0, 0, 2, 1, 1, 60, 0, 1, 5];
    assert_eq!(
        validate_result_body(0x2a, 0x2a01, 2, &bytes, ResultSource::DeviceAlgorithm),
        Ok(())
    );
    assert_eq!(
        validate_result_body(0x2a, 0x2a01, 1, &bytes[..8], ResultSource::DeviceAlgorithm),
        Ok(())
    );
    for kind in 0x2a02..=0x2a05 {
        assert!(
            validate_result_body(0x2a, kind, 2, &bytes, ResultSource::DeviceAlgorithm).is_err()
        );
    }
}

#[test]
fn test_should_keep_resting_validity_independent_and_require_same_known_source() {
    use device_health::{
        AcquisitionKind, Error, FatigueSnapshot, FatigueTrigger, Module, ProductDeclaration,
        Provenance, ResultBody, WellbeingKind,
    };
    let bytes = [0, 0, 0, 0, 0, 2, 1, 0, 60, 0, 1, 2];
    let value = FatigueSnapshot::decode(&bytes).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(value.fatigue.value, None);
    assert_eq!(value.resting_bpm, Some(60));
    assert_eq!(value.trigger, FatigueTrigger::DeviceUser);
    assert!(value.has_value());
    let mut out = [0; 12];
    assert_eq!(value.encode(&mut out), Ok(12));
    assert_eq!(out, bytes);
    assert_eq!(value.schema(), 2);
    let declaration = ProductDeclaration {
        module: Module::from_wire(0x2a).unwrap_or_else(|e| panic!("{e:?}")),
        record_type: 0x2a01,
        acquisition: AcquisitionKind::SensorMeasurement,
        algorithm_id: "actual",
        algorithm_version: "1",
        temperature_modes: 0,
        mood_labels: &[],
    };
    let mut provenance = Provenance {
        source: ResultSource::DeviceAlgorithm,
        acquisition: AcquisitionKind::SensorMeasurement,
        algorithm_id: "actual",
        algorithm_version: "1",
    };
    assert_eq!(
        declaration.validate_sensor_measurement(&value, &provenance),
        Ok(())
    );
    provenance.source = ResultSource::PhoneImport;
    assert!(declaration.validate(&value, &provenance).is_err());
    assert_eq!(
        validate_result_body(0x2a, 0x2a01, 2, &bytes, ResultSource::PhoneImport),
        Err(Error::InvalidField)
    );
    for (index, bad) in [(6, 0), (8, 0), (10, 0), (10, 2), (11, 6)] {
        let mut malformed = bytes;
        malformed[index] = bad;
        assert!(FatigueSnapshot::decode(&malformed).is_err());
    }
    let mut wrong_kind = value;
    wrong_kind.fatigue.kind = WellbeingKind::Stress;
    assert_eq!(wrong_kind.validate(), Err(Error::InvalidField));
    assert_eq!(value.encode(&mut [0; 11]), Err(Error::Capacity));
    assert!(FatigueSnapshot::decode(&bytes[..11]).is_err());
    assert!(!format!("{value:?}").contains("60"));
}

#[test]
fn test_should_roundtrip_latest_without_changing_other_wellbeing_schema1() {
    use device_health::{FatigueSnapshot, Latest, Timestamp};
    let bytes = [2, 0, 0, 0, 0, 2, 2, 1, 0, 0, 0, 5];
    assert!(FatigueSnapshot::decode(&bytes).is_ok());
    let latest = Latest::new(0x2a, 0x2a01, 2, Timestamp::unknown(), &bytes)
        .unwrap_or_else(|e| panic!("{e:?}"));
    let mut out = [0; 27];
    assert_eq!(latest.encode(&mut out), Ok(27));
    let decoded = Latest::decode(0x2a, &out).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(decoded.schema(), 2);
    assert_eq!(decoded.payload(), &bytes);
    assert_eq!(
        validate_result_body(
            0x2a,
            0x2a02,
            1,
            &[80, 0, 0, 0, 0, 1, 1, 1],
            ResultSource::DeviceAlgorithm
        ),
        Ok(())
    );
}
