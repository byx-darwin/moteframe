//! Core session wire, authorization and lifecycle contract tests.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use device_protocol::{Frame, Header, HelloResponse, Kind, ModuleEntry, ProtocolLimits};
use device_services::{
    AccessSnapshot, AccessState, BuildProfile, Config, DeviceInfo, Session, SessionError,
    SessionState, TrustedAccessProvider,
};

const MODULES: [ModuleEntry; 1] = [ModuleEntry {
    module: 0,
    major: 1,
    minor: 2,
    availability: 1,
    features: 0,
    input_max: 128,
    output_max: 128,
}];
const HELLO: [u8; 8] = [1, 1, 128, 0, 0, 0, 3, 0];

fn config(modules: &[ModuleEntry]) -> Config<'_> {
    Config {
        session: 0x1234_5678,
        capability_rev: 0x0102_0304,
        profile: BuildProfile::Commercial,
        info: DeviceInfo {
            product_id: 0x8493,
            fw_major: 2,
            fw_minor: 2,
            fw_patch: 5,
            build_id: 0x1020_3040,
        },
        modules,
        limits: ProtocolLimits {
            rx_frame_max: 20,
            tx_frame_max: 20,
            control_max: 128,
            data_max: 0,
            idle_ms: 2000,
            fragment_ms: 100,
            response_ms: 10000,
        },
    }
}

fn request(module: u8, opcode: u8, message_id: u16, payload: &[u8]) -> Frame<'_> {
    Frame {
        header: Header {
            kind: Kind::Request,
            module,
            opcode,
            message_id,
            offset: 0,
            total_length: u16::try_from(payload.len()).unwrap(),
        },
        payload,
    }
}

fn active_session() -> Session<'static> {
    let mut session = Session::new(config(&MODULES)).unwrap();
    session.control_notifications_enabled().unwrap();
    let mut out = [0; 128];
    let length = session
        .dispatch(request(0, 1, 1, &HELLO), &mut out)
        .unwrap();
    assert_eq!(length, 22);
    assert_eq!(&out[..4], &[0, 0, 0, 0]);
    session
}

#[test]
fn hello_requires_notifications_then_negotiates_without_granting_access() {
    let mut session = Session::new(config(&MODULES)).unwrap();
    let mut out = [0xaa; 128];
    assert_eq!(
        session.dispatch(request(0, 1, 1, &HELLO), &mut out),
        Err(SessionError::NotificationsRequired)
    );
    assert_eq!(out, [0xaa; 128]);
    session.control_notifications_enabled().unwrap();
    let length = session
        .dispatch(request(0, 1, 1, &HELLO), &mut out)
        .unwrap();
    assert_eq!(length, 22);
    assert_eq!(
        &out[..length],
        &[
            0, 0, 0, 0, 1, 2, 0x78, 0x56, 0x34, 0x12, 128, 0, 0, 0, 1, 0, 1, 0, 4, 3, 2, 1
        ]
    );
    assert_eq!(session.state(), SessionState::Active);
    let hello = HelloResponse::decode(&out[4..length]).unwrap();
    assert_eq!(hello.access, 1);
    assert_eq!(hello.profile, 0);
}

#[test]
fn info_encodes_exact_fourteen_byte_success_data() {
    let mut session = active_session();
    let mut out = [0xa5; 128];
    let length = session.dispatch(request(0, 2, 2, &[]), &mut out).unwrap();
    assert_eq!(length, 18);
    assert_eq!(
        &out[..length],
        &[
            0, 0, 0, 0, 0x93, 0x84, 0, 0, 2, 0, 2, 0, 5, 0, 0x40, 0x30, 0x20, 0x10
        ]
    );
    assert_eq!(out[length], 0xa5);
    let info = DeviceInfo::decode(&out[4..length]).unwrap();
    assert_eq!(info, config(&MODULES).info);
    assert!(DeviceInfo::decode(&out[4..=length]).is_err());
    assert!(info.encode(&mut [0; 13]).is_err());
}

