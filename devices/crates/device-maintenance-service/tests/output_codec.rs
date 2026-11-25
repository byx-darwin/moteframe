//! Named-operation outputs must have real typed structure before C42 can expose them.
#![cfg(any(feature = "engineering", feature = "factory-config"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]
#[cfg(feature = "factory-config")]
use device_maintenance_service::Error;
use device_maintenance_service::{NamedOutput, NamedOutputValue};
#[cfg(feature = "engineering")]
#[test]
fn test_should_decode_named_output_fields_and_reencode_exact_bytes() {
    let body = [2, 0, 2, 0, 0xff, 0xff, 0xff, 0xff, 7, 0, 0, 0, 0, 0];
    let output = NamedOutput::decode(0x72, 5, 1, &body).unwrap();
    let NamedOutputValue::AdcSamples(samples) = output.value() else {
        panic!("ADC type");
    };
    assert_eq!(samples.channel(), 2);
    assert_eq!(samples.samples().collect::<Vec<_>>(), [-1, 7]);
    assert_eq!((samples.unit(), samples.scale_exp()), (0, 0));
    let mut out = [0; 14];
    assert_eq!(output.encode(&mut out), Ok(14));
    assert_eq!(out, body);
    assert!(NamedOutput::decode(0x72, 5, 1, &body[..13]).is_err());
    let mut bad_count = body;
    bad_count[2] = 3;
    assert!(NamedOutput::decode(0x72, 5, 1, &bad_count).is_err());
    assert!(NamedOutput::decode(0x72, 5, 2, &body).is_err());
}
#[cfg(feature = "engineering")]
#[test]
fn test_should_walk_bounded_gesture_samples_and_reject_truncated_nested_parameters() {
    let body = [
        1, 0, 10, 0, 0, 0, 0, 0, 0, 0, 3, 0, 1, 0xfe, 0xff, 0xff, 0xff,
    ];
    let output = NamedOutput::decode(0x72, 9, 1, &body).unwrap();
    let NamedOutputValue::GestureSamples(samples) = output.value() else {
        panic!("gesture type");
    };
    let sample = samples.samples().next().unwrap().unwrap();
    assert_eq!((sample.monotonic_us, sample.gesture_id), (10, 3));
    assert_eq!(sample.parameters().collect::<Vec<_>>(), [-2]);
    assert!(NamedOutput::decode(0x72, 9, 1, &body[..16]).is_err());
    let mut tail = body.to_vec();
    tail.push(0);
    assert!(NamedOutput::decode(0x72, 9, 1, &tail).is_err());
    assert!(NamedOutput::decode(0x72, 12, 1, &[0]).is_err());
    assert!(matches!(
        NamedOutput::decode(0x72, 12, 1, &[]).unwrap().value(),
        NamedOutputValue::Empty
    ));
}
#[cfg(feature = "factory-config")]
#[test]
fn test_should_preserve_verified_identity_readback_and_reject_invalid_enabled_or_serial() {
    let output = NamedOutput::decode(0x73, 2, 1, &[5, 0, b'0', b'0', b'1', b'2', b'3']).unwrap();
    assert!(matches!(output.value(), NamedOutputValue::Serial("00123")));
    for body in [&[0, 0][..], &[1, 0, b'x'][..], &[1, 0, 0][..]] {
        assert!(NamedOutput::decode(0x73, 2, 1, body).is_err());
    }
    assert!(NamedOutput::decode(0x73, 4, 1, &[2]).is_err());
    assert!(matches!(
        NamedOutput::decode(0x73, 4, 1, &[1]).unwrap().value(),
        NamedOutputValue::Enabled(true)
    ));
    let mut out = [0xa5; 1];
    assert_eq!(
        NamedOutput::decode(0x73, 1, 1, &[1, 0])
            .unwrap()
            .encode(&mut out),
        Err(Error::Capacity)
    );
    assert_eq!(out, [0xa5]);
    assert!(NamedOutput::decode(0x73, 6, 1, &[]).is_err());
}
#[cfg(feature = "engineering")]
#[test]
fn test_should_match_all_fixed_engineering_operation_vectors() {
    let cases: &[(u16, &[u8], NamedOutputValue<'_>)] = &[
        (
            1,
            &[0x34, 0x12, 0xcd, 0xab],
            NamedOutputValue::Driver {
                identifier: 0x1234,
                status: 0xabcd,
            },
        ),
        (
            2,
            &[2, 0, 3, 0],
            NamedOutputValue::Driver {
                identifier: 2,
                status: 3,
            },
        ),
        (
            3,
            &[1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0],
            NamedOutputValue::ResourceCheck {
                expected_crc: 1,
                actual_crc: 2,
                bytes_checked: 3,
            },
        ),
        (4, &[5, 0, 0, 0], NamedOutputValue::InterruptCount(5)),
        (
            6,
            &[
                0xff, 0xff, 0xff, 0xff, 1, 0, 0, 0, 0xfe, 0xff, 0xff, 0xff, 0, 0xff,
            ],
            NamedOutputValue::AdcCheck {
                raw: -1,
                reference: 1,
                error: -2,
                unit: 0,
                scale_exp: -1,
            },
        ),
        (7, &[9, 0, 0, 0], NamedOutputValue::CalibrationRevision(9)),
        (8, &[8, 0, 0, 0], NamedOutputValue::CalibrationRevision(8)),
        (
            10,
            &[1, 0, 0, 0, 2, 0, 0, 0],
            NamedOutputValue::Ageing {
                elapsed_s: 1,
                failure_mask: 2,
            },
        ),
        (11, &[0xe8, 3, 0, 0], NamedOutputValue::OffDurationMs(1000)),
        (12, &[], NamedOutputValue::Empty),
    ];
    for (operation, body, value) in cases {
        let parsed = NamedOutput::decode(0x72, *operation, 1, body).unwrap();
        assert_eq!(parsed.value(), *value);
        let mut out = [0; 32];
        let size = parsed.encode(&mut out).unwrap();
        assert_eq!(&out[..size], *body);
        let mut tail = body.to_vec();
        tail.push(0);
        assert!(NamedOutput::decode(0x72, *operation, 1, &tail).is_err());
        if !body.is_empty() {
            assert!(NamedOutput::decode(0x72, *operation, 1, &body[..body.len() - 1]).is_err());
        }
    }
}
#[cfg(feature = "factory-config")]
#[test]
fn test_should_match_every_factory_scalar_vector() {
    for (operation, body, expected) in [
        (1, &[0x34, 0x12][..], NamedOutputValue::ProductId(0x1234)),
        (3, &[9, 0][..], NamedOutputValue::Subtype(9)),
        (4, &[0][..], NamedOutputValue::Enabled(false)),
        (5, &[1][..], NamedOutputValue::Enabled(true)),
    ] {
        assert_eq!(
            NamedOutput::decode(0x73, operation, 1, body)
                .unwrap()
                .value(),
            expected
        );
        assert!(NamedOutput::decode(0x73, operation, 1, &body[..body.len() - 1]).is_err());
    }
    assert!(NamedOutput::decode(0x73, 1, 1, &[0; 503]).is_err());
}
