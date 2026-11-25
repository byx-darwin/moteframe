#[cfg(feature = "oxygen")]
#[test]
fn test_should_never_promote_old_hr_derived_oxygen_to_sensor_measurement() {
    use device_health::{
        AcquisitionKind, Error, Module, Oxygen, ProductDeclaration, Provenance, ResultSource,
    };
    let declaration = ProductDeclaration {
        module: Module::from_wire(0x26).unwrap_or_else(|e| panic!("{e:?}")),
        record_type: 0x2601,
        acquisition: AcquisitionKind::DerivedEstimate,
        algorithm_id: "legacy-hr-estimate",
        algorithm_version: "audited-c",
        temperature_modes: 0,
        mood_labels: &[],
    };
    let provenance = Provenance {
        source: ResultSource::DeviceAlgorithm,
        acquisition: AcquisitionKind::DerivedEstimate,
        algorithm_id: "legacy-hr-estimate",
        algorithm_version: "audited-c",
    };
    let body = Oxygen {
        spo2_x100: Some(9800),
        pi_x100: None,
    };
    assert_eq!(declaration.validate(&body, &provenance), Ok(()));
    assert_eq!(
        declaration.validate_sensor_measurement(&body, &provenance),
        Err(Error::InvalidField)
    );
    let lie = Provenance {
        acquisition: AcquisitionKind::SensorMeasurement,
        ..provenance
    };
    assert_eq!(declaration.validate(&body, &lie), Err(Error::InvalidField));
}
#[cfg(feature = "temperature")]
#[test]
fn test_should_require_declared_temperature_mode_algorithm_and_source() {
    use device_health::{
        AcquisitionKind, Error, Module, ProductDeclaration, Provenance, ResultSource, Temperature,
        TemperatureMode,
    };
    let declaration = ProductDeclaration {
        module: Module::from_wire(0x28).unwrap_or_else(|e| panic!("{e:?}")),
        record_type: 0x2801,
        acquisition: AcquisitionKind::SensorMeasurement,
        algorithm_id: "selected-temp",
        algorithm_version: "reviewed-version",
        temperature_modes: 4,
        mood_labels: &[],
    };
    let provenance = Provenance {
        source: ResultSource::DeviceAlgorithm,
        acquisition: AcquisitionKind::SensorMeasurement,
        algorithm_id: "selected-temp",
        algorithm_version: "reviewed-version",
    };
    let body = Temperature {
        centi_c: Some(3650),
        mode: TemperatureMode::Body,
    };
    assert_eq!(
        declaration.validate_sensor_measurement(&body, &provenance),
        Ok(())
    );
    assert_eq!(
        declaration.validate(
            &Temperature {
                mode: TemperatureMode::Object,
                ..body
            },
            &provenance
        ),
        Err(Error::Unsupported)
    );
    assert_eq!(
        declaration.validate(
            &body,
            &Provenance {
                algorithm_version: "invented",
                ..provenance
            }
        ),
        Err(Error::InvalidField)
    );
    assert_eq!(
        declaration.validate(
            &body,
            &Provenance {
                source: ResultSource::PhoneImport,
                ..provenance
            }
        ),
        Err(Error::InvalidField)
    );
    assert_eq!(
        ProductDeclaration {
            temperature_modes: 16,
            ..declaration
        }
        .validate(&body, &provenance),
        Err(Error::InvalidField)
    );
}
#[cfg(feature = "wellbeing")]
#[test]
fn test_should_validate_mood_against_product_dictionary_without_inventing_generic_labels() {
    use device_health::{
        AcquisitionKind, Error, Module, MoodLabel, ProductDeclaration, Provenance, ResultSource,
        Wellbeing, WellbeingKind,
    };
    let labels = [MoodLabel {
        value: -3,
        label: "product-defined-state",
    }];
    let declaration = ProductDeclaration {
        module: Module::from_wire(0x2a).unwrap_or_else(|e| panic!("{e:?}")),
        record_type: 0x2a03,
        acquisition: AcquisitionKind::PhoneImport,
        algorithm_id: "phone-provider",
        algorithm_version: "provider-version",
        temperature_modes: 0,
        mood_labels: &labels,
    };
    let provenance = Provenance {
        source: ResultSource::PhoneImport,
        acquisition: AcquisitionKind::PhoneImport,
        algorithm_id: "phone-provider",
        algorithm_version: "provider-version",
    };
    let body = Wellbeing {
        kind: WellbeingKind::Mood,
        value: Some(-3),
        scale_exp: 0,
        source: Some(ResultSource::PhoneImport),
    };
    assert_eq!(declaration.validate(&body, &provenance), Ok(()));
    assert_eq!(
        declaration.validate(
            &Wellbeing {
                value: Some(1),
                ..body
            },
            &provenance
        ),
        Err(Error::InvalidField)
    );
    assert_eq!(
        declaration.validate(
            &Wellbeing {
                source: Some(ResultSource::DeviceAlgorithm),
                ..body
            },
            &provenance
        ),
        Err(Error::InvalidField)
    );
    assert_eq!(
        ProductDeclaration {
            mood_labels: &[],
            ..declaration
        }
        .validate(&body, &provenance),
        Err(Error::InvalidField)
    );
    assert_eq!(
        declaration.validate_sensor_measurement(&body, &provenance),
        Err(Error::InvalidField)
    );
}
#[cfg(feature = "microcirculation")]
#[test]
fn test_should_match_embedded_alk_algorithm_metadata_to_product_declaration() {
    use device_health::{
        AcquisitionKind, Alk, Error, Module, ProductDeclaration, Provenance, ResultSource,
    };
    let declaration = ProductDeclaration {
        module: Module::from_wire(0x2b).unwrap_or_else(|e| panic!("{e:?}")),
        record_type: 0x2b01,
        acquisition: AcquisitionKind::DerivedEstimate,
        algorithm_id: "alk-provider",
        algorithm_version: "v2",
        temperature_modes: 0,
        mood_labels: &[],
    };
    let provenance = Provenance {
        source: ResultSource::DeviceAlgorithm,
        acquisition: AcquisitionKind::DerivedEstimate,
        algorithm_id: "alk-provider",
        algorithm_version: "v2",
    };
    let body = Alk {
        per_s: Some(2.0),
        source: Some(ResultSource::DeviceAlgorithm),
        algorithm_id: "alk-provider",
        algorithm_version: "v2",
    };
    assert_eq!(declaration.validate(&body, &provenance), Ok(()));
    assert_eq!(
        declaration.validate(
            &Alk {
                algorithm_version: "unknown",
                ..body
            },
            &provenance
        ),
        Err(Error::InvalidField)
    );
}
#[cfg(feature = "temperature")]
#[test]
fn test_should_not_classify_absent_temperature_or_unversioned_declaration_as_measurement() {
    use device_health::{
        AcquisitionKind, Error, Module, ProductDeclaration, Provenance, ResultSource, Temperature,
        TemperatureMode,
    };
    let declaration = ProductDeclaration {
        module: Module::from_wire(0x28).unwrap_or_else(|e| panic!("{e:?}")),
        record_type: 0x2801,
        acquisition: AcquisitionKind::SensorMeasurement,
        algorithm_id: "sensor",
        algorithm_version: "actual-version",
        temperature_modes: 4,
        mood_labels: &[],
    };
    let provenance = Provenance {
        source: ResultSource::DeviceAlgorithm,
        acquisition: AcquisitionKind::SensorMeasurement,
        algorithm_id: "sensor",
        algorithm_version: "actual-version",
    };
    let body = Temperature {
        centi_c: None,
        mode: TemperatureMode::Body,
    };
    assert_eq!(declaration.validate(&body, &provenance), Ok(()));
    assert_eq!(
        declaration.validate_sensor_measurement(&body, &provenance),
        Err(Error::NotFound)
    );
    assert_eq!(
        ProductDeclaration {
            algorithm_version: "",
            ..declaration
        }
        .validate(&body, &provenance),
        Err(Error::InvalidField)
    );
    assert_eq!(
        ProductDeclaration {
            record_type: 0x0020,
            ..declaration
        }
        .validate(&body, &provenance),
        Err(Error::InvalidField)
    );
}
#[cfg(feature = "wellbeing")]
#[test]
fn test_should_bound_product_mood_dictionary_to_its_u8_count_contract() {
    use device_health::{
        AcquisitionKind, Error, Module, MoodLabel, ProductDeclaration, Provenance, ResultSource,
        Wellbeing, WellbeingKind,
    };
    let labels: Vec<_> = (0..256)
        .map(|value| MoodLabel {
            value,
            label: "declared-state",
        })
        .collect();
    let declaration = ProductDeclaration {
        module: Module::from_wire(0x2a).unwrap_or_else(|e| panic!("{e:?}")),
        record_type: 0x2a03,
        acquisition: AcquisitionKind::PhoneImport,
        algorithm_id: "provider",
        algorithm_version: "version",
        temperature_modes: 0,
        mood_labels: &labels,
    };
    let provenance = Provenance {
        source: ResultSource::PhoneImport,
        acquisition: AcquisitionKind::PhoneImport,
        algorithm_id: "provider",
        algorithm_version: "version",
    };
    let body = Wellbeing {
        kind: WellbeingKind::Mood,
        value: Some(0),
        scale_exp: 0,
        source: Some(ResultSource::PhoneImport),
    };
    assert_eq!(
        declaration.validate(&body, &provenance),
        Err(Error::InvalidField)
    );
}