#[test]
fn limits_and_ping_are_real_replies_not_padded_capacity() {
    let mut session = active_session();
    let mut out = [0xa5; 128];
    let n = session.dispatch(request(0, 4, 2, &[]), &mut out).unwrap();
    assert_eq!(
        &out[..n],
        &[
            0, 0, 0, 0, 20, 0, 20, 0, 128, 0, 0, 0, 0xd0, 7, 100, 0, 0x10, 0x27
        ]
    );
    assert_eq!(out[n], 0xa5);
    let n = session
        .dispatch(request(0, 5, 3, &[0x78, 0x56, 0x34, 0x12]), &mut out)
        .unwrap();
    assert_eq!(&out[..n], &[0, 0, 0, 0, 0x78, 0x56, 0x34, 0x12]);
}

#[test]
fn protected_core_commands_never_fabricate_stream_or_dataset() {
    let mut session = active_session();
    let mut out = [0xaa; 128];
    for (id, opcode, body) in [(2, 7, &[1, 2, 0, 1, 0, 0, 0][..]), (3, 8, &[][..])] {
        let n = session
            .dispatch(request(0, opcode, id, body), &mut out)
            .unwrap();
        assert_eq!(&out[..n], &[5, 0, 3, 255]);
        assert_eq!(n, 4);
    }
    let n = session.dispatch(request(0, 6, 4, &[]), &mut out).unwrap();
    assert_eq!(&out[..n], &[0, 0, 0, 0, 1, 0, 0]);
}

#[derive(Debug)]
struct TrustedStatus;
impl TrustedAccessProvider for TrustedStatus {
    fn access_snapshot(&self) -> AccessSnapshot {
        AccessSnapshot {
            state: AccessState::Authorized,
            reason: 0x1234,
        }
    }
}

#[test]
fn trusted_status_is_reported_but_is_not_resource_authorization() {
    let mut session = active_session();
    let mut out = [0; 128];
    let n = session
        .dispatch_with_access(request(0, 6, 2, &[]), &TrustedStatus, &mut out)
        .unwrap();
    assert_eq!(&out[..n], &[0, 0, 0, 0, 2, 0x34, 0x12]);
    let n = session
        .dispatch_with_access(request(0, 8, 3, &[]), &TrustedStatus, &mut out)
        .unwrap();
    assert_eq!(&out[..n], &[5, 0, 3, 255]);
}

#[test]
fn last_identical_request_replays_identical_response_only() {
    let mut session = active_session();
    let mut first = [0; 128];
    let n = session.dispatch(request(0, 6, 2, &[]), &mut first).unwrap();
    let mut replay = [0xff; 128];
    let replay_n = session
        .dispatch_with_access(request(0, 6, 2, &[]), &TrustedStatus, &mut replay)
        .unwrap();
    assert_eq!(n, replay_n);
    assert_eq!(&first[..n], &replay[..n]);
    assert_eq!(&replay[..n], &[0, 0, 0, 0, 1, 0, 0]);
}

#[test]
fn same_id_different_payload_or_identity_faults_connection() {
    for alter_identity in [false, true] {
        let mut session = active_session();
        let mut out = [0; 128];
        session
            .dispatch(request(0, 5, 2, &[1, 2, 3, 4]), &mut out)
            .unwrap();
        let conflicting = if alter_identity {
            request(0, 2, 2, &[])
        } else {
            request(0, 5, 2, &[1, 2, 3, 5])
        };
        assert_eq!(
            session.dispatch(conflicting, &mut out),
            Err(SessionError::RequestConflict)
        );
        assert_eq!(session.state(), SessionState::Faulted);
        assert_eq!(
            session.dispatch(request(0, 6, 3, &[]), &mut out),
            Err(SessionError::Faulted)
        );
        assert!(session.control_notifications_enabled().is_err());
    }
}

#[test]
fn older_id_and_wrap_are_not_a_second_cache_entry() {
    let mut session = active_session();
    let mut out = [0; 128];
    session
        .dispatch(request(0, 2, u16::MAX, &[]), &mut out)
        .unwrap();
    assert_eq!(
        session.dispatch(request(0, 1, 1, &HELLO), &mut out),
        Err(SessionError::RequestOrder)
    );
    assert_eq!(session.state(), SessionState::Faulted);
}

