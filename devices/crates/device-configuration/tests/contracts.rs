use device_configuration::*;
fn caps() -> ProductCapabilities<'static> {
    ProductCapabilities::NONE
}
#[test]
fn unknown_is_never_accepted() {
    assert_eq!(
        validate_property(0xff, 1, &[], &caps()),
        Err(Error::Unknown)
    );
    assert_eq!(validate_object(0xff, 1, &[], &caps()), Err(Error::Unknown));
}
#[cfg(feature = "time")]
#[test]
fn timezone_literal_and_strict_tail() {
    let b = [0xe0, 1, 0x10, 0xff, 3, 0, b'L', b'o', b'n'];
    let v = decode_property(1, 1, &b, &caps()).unwrap();
    assert!(matches!(
        v,
        Value::Timezones(Timezones {
            offset_min: 480,
            second_offset_min: -240,
            second_city: "Lon"
        })
    ));
    let mut out = [0u8; 9];
    assert_eq!(encode_property(1, 1, v, &mut out, &caps()), Ok(9));
    assert_eq!(out, b);
    assert_eq!(
        validate_property(1, 1, &b[..8], &caps()),
        Err(Error::Invalid)
    );
    assert_eq!(
        validate_property(1, 1, &[0, 0, 0, 0, 1, 0, 0], &caps()),
        Err(Error::Invalid)
    );
}
#[cfg(feature = "device")]
#[test]
fn frozen_identity_and_readonly_validation() {
    let b = [0x78, 0x56, 0x34, 0x12, 2, 0, 3, 0, 2, 0, b'S', b'N'];
    assert_eq!(validate_property(2, 2, &b, &caps()), Err(Error::Readonly));
    let v = decode_property(2, 2, &b, &caps()).unwrap();
    assert!(matches!(
        v,
        Value::Identity(Identity {
            product: 0x12345678,
            subtype: 2,
            group: 3,
            serial: "SN"
        })
    ));
    assert_eq!(
        validate_property_value(2, 2, &b, &caps())
            .unwrap()
            .max_length,
        65545
    );
    assert_eq!(
        validate_property_value(2, 2, &[0; 9], &caps()),
        Err(Error::Invalid)
    );
    assert_eq!(
        validate_property_value(2, 3, &[1, 0, b'v', 2, 1], &caps()),
        Err(Error::Invalid)
    );
    assert!(
        validate_property_value(2, 4, &[1, 0, b'h', 3, 40, 1, 40, 1, 1, 0, b'a'], &caps()).is_ok()
    );
    assert_eq!(
        validate_property_value(2, 4, &[0, 0, 4, 0, 0, 0, 0, 0, 0], &caps()),
        Err(Error::Invalid)
    );
}
#[cfg(feature = "device")]
#[test]
fn battery_reset_address_validity_is_explicit() {
    assert!(validate_property_value(2, 1, &[100, 0, 0, 0, 0, 0, 0, 1], &caps()).is_ok());
    for b in [
        [101, 0, 0, 0, 0, 0, 0, 1],
        [0, 1, 0, 0, 0, 0, 0, 0],
        [0, 0, 0, 0, 0, 0, 0, 16],
    ] {
        assert_eq!(
            validate_property_value(2, 1, &b, &caps()),
            Err(Error::Invalid)
        );
    }
    assert!(validate_property_value(2, 6, &[0; 15], &caps()).is_ok());
    let mut bad = [0; 15];
    bad[0] = 7;
    bad[14] = 1;
    assert_eq!(
        validate_property_value(2, 6, &bad, &caps()),
        Err(Error::Invalid)
    );
    assert!(validate_property_value(2, 7, &[2, 1, 2, 3, 4, 5, 6, 1], &caps()).is_ok());
    assert_eq!(
        validate_property_value(2, 7, &[0, 1, 0, 0, 0, 0, 0, 0], &caps()),
        Err(Error::Invalid)
    );
}
#[cfg(feature = "settings")]
#[test]
fn settings_require_real_limits_and_disabled_window_preserves_values() {
    assert!(validate_property(4, 4, &[0, 0, 0xe0, 1, 0xb0, 4], &caps()).is_ok());
    assert!(validate_property(4, 8, &[1, 0, 0, 0, 0, 0], &caps()).is_ok());
    assert_eq!(
        validate_property(4, 4, &[0, 128, 0, 0, 0, 0], &caps()),
        Err(Error::Invalid)
    );
    assert_eq!(
        validate_property(4, 6, &[10, 0, 0, 0, 20, 0, 0, 0], &caps()),
        Err(Error::Unavailable)
    );
    assert_eq!(
        validate_property(4, 7, &[2, 0, b'e', b'n'], &caps()),
        Err(Error::Unavailable)
    );
    let c = ProductCapabilities {
        languages: Some(&["en"]),
        saver_levels: Some(&[0, 250, 1000]),
        normal_levels: Some(&[0, 500, 1000]),
        display_ms: Some(Range {
            min: 10,
            max: 10000,
        }),
        ..caps()
    };
    assert!(validate_property(4, 7, &[2, 0, b'e', b'n'], &c).is_ok());
    assert_eq!(
        validate_property(4, 7, &[2, 0, b'z', b'h'], &c),
        Err(Error::Unsupported)
    );
    assert!(validate_property(4, 9, &[250, 0], &c).is_ok());
    assert_eq!(
        validate_property(4, 9, &[251, 0], &c),
        Err(Error::Unsupported)
    );
    assert_eq!(validate_property(4, 9, &[0xe9, 3], &c), Err(Error::Invalid));
    assert!(validate_property(4, 6, &[10, 0, 0, 0, 20, 0, 0, 0], &c).is_ok());
    assert_eq!(
        validate_property(4, 6, &[9, 0, 0, 0, 20, 0, 0, 0], &c),
        Err(Error::Invalid)
    );
    assert_eq!(
        validate_property_value(4, 11, &[2, 0, 0, 0, 0, 0], &c),
        Err(Error::Invalid)
    );
    assert!(validate_property_value(4, 11, &[2, 0, 0, 0xe8, 3, 1, 0xf4, 1], &c).is_ok());
}
#[cfg(feature = "settings")]
#[test]
fn window_objects_use_real_dictionary() {
    let b = [1, 0, 255, 255, 1, 2, 0];
    assert_eq!(validate_object(4, 1, &b, &caps()), Err(Error::Unavailable));
    let c = ProductCapabilities {
        windows: Some(&[WindowCapability {
            id: 1,
            parent: 65535,
        }]),
        ..caps()
    };
    assert!(validate_object(4, 1, &b, &c).is_ok());
    assert_eq!(
        validate_object(4, 1, &[2, 0, 255, 255, 1, 2, 0], &c),
        Err(Error::Unsupported)
    );
    assert_eq!(
        validate_object(4, 1, &[1, 0, 1, 0, 1, 2, 0], &c),
        Err(Error::Invalid)
    );
}
#[cfg(feature = "find")]
#[test]
fn find_policy_is_three_frozen_bytes() {
    assert!(validate_property(6, 1, &[0, 10, 0], &caps()).is_ok());
    assert_eq!(
        validate_property(6, 1, &[2, 10, 0], &caps()),
        Err(Error::Invalid)
    );
    assert_eq!(
        validate_property(6, 1, &[1, 10, 0, 0], &caps()),
        Err(Error::Invalid)
    );
}
#[cfg(feature = "telemetry")]
#[test]
fn telemetry_only_disabled_candidate_is_accepted() {
    assert!(validate_property(9, 1, &[0; 6], &caps()).is_ok());
    assert_eq!(
        validate_property(9, 1, &[0, 1, 0, 0, 0, 0], &caps()),
        Err(Error::Invalid)
    );
    assert_eq!(
        validate_property(9, 1, &[1, 0, 0, 0, 0, 0], &caps()),
        Err(Error::Unsupported)
    );
}
#[cfg(feature = "health-policy")]
#[test]
fn health_policy_units_modes_and_frozen_shapes() {
    let c = ProductCapabilities {
        health: Some(&[
            HealthCapability {
                module: 0x20,
                interval_s: Range { min: 1, max: 86400 },
                temperature_modes: 0,
            },
            HealthCapability {
                module: 0x26,
                interval_s: Range {
                    min: 10,
                    max: 86400,
                },
                temperature_modes: 0,
            },
            HealthCapability {
                module: 0x28,
                interval_s: Range { min: 1, max: 86400 },
                temperature_modes: 0b1000,
            },
        ]),
        ..caps()
    };
    assert_eq!(
        validate_property(0x20, 1, &[1, 1, 60, 0, 0, 0, 0], &caps()),
        Err(Error::Unavailable)
    );
    assert!(validate_property(0x20, 1, &[1, 1, 60, 0, 0, 0, 0], &c).is_ok());
    assert_eq!(
        validate_property(0x20, 1, &[1, 0, 60, 0, 0, 0, 0], &c),
        Err(Error::Invalid)
    );
    assert!(validate_property(0x26, 1, &[1, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0], &c).is_ok());
    assert_eq!(
        validate_property(0x26, 2, &[1, 0x11, 0x27], &c),
        Err(Error::Invalid)
    );
    assert!(validate_property(0x28, 1, &[1, 60, 0, 0, 0, 4], &c).is_ok());
    assert_eq!(
        validate_property(0x28, 1, &[1, 60, 0, 0, 0, 3], &c),
        Err(Error::Unsupported)
    );
    assert!(validate_property(0x28, 3, &[0, 0x42, 0x0e], &caps()).is_ok());
    assert_eq!(
        validate_property(0x28, 3, &[0, 0x42, 0x0e, 4], &caps()),
        Err(Error::Invalid)
    );
    assert!(validate_property(0x25, 3, &[1, 120, 0, 80, 0], &caps()).is_ok());
}
#[cfg(feature = "health-policy")]
#[test]
fn zones_glucose_phone_import_and_algorithm_directory() {
    let z = [
        23, 0, 1, 60, 0, 180, 0, 90, 0, 108, 0, 126, 0, 144, 0, 162, 0,
    ];
    assert!(validate_property(0x20, 3, &z, &caps()).is_ok());
    let mut bad = z;
    bad[9] = 90;
    assert_eq!(
        validate_property(0x20, 3, &bad, &caps()),
        Err(Error::Invalid)
    );
    let g = [0x1f, 100, 0, 200, 0, 0xe0, 1, 0xd0, 2, 0x38, 4];
    assert!(validate_property(0x27, 1, &g, &caps()).is_ok());
    assert_eq!(
        validate_property(0x27, 1, &[0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0], &caps()),
        Err(Error::Invalid)
    );
    assert!(validate_property(0x2a, 3, &[2, 0, 0, 0, 0, 0, 0, 0], &caps()).is_ok());
    assert_eq!(
        validate_property(0x2a, 3, &[2, 0, 0, 0, 0, 0, 0, 1], &caps()),
        Err(Error::Invalid)
    );
    assert_eq!(
        validate_property(0x2a, 3, &[1, 1, 0, 0, 0, 0, 0, 1], &caps()),
        Err(Error::Unavailable)
    );
    let c = ProductCapabilities {
        mood_values: Some(&[-1, 1]),
        ..caps()
    };
    assert!(validate_property(0x2a, 3, &[1, 1, 0, 4, 0, 100, 0, 7], &c).is_ok());
    assert_eq!(
        validate_property(0x2a, 3, &[1, 1, 0, 5, 0, 100, 0, 7], &c),
        Err(Error::Invalid)
    );
    assert!(validate_property_value(0x2a, 2, &[0], &caps()).is_ok());
    assert_eq!(
        validate_property_value(0x2a, 2, &[0, 1], &caps()),
        Err(Error::Invalid)
    );
}
#[cfg(feature = "hid")]
#[test]
fn hid_exact_mapping_and_real_actions() {
    let b = [1, 2, 2, 1, 4, 2, 5];
    assert_eq!(
        validate_object(0x37, 1, &b, &caps()),
        Err(Error::Unavailable)
    );
    let c = ProductCapabilities {
        hid: Some(&[
            HidBinding {
                platform: 2,
                gesture: 1,
                action: 4,
            },
            HidBinding {
                platform: 2,
                gesture: 2,
                action: 5,
            },
        ]),
        ..caps()
    };
    assert!(validate_object(0x37, 1, &b, &c).is_ok());
    assert_eq!(
        validate_object(0x37, 1, &[1, 2, 2, 1, 4, 1, 4], &c),
        Err(Error::Invalid)
    );
    assert_eq!(
        validate_object(0x37, 1, &[1, 2, 1, 3, 4], &c),
        Err(Error::Unsupported)
    );
    assert_eq!(validate_object(0x37, 1, &b[..6], &c), Err(Error::Invalid));
}
#[cfg(feature = "antplus")]
#[test]
fn antplus_schema2_separates_command_from_actual_state() {
    assert_eq!(
        validate_property(0x38, 1, &[1], &caps()),
        Err(Error::Unavailable)
    );
    let c = ProductCapabilities {
        antplus_backend: true,
        ..caps()
    };
    assert!(validate_property(0x38, 1, &[1], &c).is_ok());
    assert_eq!(validate_property(0x38, 1, &[1, 4], &c), Err(Error::Invalid));
    assert_eq!(validate_property(0x38, 2, &[4], &c), Err(Error::Readonly));
    assert!(validate_property_value(0x38, 2, &[4], &c).is_ok());
    assert_eq!(
        validate_property_value(0x38, 2, &[7], &c),
        Err(Error::Invalid)
    );
    assert_eq!(property_descriptor(0x38, 1).unwrap().revision, 2);
}
#[cfg(feature = "engineering")]
#[test]
fn engineering_profile_native_ranges_and_typed_log() {
    assert_eq!(
        validate_property(0x72, 1, &[1], &caps()),
        Err(Error::Unavailable)
    );
    let c = ProductCapabilities {
        engineering: true,
        amplitude: Some(Range { min: 10, max: 100 }),
        tap_interval_ms: Some(Range { min: 5, max: 1000 }),
        wear_counts: Some(Range { min: 1, max: 400 }),
        diagnostics: Some(&[DiagnosticCapability {
            id: 2,
            reason: 3,
            parameter_length: 2,
            revision: 1,
            validate_parameters: |_| true,
        }]),
        ..caps()
    };
    assert!(validate_property(0x72, 1, &[1], &c).is_ok());
    assert_eq!(validate_property(0x72, 2, &[0, 1], &c), Err(Error::Invalid));
    assert!(validate_property(0x72, 3, &[10, 0, 0, 0, 5, 0, 0, 0], &c).is_ok());
    assert_eq!(validate_property(0x72, 4, &[0, 0], &c), Err(Error::Invalid));
    let mut b = [0; 29];
    b[0] = 1;
    b[19] = 2;
    b[21] = 3;
    b[23] = 2;
    b[25] = 0xaa;
    b[26] = 0xbb;
    b[27] = 1;
    assert!(validate_object_value(0x72, 1, &b, &c).is_ok());
    assert_eq!(validate_object(0x72, 1, &b, &c), Err(Error::Readonly));
    b[18] = 2;
    assert_eq!(validate_object_value(0x72, 1, &b, &c), Err(Error::Invalid));
    b[18] = 0;
    b[10] = 1;
    assert_eq!(validate_object_value(0x72, 1, &b, &c), Err(Error::Invalid));
}
#[cfg(feature = "profile")]
#[test]
fn profile_uses_existing_models_and_rejects_reserved_mask() {
    let mut b = [0u8; 14];
    assert!(validate_property(3, 1, &b, &caps()).is_ok());
    b[13] = 128;
    assert_eq!(validate_property(3, 1, &b, &caps()), Err(Error::Invalid));
    assert!(validate_property(3, 2, &[0; 18], &caps()).is_ok());
    assert_eq!(
        validate_property(3, 2, &[0; 19], &caps()),
        Err(Error::Invalid)
    );
}
#[cfg(feature = "sleep")]
#[test]
fn sleep_readback_uses_real_typed_detection_and_frozen_window() {
    assert!(validate_property(0x21, 1, &[0, 0, 60, 0, 120, 0], &caps()).is_ok());
    assert!(validate_property_value(0x21, 2, &[0; 18], &caps()).is_ok());
    let mut b = [0; 18];
    b[0] = 6;
    assert_eq!(
        validate_property_value(0x21, 2, &b, &caps()),
        Err(Error::Invalid)
    );
    assert_eq!(
        validate_property(0x21, 2, &[0; 18], &caps()),
        Err(Error::Readonly)
    );
}
#[cfg(feature = "sport")]
#[test]
fn automatic_sport_requires_declared_classifier_not_a_step_guess() {
    assert_eq!(property_descriptor(0x22, 1).unwrap().revision, 2);
    assert_eq!(
        validate_property(0x22, 1, &[1, 30, 0, 0, 0, 2, 0, 1], &caps()),
        Err(Error::Unavailable)
    );
    let c = ProductCapabilities {
        automatic_sports: Some(&[2]),
        auto_sport_delay_s: Some(Range { min: 10, max: 60 }),
        ..caps()
    };
    assert!(validate_property(0x22, 1, &[1, 30, 0, 0, 0, 2, 0, 1], &c).is_ok());
    assert_eq!(
        validate_property(0x22, 1, &[1, 30, 0, 0, 0, 2, 0, 2], &c),
        Err(Error::Invalid)
    );
    assert_eq!(
        validate_property(0x22, 1, &[1, 30, 0, 0, 0, 3, 0, 1], &c),
        Err(Error::Unsupported)
    );
}
#[cfg(feature = "activity")]
#[test]
fn activity_segmentation_uses_manifest_range() {
    assert_eq!(
        validate_property(0x23, 1, &[60, 0, 0, 0], &caps()),
        Err(Error::Unavailable)
    );
    let c = ProductCapabilities {
        activity_segment_s: Some(Range { min: 30, max: 300 }),
        ..caps()
    };
    assert!(validate_property(0x23, 1, &[60, 0, 0, 0], &c).is_ok());
    assert_eq!(
        validate_property(0x23, 1, &[0, 0, 0, 0], &c),
        Err(Error::Invalid)
    );
}
#[cfg(feature = "womens-health")]
#[test]
fn womens_policy_delegates_mutually_exclusive_configuration() {
    assert!(validate_property(0x2d, 1, &[0, 2, 0, 0, 0, 0, 0, 0, 0, 0], &caps()).is_ok());
    assert_eq!(
        validate_property(0x2d, 1, &[3, 0, 0, 0, 0, 0, 0, 0, 0, 0], &caps()),
        Err(Error::Invalid)
    );
}
#[cfg(feature = "uv")]
#[test]
fn uv_policy_reuses_candidate_validator() {
    assert!(validate_property(0x2e, 1, &[0; 8], &caps()).is_ok());
    assert_eq!(
        validate_property(0x2e, 1, &[2, 0, 0, 0, 0, 0, 0, 0], &caps()),
        Err(Error::Invalid)
    );
}
#[cfg(feature = "reminders")]
#[test]
fn reminder_object_kind_and_repeat_validation_remain_in_models() {
    let mut b = [0; 34]; // 32 fixed bytes plus empty Str prefix.
    assert!(validate_object(5, 1, &b, &caps()).is_ok());
    b[1] = 3;
    assert_eq!(validate_object(5, 1, &b, &caps()), Err(Error::Invalid));
    assert!(validate_property(5, 1, &[0, 0, 60, 0, 120, 0], &caps()).is_ok());
}
#[cfg(feature = "extension")]
#[test]
fn frozen_extension_is_explicit_bounded_bytestring() {
    assert!(validate_property(0x11, 1, &[2, 0, 1, 0], &caps()).is_ok());
    assert_eq!(
        validate_property(0x11, 1, &[1, 0, 1, 2], &caps()),
        Err(Error::Invalid)
    );
    assert_eq!(
        validate_property(0x11, 1, &[65, 0], &caps()),
        Err(Error::Invalid)
    );
}
#[cfg(feature = "radiation")]
#[test]
fn radiation_threshold_is_volts_per_metre_not_a_measurement() {
    assert!(validate_property(0x39, 1, &[0xe8, 3], &caps()).is_ok());
    assert_eq!(
        validate_property(0x39, 1, &[0xe9, 3], &caps()),
        Err(Error::Invalid)
    );
}
#[test]
fn catalog_has_every_frozen_property_object_route_and_no_reserved40() {
    assert_eq!(
        catalog().filter(|e| e.kind == EntryKind::Property).count(),
        62
    );
    assert_eq!(
        catalog().filter(|e| e.kind == EntryKind::Object).count(),
        15
    );
    assert!(!catalog().any(|e| e.module == Some(0x40)));
    assert!(catalog().all(|e| !e.owner.is_empty() && !e.feature.is_empty()));
}
#[cfg(feature = "external-content")]
#[test]
fn external_provider_is_explicit_and_cannot_override_builtin_validation() {
    struct Content;
    impl SchemaProvider for Content {
        fn property_descriptor(&self, m: u8, k: u16) -> Option<SchemaDescriptor> {
            (m == 0x30 && k == 1).then_some(SchemaDescriptor {
                module: m,
                key: k,
                schema_type: SchemaType::Boolean,
                revision: 1,
                max_length: 1,
                access: Access::ReadWrite,
            })
        }
        fn validate_property_value(
            &self,
            _: u8,
            _: u16,
            b: &[u8],
            _: &ProductCapabilities<'_>,
        ) -> Result<(), Error> {
            if b == [0] || b == [1] {
                Ok(())
            } else {
                Err(Error::Invalid)
            }
        }
    }
    assert!(validate_property_with_provider(0x30, 1, &[1], &caps(), &Content).is_ok());
    assert_eq!(
        validate_property_with_provider(0x30, 1, &[2], &caps(), &Content),
        Err(Error::Invalid)
    );
    assert_eq!(
        validate_property_with_provider(0x30, 1, &[1, 0], &caps(), &Content),
        Err(Error::Invalid)
    );
    assert_eq!(
        validate_property_with_provider(0x31, 1, &[], &caps(), &Content),
        Err(Error::Unsupported)
    );
    #[cfg(feature = "settings")]
    assert_eq!(
        validate_property_with_provider(4, 1, &[2], &caps(), &Content),
        Err(Error::Invalid)
    );
}
#[cfg(feature = "health-policy")]
#[test]
fn algorithm_directory_is_a_real_typed_body_with_unique_identity() {
    // RMSSD device algorithm r/v, milliseconds exp0, no categorical labels.
    let b = [1, 4, 0x2a, 1, 1, 0, b'r', 1, 0, b'v', 4, 0, 0];
    let value = decode_property(0x2a, 2, &b, &caps()).unwrap();
    if let Value::AlgorithmDirectory(d) = value {
        let e = d.entries().next().unwrap();
        assert_eq!(e.record_type, 0x2a04);
        assert_eq!(e.algorithm_id, "r");
        assert_eq!(e.source, 1);
    } else {
        panic!("wrong DTO")
    }
    let mut out = [0; 13];
    assert_eq!(encode_property(0x2a, 2, value, &mut out, &caps()), Ok(13));
    assert_eq!(out, b);
    let mut bad = b;
    bad[3] = 3;
    assert_eq!(
        validate_property_value(0x2a, 2, &bad, &caps()),
        Err(Error::Invalid)
    );
    let mut duplicate = vec![2];
    duplicate.extend_from_slice(&b[1..]);
    duplicate.extend_from_slice(&b[1..]);
    assert_eq!(
        validate_property_value(0x2a, 2, &duplicate, &caps()),
        Err(Error::Invalid)
    );
}
#[cfg(feature = "engineering")]
#[test]
fn log_parameters_require_typed_product_validator_not_only_length() {
    let c = ProductCapabilities {
        engineering: true,
        diagnostics: Some(&[DiagnosticCapability {
            id: 2,
            reason: 3,
            parameter_length: 2,
            revision: 1,
            validate_parameters: |b| b == [0xaa, 0xbb],
        }]),
        ..caps()
    };
    let mut b = [0; 29];
    b[0] = 1;
    b[19] = 2;
    b[21] = 3;
    b[23] = 2;
    b[25] = 0xaa;
    b[26] = 0xbb;
    assert!(validate_object_value(0x72, 1, &b, &c).is_ok());
    b[25] = 0xcc;
    assert_eq!(validate_object_value(0x72, 1, &b, &c), Err(Error::Invalid));
}
#[cfg(feature = "hid")]
#[test]
fn frozen_hid_dictionary_cannot_be_extended_by_product_capability() {
    let c = ProductCapabilities {
        hid: Some(&[HidBinding {
            platform: 1,
            gesture: 8,
            action: 11,
        }]),
        ..caps()
    };
    assert_eq!(
        validate_object(0x37, 1, &[1, 1, 1, 8, 11], &c),
        Err(Error::Invalid)
    );
}
#[cfg(feature = "settings")]
#[test]
fn reserved_window_ids_and_sentinel_cannot_be_real_windows() {
    for id in [26u16, 32767, 65535] {
        let c = ProductCapabilities {
            windows: Some(&[WindowCapability { id, parent: 65535 }]),
            ..caps()
        };
        let b = [id as u8, (id >> 8) as u8, 255, 255, 1, 0, 0];
        assert_eq!(validate_object(4, 1, &b, &c), Err(Error::Invalid));
    }
}
#[cfg(all(feature = "limits", feature = "device"))]
#[test]
fn common_limits_stay_exactly_eight_readonly_bytes() {
    let b = [20, 0, 0, 16, 0, 0, 128, 0];
    assert_eq!(
        validate_property(2, 0xf000, &b, &caps()),
        Err(Error::Readonly)
    );
    let v = decode_property(2, 0xf000, &b, &caps()).unwrap();
    assert!(matches!(
        v,
        Value::ModuleLimits(ModuleLimits {
            max_objects: 20,
            max_object_bytes: 4096,
            max_string_bytes: 128
        })
    ));
    assert_eq!(
        validate_property_value(2, 0xf000, &b[..7], &caps()),
        Err(Error::Invalid)
    );
}
#[cfg(all(
    feature = "device",
    feature = "settings",
    feature = "health-policy",
    feature = "engineering",
    feature = "find",
    feature = "telemetry",
    feature = "antplus"
))]
#[test]
fn every_new_core_dto_roundtrips_independent_literals_and_rejects_extra_tail() {
    let c = ProductCapabilities {
        languages: Some(&["en"]),
        saver_levels: Some(&[250]),
        normal_levels: Some(&[500]),
        display_ms: Some(Range { min: 1, max: 60000 }),
        health: Some(&[
            HealthCapability {
                module: 0x20,
                interval_s: Range { min: 1, max: 600 },
                temperature_modes: 0,
            },
            HealthCapability {
                module: 0x26,
                interval_s: Range { min: 1, max: 600 },
                temperature_modes: 0,
            },
            HealthCapability {
                module: 0x28,
                interval_s: Range { min: 1, max: 600 },
                temperature_modes: 8,
            },
        ]),
        engineering: true,
        antplus_backend: true,
        rmssd_allowed: true,
        amplitude: Some(Range { min: 1, max: 100 }),
        tap_interval_ms: Some(Range { min: 1, max: 100 }),
        wear_counts: Some(Range { min: 1, max: 100 }),
        ..caps()
    };
    let fixtures: &[(u8, u16, &[u8])] = &[
        (2, 1, &[80, 0, 0, 0x78, 0x56, 0x34, 0x12, 15]),
        (2, 2, &[1, 2, 3, 4, 5, 0, 6, 0, 1, 0, b's']),
        (2, 3, &[1, 0, b'v', 1, 0]),
        (2, 4, &[1, 0, b'h', 3, 40, 1, 40, 1, 1, 0, b'a']),
        (2, 5, &[1, 0, b'n']),
        (
            2,
            6,
            &[5, 0, 1, 2, 3, 4, 0x80, 0x96, 0x98, 0, 0, 0, 0, 0, 7],
        ),
        (2, 7, &[1, 1, 2, 3, 4, 5, 6, 1]),
        (4, 1, &[1]),
        (4, 2, &[1, 0]),
        (4, 3, &[1]),
        (4, 4, &[0, 0, 60, 0, 120, 0]),
        (4, 5, &[0]),
        (4, 6, &[0x10, 0x27, 0, 0, 0x88, 0x13, 0, 0]),
        (4, 7, &[2, 0, b'e', b'n']),
        (4, 8, &[1, 127, 0, 0, 0, 0]),
        (4, 9, &[250, 0]),
        (4, 10, &[0xf4, 1]),
        (4, 11, &[1, 250, 0, 1, 0xf4, 1]),
        (6, 1, &[1, 10, 0]),
        (9, 1, &[0; 6]),
        (0x20, 1, &[1, 2, 60, 0, 0, 0, 1]),
        (0x20, 2, &[1, 0x78, 0]),
        (0x25, 2, &[0, 0x8c, 0]),
        (0x25, 3, &[0, 120, 0, 80, 0]),
        (0x26, 1, &[0, 60, 0, 0, 0, 0, 0, 60, 0, 120, 0]),
        (0x26, 2, &[1, 0x28, 0x23]),
        (0x27, 1, &[0x1c, 0, 0, 0, 0, 0xe0, 1, 0xd0, 2, 0x38, 4]),
        (0x28, 1, &[1, 60, 0, 0, 0, 4]),
        (0x28, 2, &[1, 0xd8, 0x0e, 4]),
        (0x28, 3, &[0, 0x42, 0x0e]),
        (0x2a, 1, &[1]),
        (0x2a, 2, &[0]),
        (0x2a, 3, &[1, 0, 0, 4, 0, 100, 0, 6]),
        (0x38, 1, &[1]),
        (0x38, 2, &[6]),
        (0x72, 1, &[1]),
        (0x72, 2, &[1, 3]),
        (0x72, 3, &[10, 0, 0, 0, 20, 0, 0, 0]),
        (0x72, 4, &[30, 0]),
    ];
    for &(m, k, b) in fixtures {
        let value = decode_property(m, k, b, &c).unwrap();
        let mut out = [0; 256];
        let n = encode_property(m, k, value, &mut out, &c).unwrap();
        assert_eq!(&out[..n], b, "{m:02x}.{k:04x}");
        let mut bad = b.to_vec();
        bad.push(0);
        assert_eq!(
            validate_property_value(m, k, &bad, &c),
            Err(Error::Invalid),
            "{m:02x}.{k:04x}"
        );
        let mut short = vec![0; b.len() - 1];
        assert_eq!(
            encode_property(m, k, value, &mut short, &c),
            Err(Error::Capacity),
            "{m:02x}.{k:04x}"
        );
    }
}
#[cfg(feature = "health-policy")]
#[test]
fn typed_directory_writer_preserves_label_order_and_rejects_duplicates() {
    let labels = [
        Label {
            value: 2,
            text: "up",
        },
        Label {
            value: -1,
            text: "down",
        },
    ];
    let entries = [AlgorithmDefinition {
        record_type: 0x2a03,
        source: 2,
        algorithm_id: "m",
        version: "v",
        unit: 3,
        scale_exp: 0,
        labels: &labels,
    }];
    let expected = [
        1, 3, 0x2a, 2, 1, 0, b'm', 1, 0, b'v', 3, 0, 2, 2, 0, 0, 0, 2, 0, b'u', b'p', 255, 255,
        255, 255, 4, 0, b'd', b'o', b'w', b'n',
    ];
    let mut b = [0; 31];
    assert_eq!(AlgorithmDirectory::encode_entries(&entries, &mut b), Ok(31));
    assert_eq!(b, expected);
    let d = AlgorithmDirectory::decode(&b).unwrap();
    let e = d.entries().next().unwrap();
    let got: Vec<_> = e.labels.entries().collect();
    assert_eq!(got, vec![(2, "up"), (-1, "down")]);
    let duplicates = [
        Label {
            value: 1,
            text: "a",
        },
        Label {
            value: 1,
            text: "b",
        },
    ];
    let e = [AlgorithmDefinition {
        labels: &duplicates,
        ..entries[0]
    }];
    assert_eq!(
        AlgorithmDirectory::encode_entries(&e, &mut b),
        Err(Error::Invalid)
    );
}
#[cfg(feature = "settings")]
#[test]
fn typed_brightness_writer_checks_actual_discrete_order() {
    let mut b = [0; 8];
    assert_eq!(
        BrightnessLimits::encode_levels(&[0, 1000], &[500], &mut b),
        Ok(8)
    );
    assert_eq!(b, [2, 0, 0, 232, 3, 1, 244, 1]);
    assert_eq!(
        BrightnessLimits::encode_levels(&[2, 1], &[], &mut b),
        Err(Error::Invalid)
    );
}
#[cfg(feature = "hid")]
#[test]
fn typed_hid_writer_uses_the_frozen_numeric_dictionary() {
    assert_eq!(object_descriptor(0x37, 1).unwrap().max_length, 19);
    let mut b = [0; 7];
    assert_eq!(
        HidMappings::encode_mappings(
            true,
            2,
            &[
                Mapping {
                    gesture: 0,
                    action: 0
                },
                Mapping {
                    gesture: 7,
                    action: 10
                }
            ],
            &mut b
        ),
        Ok(7)
    );
    assert_eq!(b, [1, 2, 2, 0, 0, 7, 10]);
}
#[cfg(feature = "engineering")]
#[test]
fn typed_log_writer_has_independent_little_endian_expectation() {
    let entries = [DiagnosticEntry {
        mono_us: 0x0102030405060708,
        utc_ms: 0,
        time_valid: false,
        diagnostic_id: 2,
        reason: 3,
        parameters: &[0xaa, 0xbb],
        end_marker: 2,
        error_state: 1,
    }];
    let mut b = [0; 29];
    assert_eq!(DiagnosticLog::encode_entries(&entries, &mut b), Ok(29));
    assert_eq!(
        b,
        [
            1, 0, 8, 7, 6, 5, 4, 3, 2, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 3, 0, 2, 0, 0xaa, 0xbb,
            2, 1
        ]
    );
}
#[test]
fn compiled_registry_rejects_malformed_inputs_without_panics() {
    let mut seed = 0x12345678u32;
    for entry in catalog() {
        let Some(module) = entry.module else { continue };
        let descriptor = match entry.kind {
            EntryKind::Property => property_descriptor(module, entry.key),
            EntryKind::Object => object_descriptor(module, entry.key),
        };
        if descriptor.is_err() {
            continue;
        }
        for len in 0..80 {
            let mut b = [0; 80];
            for v in &mut b[..len] {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                *v = (seed >> 24) as u8;
            }
            let _ = match entry.kind {
                EntryKind::Property => {
                    validate_property_value(module, entry.key, &b[..len], &caps())
                }
                EntryKind::Object => validate_object_value(module, entry.key, &b[..len], &caps()),
            };
        }
    }
}
#[cfg(not(any(
    feature = "device",
    feature = "time",
    feature = "settings",
    feature = "find",
    feature = "telemetry",
    feature = "health-policy",
    feature = "heart-rate",
    feature = "blood-pressure",
    feature = "oxygen",
    feature = "glucose",
    feature = "temperature",
    feature = "wellbeing",
    feature = "hid",
    feature = "antplus",
    feature = "engineering",
    feature = "profile",
    feature = "sleep",
    feature = "sport",
    feature = "activity",
    feature = "womens-health",
    feature = "uv",
    feature = "reminders",
    feature = "extension",
    feature = "radiation"
)))]
#[test]
fn minimal_build_registers_no_builtin_business_schemas() {
    for e in catalog() {
        let Some(m) = e.module else { continue };
        assert_eq!(
            match e.kind {
                EntryKind::Property => property_descriptor(m, e.key),
                EntryKind::Object => object_descriptor(m, e.key),
            },
            Err(Error::Unknown)
        );
    }
}
#[cfg(all(
    feature = "time",
    feature = "settings",
    feature = "health-policy",
    feature = "reminders"
))]
#[test]
fn explicit_product_limits_bound_strings_and_objects_even_when_wire_format_allows_more() {
    let c = ProductCapabilities {
        limits: Some(ModuleLimits {
            max_objects: 1,
            max_object_bytes: 34,
            max_string_bytes: 1,
        }),
        ..caps()
    };
    assert_eq!(
        validate_property(1, 1, &[0, 0, 0, 0, 2, 0, b'A', b'B'], &c),
        Err(Error::Invalid)
    );
    assert_eq!(validate_object(5, 1, &[0; 35], &c), Err(Error::Invalid));
    let b = [1, 4, 0x2a, 1, 2, 0, b'r', b'r', 1, 0, b'v', 4, 0, 0];
    assert_eq!(
        validate_property_value(0x2a, 2, &b, &c),
        Err(Error::Invalid)
    );
}
#[cfg(feature = "health-policy")]
#[test]
fn frozen_heart_policy_interval_has_only_the_declared_product_range() {
    let c = ProductCapabilities {
        health: Some(&[HealthCapability {
            module: 0x20,
            interval_s: Range { min: 0, max: 60 },
            temperature_modes: 0,
        }]),
        ..caps()
    };
    assert!(validate_property(0x20, 1, &[1, 2, 0, 0, 0, 0, 0], &c).is_ok());
}
#[cfg(feature = "external-content")]
#[test]
fn external_object_provider_cannot_bypass_f000_capacity() {
    struct Content;
    impl SchemaProvider for Content {
        fn object_descriptor(&self, m: u8, k: u16) -> Option<SchemaDescriptor> {
            (m == 0x34 && k == 1).then_some(SchemaDescriptor {
                module: m,
                key: k,
                schema_type: SchemaType::Object,
                revision: 1,
                max_length: 4,
                access: Access::ReadWrite,
            })
        }
        fn validate_object_value(
            &self,
            _: u8,
            _: u16,
            _: &[u8],
            _: &ProductCapabilities<'_>,
        ) -> Result<(), Error> {
            Ok(())
        }
    }
    let c = ProductCapabilities {
        limits: Some(ModuleLimits {
            max_objects: 1,
            max_object_bytes: 2,
            max_string_bytes: 1,
        }),
        ..caps()
    };
    assert_eq!(
        validate_object_with_provider(0x34, 1, &[0; 4], &c, &Content),
        Err(Error::Invalid)
    );
    let c = ProductCapabilities {
        limits: Some(ModuleLimits {
            max_objects: 0,
            max_object_bytes: 4,
            max_string_bytes: 1,
        }),
        ..caps()
    };
    assert_eq!(
        validate_object_value_with_provider(0x34, 1, &[0; 4], &c, &Content),
        Err(Error::Unsupported)
    );
}
