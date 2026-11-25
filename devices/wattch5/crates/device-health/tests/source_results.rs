//! Literal schema2 vectors: the final byte is provenance, never a sensor-validity bit.
#[cfg(all(feature = "blood-pressure", feature = "oxygen", feature = "glucose"))]
#[test]
fn test_should_preserve_three_origins_in_exact_result_bodies() {
    use device_health::{ResultSource, validate_result_body};
    for (module, kind, bytes, source) in [
        (
            0x25,
            0x2501,
            &[120, 0, 80, 0, 60, 0, 7, 0, 1][..],
            ResultSource::DeviceAlgorithm,
        ),
        (
            0x25,
            0x2501,
            &[118, 0, 78, 0, 0, 0, 3, 0, 2][..],
            ResultSource::PhoneImport,
        ),
        (
            0x26,
            0x2601,
            &[0x48, 0x26, 0, 0, 1, 0, 1][..],
            ResultSource::DeviceAlgorithm,
        ),
        (
            0x26,
            0x2601,
            &[0x48, 0x26, 0, 0, 1, 0, 2][..],
            ResultSource::PhoneImport,
        ),
        (
            0x27,
            0x2701,
            &[0x26, 2, 1, 3][..],
            ResultSource::DeviceAlgorithm,
        ),
        (
            0x27,
            0x2701,
            &[0x58, 2, 1, 2][..],
            ResultSource::PhoneImport,
        ),
    ] {
        assert_eq!(validate_result_body(module, kind, 2, bytes, source), Ok(()));
    }
}
#[cfg(all(feature = "blood-pressure", feature = "oxygen", feature = "glucose"))]
#[test]
fn test_should_reject_source_forgery_unknown_sources_and_schema_confusion() {
    use device_health::{ResultSource as S, validate_result_body as validate};
    let bp = [120, 0, 80, 0, 0, 0, 3, 0, 1];
    assert!(validate(0x25, 0x2501, 2, &bp, S::PhoneImport).is_err());
    assert!(validate(0x25, 0x2501, 1, &bp, S::DeviceAlgorithm).is_err());
    assert!(validate(0x25, 0x2501, 2, &bp[..8], S::DeviceAlgorithm).is_err());
    for origin in [0, 3, 4, 255] {
        let mut bad = bp;
        bad[8] = origin;
        assert!(validate(0x25, 0x2501, 2, &bad, S::DeviceAlgorithm).is_err());
    }
    assert!(validate(0x27, 0x2701, 2, &[0x26, 2, 1, 1], S::DeviceAlgorithm).is_err());
    assert!(
        validate(
            0x26,
            0x2601,
            2,
            &[0x11, 0x27, 0, 0, 1, 0, 1],
            S::DeviceAlgorithm
        )
        .is_err()
    );
    assert_eq!(
        validate(0x25, 0x2501, 1, &bp[..8], S::DeviceAlgorithm),
        Ok(())
    );
}
#[cfg(feature = "glucose")]
#[test]
fn test_should_encode_exactly_one_source_byte_and_preserve_short_output() {
    use device_health::{ResultOrigin, SourcedResult};
    let sourced = SourcedResult::new(0x27, &[0x26, 2, 1], ResultOrigin::LegacyGenerated)
        .unwrap_or_else(|e| panic!("{e:?}"));
    let mut short = [0xa7; 3];
    assert!(sourced.encode(&mut short).is_err());
    assert_eq!(short, [0xa7; 3]);
    let mut out = [0; 4];
    assert_eq!(sourced.encode(&mut out), Ok(4));
    assert_eq!(out, [0x26, 2, 1, 3]);
    assert!(!format!("{sourced:?}").contains("550"));
}
#[cfg(feature = "oxygen")]
#[test]
fn test_should_not_invent_pi_for_the_legacy_hr_oxygen_estimate() {
    assert!(device_health::SourcedResult::decode(0x26, &[0x48, 0x26, 123, 0, 3, 0, 1]).is_err());
    assert!(device_health::SourcedResult::decode(0x26, &[0x48, 0x26, 123, 0, 3, 0, 2]).is_ok());
}