#[test]
fn disabling_notifications_is_terminal_on_this_connection() {
    let mut session = active_session();
    session.control_notifications_disabled();
    assert_eq!(session.state(), SessionState::Closed);
    assert!(session.control_notifications_enabled().is_err());
    assert_eq!(
        session.dispatch(request(0, 1, 2, &HELLO), &mut [0; 128]),
        Err(SessionError::Closed)
    );
    let mut disconnected = active_session();
    disconnected.disconnect();
    assert_eq!(disconnected.state(), SessionState::Closed);
    assert!(disconnected.control_notifications_enabled().is_err());
}

#[test]
fn insufficient_output_does_not_consume_id_or_activate_hello() {
    let mut session = Session::new(config(&MODULES)).unwrap();
    session.control_notifications_enabled().unwrap();
    let mut short = [0xa5; 21];
    assert_eq!(
        session.dispatch(request(0, 1, 1, &HELLO), &mut short),
        Err(SessionError::Capacity)
    );
    assert_eq!(short, [0xa5; 21]);
    assert_eq!(session.state(), SessionState::AwaitingHello);
    assert_eq!(
        session.dispatch(request(0, 1, 1, &HELLO), &mut [0; 128]),
        Ok(22)
    );
}

#[test]
fn hello_version_errors_and_argument_errors_are_exact_status_only() {
    for (body, expected) in [
        (&[1, 1, 128, 0, 0, 0, 1, 0][..], [13, 0, 0, 0]),
        (&[2, 2, 128, 0, 0, 0, 3, 0][..], [13, 0, 0, 0]),
        (&[1, 1, 127, 0, 0, 0, 3, 0][..], [1, 0, 0, 0]),
        (&HELLO[..7], [1, 0, 0, 0]),
    ] {
        let mut session = Session::new(config(&MODULES)).unwrap();
        session.control_notifications_enabled().unwrap();
        let mut out = [0xaa; 128];
        let n = session.dispatch(request(0, 1, 1, body), &mut out).unwrap();
        assert_eq!(&out[..n], &expected);
        assert_eq!(n, 4);
        assert_eq!(out[n], 0xaa);
        assert_eq!(session.state(), SessionState::AwaitingHello);
        assert_eq!(session.dispatch(request(0, 1, 2, &HELLO), &mut out), Ok(22));
    }
}

#[test]
fn repeated_hello_is_not_a_new_session_or_access_grant() {
    let mut session = active_session();
    let mut out = [0; 128];
    let n = session
        .dispatch(request(0, 1, 2, &HELLO), &mut out)
        .unwrap();
    assert_eq!(&out[..n], &[4, 0, 0, 0]);
    let n = session.dispatch(request(0, 6, 3, &[]), &mut out).unwrap();
    assert_eq!(&out[..n], &[0, 0, 0, 0, 1, 0, 0]);
}

#[test]
fn commands_before_hello_and_bad_lengths_do_not_execute_success() {
    let mut session = Session::new(config(&MODULES)).unwrap();
    session.control_notifications_enabled().unwrap();
    let mut out = [0; 128];
    let n = session.dispatch(request(0, 2, 1, &[]), &mut out).unwrap();
    assert_eq!(&out[..n], &[4, 0, 0, 0]);
    session
        .dispatch(request(0, 1, 2, &HELLO), &mut out)
        .unwrap();
    for (id, opcode, payload) in [
        (3, 2, &[1][..]),
        (4, 4, &[1][..]),
        (5, 5, &[][..]),
        (6, 6, &[1][..]),
    ] {
        let n = session
            .dispatch(request(0, opcode, id, payload), &mut out)
            .unwrap();
        assert_eq!(&out[..n], &[1, 0, 0, 0]);
    }
}

#[test]
fn only_complete_request_frames_are_accepted() {
    for bad in [
        Header {
            kind: Kind::Response,
            module: 0,
            opcode: 5,
            message_id: 2,
            offset: 0,
            total_length: 4,
        },
        Header {
            kind: Kind::Request,
            module: 0,
            opcode: 5,
            message_id: 0,
            offset: 0,
            total_length: 4,
        },
        Header {
            kind: Kind::Request,
            module: 0,
            opcode: 5,
            message_id: 2,
            offset: 1,
            total_length: 5,
        },
        Header {
            kind: Kind::Request,
            module: 0,
            opcode: 5,
            message_id: 2,
            offset: 0,
            total_length: 5,
        },
    ] {
        let mut session = active_session();
        assert_eq!(
            session.dispatch(
                Frame {
                    header: bad,
                    payload: &[1, 2, 3, 4]
                },
                &mut [0; 128]
            ),
            Err(SessionError::InvalidFrame)
        );
        assert_eq!(session.state(), SessionState::Faulted);
    }
}

