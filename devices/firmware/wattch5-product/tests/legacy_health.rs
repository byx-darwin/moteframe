#![cfg(feature = "legacy-health")]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use device_health::{ResultOrigin, SourcedResult};
use wattch5_product::runtime::legacy_health::{LegacyInputs, calculate};
#[test]
fn test_should_preserve_hr_formula_and_clock_generated_values_with_explicit_origins() {
    let inputs = LegacyInputs {
        heart_rate: Some(75),
        age: 30,
        male: true,
        reference_bp: None,
        hour: Some(6),
        clock_ticks: Some(0),
    };
    let mut out = [0; 9];
    let n = calculate(0x25, inputs, &mut out).unwrap();
    assert_eq!(&out[..n], &[117, 0, 76, 0, 75, 0, 7, 0, 1]);
    let n = calculate(0x26, inputs, &mut out).unwrap();
    assert_eq!(&out[..n], &[0x84, 0x26, 0, 0, 1, 0, 1]);
    let n = calculate(0x27, inputs, &mut out).unwrap();
    assert_eq!(&out[..n], &[0xc2, 1, 1, 3]);
    assert_eq!(
        SourcedResult::decode(0x27, &out[..n]).unwrap().origin(),
        ResultOrigin::LegacyGenerated
    );
}
#[test]
fn test_should_preserve_missing_inputs_and_page_content_without_source_labels() {
    use wattch5_product::runtime::legacy_health::measurement;
    let missing = LegacyInputs {
        heart_rate: None,
        age: 30,
        male: true,
        reference_bp: None,
        hour: Some(6),
        clock_ticks: Some(0),
    };
    assert!(calculate(0x25, missing, &mut [0; 9]).is_err());
    assert!(calculate(0x27, missing, &mut [0; 9]).is_err());
    let mut local = [118, 0, 78, 0, 0, 0, 3, 0, 1];
    let a = measurement(0x25, &local).unwrap();
    local[8] = 2;
    assert_eq!(a, measurement(0x25, &local).unwrap());
    let input = LegacyInputs {
        heart_rate: Some(85),
        hour: None,
        ..missing
    };
    assert!(calculate(0x27, input, &mut [0; 9]).is_err());
    let input = LegacyInputs {
        heart_rate: Some(85),
        reference_bp: Some((120, 80)),
        ..missing
    };
    let mut short = [0xa7; 8];
    assert!(calculate(0x25, input, &mut short).is_err());
    assert_eq!(short, [0xa7; 8]);
    let mut out = [0; 9];
    let n = calculate(0x25, input, &mut out).unwrap();
    assert_eq!(&out[..n], &[124, 0, 82, 0, 85, 0, 7, 0, 1]);
}

#[test]
fn test_should_retain_a_generated_result_until_acknowledged_even_after_stop() {
    use device_health::{Backend, HealthStart, JobId};
    use wattch5_product::runtime::legacy_health::{LegacyBackend, LegacyInputSource};
    struct Input;
    impl LegacyInputSource for Input {
        fn inputs(&self, _: u8) -> Result<LegacyInputs, device_health::Error> {
            Ok(LegacyInputs {
                heart_rate: Some(75),
                age: 30,
                male: true,
                reference_bp: None,
                hour: Some(6),
                clock_ticks: Some(0),
            })
        }
        fn timestamp(&self) -> device_health::Timestamp {
            device_health::Timestamp::unknown()
        }
    }
    let mut backend = LegacyBackend::new(&Input);
    let request = HealthStart::decode(0x25, &[1, 1, 0x25, 0, 0, 0, 0, 0]).unwrap();
    let job = JobId::new(1).unwrap();
    backend.start(job, request).unwrap();
    let mut before = [0; 9];
    let info = backend.result(job, &mut before).unwrap().unwrap();
    backend.stop(job).unwrap();
    assert!(backend.has_pending_result(job).unwrap());
    let mut after = [0; 9];
    assert_eq!(backend.result(job, &mut after).unwrap(), Some(info));
    assert_eq!(before, after);
    backend.acknowledge_result(job, info.sample_id).unwrap();
    assert!(!backend.has_pending_result(job).unwrap());
}

#[test]
fn test_should_match_original_c_boundaries_rounding_and_every_clock_digit() {
    for row in include_str!("fixtures/legacy_health_c_oracle.txt").lines() {
        let mut parts = row.split_whitespace();
        let kind = parts.next().unwrap();
        let values: Vec<u16> = parts.map(|v| v.parse().unwrap()).collect();
        let mut input = LegacyInputs {
            heart_rate: Some(75),
            age: 30,
            male: true,
            reference_bp: None,
            hour: Some(0),
            clock_ticks: Some(0),
        };
        let mut out = [0; 9];
        match kind {
            "b" => {
                input.age = u8::try_from(values[0]).unwrap();
                input.male = values[1] == 1;
                input.heart_rate = Some(values[2]);
                input.reference_bp = (values[3] == 1).then_some((120, 80));
                calculate(0x25, input, &mut out).unwrap();
                assert_eq!(
                    (u16::from(out[0]), u16::from(out[2])),
                    (values[4], values[5]),
                    "{row}"
                );
            }
            "o" => {
                input.heart_rate = Some(values[0]);
                calculate(0x26, input, &mut out).unwrap();
                assert_eq!(u16::from_le_bytes([out[0], out[1]]), values[1], "{row}");
            }
            "g" => {
                input.hour = Some(u8::try_from(values[0]).unwrap());
                input.clock_ticks = Some(u32::from(values[1]));
                calculate(0x27, input, &mut out).unwrap();
                assert_eq!(u16::from_le_bytes([out[0], out[1]]), values[2], "{row}");
            }
            _ => panic!("unknown independent fixture row"),
        }
    }
}

#[test]
fn test_should_keep_valid_wire_values_inside_the_existing_page_representation() {
    use wattch5_parity_ui_demo::{MeasureState, Reading};
    use wattch5_product::runtime::legacy_health::measurement;
    // Original UI uses the integer SpO2 part; wire/history retain its hundredths.
    assert_eq!(
        measurement(0x26, &[0x84, 0x26, 0, 0, 1, 0, 1])
            .unwrap()
            .reading,
        Reading::Number(98)
    );
    assert_eq!(
        measurement(0x26, &[0x10, 0x27, 0, 0, 1, 0, 2])
            .unwrap()
            .reading,
        Reading::Number(100)
    );
    assert_eq!(
        measurement(0x27, &[0xff, 0xff, 1, 2]).unwrap().reading,
        Reading::Tenth(6554)
    );
    // Protocol u16 support must not inject an out-of-range three-digit pressure UI value.
    let value = measurement(0x25, &[0xff, 0xff, 80, 0, 0, 0, 3, 0, 2]).unwrap();
    assert_eq!(value.state, MeasureState::Error);
    assert_eq!(value.reading, Reading::Missing);
}