#[test]
fn modules_use_exact_sorted_snapshot_and_empty_end_page() {
    let mut entries = [MODULES[0]; 10];
    for (index, entry) in entries.iter_mut().enumerate() {
        entry.module = u8::try_from(index).unwrap();
    }
    let mut session = Session::new(config(&entries)).unwrap();
    session.control_notifications_enabled().unwrap();
    session
        .dispatch(request(0, 1, 1, &HELLO), &mut [0; 128])
        .unwrap();
    let mut out = [0xa5; 128];
    let n = session
        .dispatch(request(0, 3, 2, &[0, 0, 8]), &mut out)
        .unwrap();
    assert_eq!(n, 107);
    assert_eq!(&out[..11], &[0, 0, 0, 0, 4, 3, 2, 1, 8, 0, 8]);
    assert_eq!(out[n], 0xa5);
    for (index, bytes) in out[11..n].chunks_exact(12).enumerate() {
        assert_eq!(bytes[0], u8::try_from(index).unwrap());
        assert_eq!(
            bytes,
            &[
                u8::try_from(index).unwrap(),
                1,
                2,
                1,
                0,
                0,
                0,
                0,
                128,
                0,
                128,
                0
            ]
        );
    }
    let n = session
        .dispatch(request(0, 3, 3, &[8, 0, 8]), &mut out)
        .unwrap();
    assert_eq!(n, 35);
    assert_eq!(&out[..11], &[0, 0, 0, 0, 4, 3, 2, 1, 255, 255, 2]);
    let n = session
        .dispatch(request(0, 3, 4, &[10, 0, 8]), &mut out)
        .unwrap();
    assert_eq!(&out[..n], &[0, 0, 0, 0, 4, 3, 2, 1, 255, 255, 0]);
}

#[test]
fn modules_reject_zero_max_sentinel_and_past_end() {
    let mut session = active_session();
    let mut out = [0; 128];
    for (id, body) in [
        (2, [0, 0, 0]),
        (3, [0, 0, 9]),
        (4, [2, 0, 1]),
        (5, [255, 255, 1]),
    ] {
        let n = session
            .dispatch(request(0, 3, id, &body), &mut out)
            .unwrap();
        assert_eq!(&out[..n], &[1, 0, 0, 0]);
    }
}

#[test]
fn negotiation_bounds_module_limits_and_data_capacity() {
    let mut entries = MODULES;
    entries[0].input_max = 512;
    entries[0].output_max = 512;
    let mut cfg = config(&entries);
    cfg.limits.control_max = 512;
    cfg.limits.data_max = 256;
    let mut session = Session::new(cfg).unwrap();
    session.control_notifications_enabled().unwrap();
    let mut out = [0; 512];
    session
        .dispatch(request(0, 1, 1, &[1, 1, 128, 0, 128, 0, 3, 0]), &mut out)
        .unwrap();
    let hello = HelloResponse::decode(&out[4..22]).unwrap();
    assert_eq!((hello.control_max, hello.data_max), (128, 128));
    let n = session
        .dispatch(request(0, 3, 2, &[0, 0, 1]), &mut out)
        .unwrap();
    assert_eq!(n, 23);
    let entry = ModuleEntry::decode(&out[11..23]).unwrap();
    assert_eq!((entry.input_max, entry.output_max), (128, 128));
}

#[test]
fn absent_module_and_unknown_opcode_have_distinct_statuses() {
    let mut session = active_session();
    let mut out = [0; 128];
    let n = session
        .dispatch(request(0x70, 1, 2, &[]), &mut out)
        .unwrap();
    assert_eq!(&out[..n], &[2, 0, 0, 0]);
    let n = session
        .dispatch(request(0, 0x77, 3, &[]), &mut out)
        .unwrap();
    assert_eq!(&out[..n], &[3, 0, 0, 0]);
}

#[test]
fn configuration_rejects_zero_ids_and_false_catalogue_entries() {
    let mut cfg = config(&MODULES);
    cfg.session = 0;
    assert!(Session::new(cfg).is_err());
    cfg = config(&MODULES);
    cfg.capability_rev = 0;
    assert!(Session::new(cfg).is_err());
    cfg = config(&MODULES);
    cfg.limits.control_max = 127;
    assert!(Session::new(cfg).is_err());
    assert!(Session::new(config(&[])).is_err());
    let duplicate = [MODULES[0]; 2];
    assert!(Session::new(config(&duplicate)).is_err());
    let mut bad = MODULES;
    bad[0].availability = 3;
    assert!(Session::new(config(&bad)).is_err());
    bad = MODULES;
    bad[0].major = 2;
    assert!(Session::new(config(&bad)).is_err());
}

#[test]
fn debug_omits_cached_request_and_response_payloads() {
    let mut session = active_session();
    let secret = b"private-payload-must-never-be-debugged";
    session
        .dispatch(request(0, 5, 2, secret), &mut [0; 128])
        .unwrap();
    let debug = format!("{session:?}");
    assert!(!debug.contains("private-payload"));
    assert!(!debug.contains("112, 114, 105, 118"));
    assert!(!debug.contains("cached_request"));
    assert!(!debug.contains("cached_response"));
}

#[test]
fn profile_and_peer_data_capacity_never_invent_authorization_or_data_backend() {
    for profile in [
        BuildProfile::Commercial,
        BuildProfile::Development,
        BuildProfile::Factory,
    ] {
        let mut cfg = config(&MODULES);
        cfg.profile = profile;
        let mut session = Session::new(cfg).unwrap();
        session.control_notifications_enabled().unwrap();
        let mut out = [0; 128];
        session
            .dispatch(request(0, 1, 1, &[1, 1, 0, 2, 0, 2, 3, 0]), &mut out)
            .unwrap();
        let hello = HelloResponse::decode(&out[4..22]).unwrap();
        assert_eq!(hello.control_max, 128);
        assert_eq!(hello.data_max, 0);
        assert_eq!(hello.access, 1);
        let n = session.dispatch(request(0, 8, 2, &[]), &mut out).unwrap();
        assert_eq!(&out[..n], &[5, 0, 3, 255]);
    }
}

#[test]
fn short_replay_and_short_new_response_preserve_only_the_last_cache() {
    let mut session = active_session();
    let mut original = [0; 128];
    let n = session
        .dispatch(request(0, 5, 2, &[9, 8, 7, 6]), &mut original)
        .unwrap();
    let mut short = [0xa5; 7];
    assert_eq!(
        session.dispatch(request(0, 5, 2, &[9, 8, 7, 6]), &mut short),
        Err(SessionError::Capacity)
    );
    assert_eq!(short, [0xa5; 7]);
    assert_eq!(
        session.dispatch(request(0, 2, 3, &[]), &mut short),
        Err(SessionError::Capacity)
    );
    let mut replay = [0; 128];
    let replay_n = session
        .dispatch(request(0, 5, 2, &[9, 8, 7, 6]), &mut replay)
        .unwrap();
    assert_eq!(n, replay_n);
    assert_eq!(&replay[..n], &original[..n]);
    assert_eq!(session.dispatch(request(0, 2, 3, &[]), &mut replay), Ok(18));
}

#[test]
fn complete_oversized_request_is_a_status_failure_but_not_a_partial_parse() {
    let mut session = active_session();
    let mut out = [0xa5; 128];
    let n = session
        .dispatch(request(0, 5, 2, &[0x7b; 129]), &mut out)
        .unwrap();
    assert_eq!(&out[..n], &[1, 0, 0, 0]);
    assert_eq!(session.state(), SessionState::Active);
    let n = session
        .dispatch(request(0, 3, 3, &[0, 0]), &mut out)
        .unwrap();
    assert_eq!(&out[..n], &[1, 0, 0, 0]);
    let oversized = [0x7b; 513];
    assert_eq!(
        session.dispatch(request(0, 5, 4, &oversized), &mut out),
        Err(SessionError::InvalidFrame)
    );
    assert_eq!(session.state(), SessionState::Faulted);
}

#[test]
fn request_ids_begin_at_one_and_duplicate_subscription_does_not_reset_them() {
    let mut session = Session::new(config(&MODULES)).unwrap();
    session.control_notifications_enabled().unwrap();
    session.control_notifications_enabled().unwrap();
    assert_eq!(
        session.dispatch(request(0, 1, 2, &HELLO), &mut [0; 128]),
        Err(SessionError::RequestOrder)
    );
    let mut session = active_session();
    session.control_notifications_enabled().unwrap();
    let mut out = [0; 128];
    assert_eq!(session.dispatch(request(0, 1, 1, &HELLO), &mut out), Ok(22));
    let n = session.dispatch(request(0, 6, 2, &[]), &mut out).unwrap();
    assert_eq!(&out[..n], &[0, 0, 0, 0, 1, 0, 0]);
}

#[test]
fn registered_personal_module_still_requires_real_resource_authorization() {
    let entries = [
        MODULES[0],
        ModuleEntry {
            module: 0x20,
            ..MODULES[0]
        },
    ];
    let mut session = Session::new(config(&entries)).unwrap();
    session.control_notifications_enabled().unwrap();
    let mut out = [0; 128];
    session
        .dispatch(request(0, 1, 1, &HELLO), &mut out)
        .unwrap();
    let n = session
        .dispatch_with_access(request(0x20, 1, 2, &[]), &TrustedStatus, &mut out)
        .unwrap();
    assert_eq!(&out[..n], &[5, 0, 3, 255]);
}

#[test]
fn test_should_validate_configuration_without_constructing_session_caches() {
    use device_services::ConfigError;
    let compare = |cfg: Config<'_>, expected| {
        assert_eq!(Session::validate_config(&cfg), expected);
        assert_eq!(Session::new(cfg).map(|_| ()), expected);
    };
    compare(config(&MODULES), Ok(()));
    for session in [0, 1, u32::MAX] {
        for capability_rev in [0, 1, u32::MAX] {
            let mut cfg = config(&MODULES);
            cfg.session = session;
            cfg.capability_rev = capability_rev;
            compare(
                cfg,
                if session == 0 {
                    Err(ConfigError::InvalidSession)
                } else if capability_rev == 0 {
                    Err(ConfigError::InvalidCapabilityRevision)
                } else {
                    Ok(())
                },
            );
        }
    }
    for field in 0..7 {
        for value in [0, 19, 20, 127, 128, 512, 522, 523, u16::MAX] {
            let mut cfg = config(&MODULES);
            match field {
                0 => cfg.limits.rx_frame_max = value,
                1 => cfg.limits.tx_frame_max = value,
                2 => cfg.limits.control_max = value,
                3 => cfg.limits.data_max = value,
                4 => cfg.limits.idle_ms = value,
                5 => cfg.limits.fragment_ms = value,
                _ => cfg.limits.response_ms = value,
            }
            let expected = Session::new(cfg).map(|_| ());
            compare(cfg, expected);
        }
    }
    compare(config(&[]), Err(ConfigError::InvalidModules));
    for field in 0..7 {
        for value in [0, 1, 2, 3, 7, 8, 22, 23, 127, 128, 129, 512, 513] {
            let mut entries = MODULES;
            match field {
                0 => entries[0].module = u8::try_from(value).unwrap_or(255),
                1 => entries[0].major = u8::try_from(value).unwrap_or(255),
                2 => entries[0].minor = u8::try_from(value).unwrap_or(255),
                3 => entries[0].availability = u8::try_from(value).unwrap_or(255),
                4 => entries[0].features = u32::from(value),
                5 => entries[0].input_max = value,
                _ => entries[0].output_max = value,
            }
            let cfg = config(&entries);
            compare(cfg, Session::new(cfg).map(|_| ()));
        }
    }
    for second_module in [0, 1, 4, 255] {
        let mut entries = [MODULES[0]; 2];
        entries[1].module = second_module;
        let cfg = config(&entries);
        compare(
            cfg,
            if second_module == 0 {
                Err(ConfigError::InvalidModules)
            } else {
                Ok(())
            },
        );
    }
    let mut entries = [MODULES[0]; 9];
    for (i, entry) in entries.iter_mut().enumerate() {
        entry.module = u8::try_from(i).unwrap();
    }
    for capacity in [106, 107, 128] {
        entries[0].output_max = capacity;
        compare(
            config(&entries),
            if capacity < 107 {
                Err(ConfigError::InvalidModules)
            } else {
                Ok(())
            },
        );
    }
}
