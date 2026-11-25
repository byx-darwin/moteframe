//! Exact decoder boundaries for every compiled command and distinct result dictionaries.
#![cfg(any(
    feature = "phone",
    feature = "media",
    feature = "find",
    feature = "notifications",
    feature = "companion-radio",
    feature = "gimbal"
))]
use device_content_service::{Command, Error};
fn fixtures() -> Vec<(u8, u8, Vec<u8>)> {
    let fixtures: &[(bool, u8, u8, &[u8])] = &[
        (cfg!(feature = "find"), 6, 0x30, &[0, 3, 5, 0]),
        (cfg!(feature = "find"), 6, 0x31, &[1, 0, 0, 0]),
        (cfg!(feature = "find"), 6, 0x32, &[1, 0, 0, 0, 1]),
        (cfg!(feature = "phone"), 7, 0x30, &[0, 0, 0, 0]),
        (cfg!(feature = "phone"), 7, 0x31, &[1, 0, 0, 0, 1]),
        (cfg!(feature = "companion-radio"), 8, 0x30, &[1]),
        (
            cfg!(feature = "notifications"),
            0x30,
            0x30,
            &[1, 0, 0, 0, 1, 5],
        ),
        (cfg!(feature = "media"), 0x33, 0x30, &[1]),
        (cfg!(feature = "media"), 0x33, 0x31, &[1, 0, 0, 0, 0]),
        (cfg!(feature = "media"), 0x33, 0x32, &[1]),
        (cfg!(feature = "media"), 0x33, 0x33, &[1, 0, 0, 0, 0]),
        (
            cfg!(feature = "gimbal"),
            0x33,
            0x34,
            &[1, 0, 0, 0, 1, 0, 0, 0xe8, 3, 0, 0],
        ),
    ];
    fixtures
        .iter()
        .filter(|value| value.0)
        .map(|value| (value.1, value.2, value.3.to_vec()))
        .collect()
}
#[test]
fn test_should_decode_every_compiled_route_and_reject_truncation_or_trailing_bytes()
-> Result<(), Error> {
    for (module, opcode, bytes) in fixtures() {
        Command::decode(module, opcode, &bytes)?;
        for length in 0..bytes.len() {
            assert!(Command::decode(module, opcode, &bytes[..length]).is_err());
        }
        let mut trailing = bytes;
        trailing.push(0);
        assert!(Command::decode(module, opcode, &trailing).is_err());
    }
    assert_eq!(
        Command::decode(0xfe, 0xff, &[]),
        Err(Error::Content(device_content::Error::Unknown))
    );
    Ok(())
}
#[cfg(feature = "media")]
#[test]
fn test_should_keep_frozen_camera_music_results_zero_through_three() -> Result<(), Error> {
    for result in 0..=3 {
        assert!(matches!(
            Command::decode(0x33, 0x31, &[1, 0, 0, 0, result])?,
            Command::CameraAck { id: 1, .. }
        ));
        assert!(matches!(
            Command::decode(0x33, 0x33, &[1, 0, 0, 0, result])?,
            Command::MusicAck { id: 1, .. }
        ));
    }
    assert_eq!(
        Command::decode(0x33, 0x31, &[1, 0, 0, 0, 4]),
        Err(Error::EnumUnsupported)
    );
    Ok(())
}
#[cfg(feature = "notifications")]
#[test]
fn test_should_keep_notification_results_one_through_five() -> Result<(), Error> {
    for result in 1..=5 {
        assert!(matches!(
            Command::decode(0x30, 0x30, &[1, 0, 0, 0, 1, result])?,
            Command::NotificationAck {
                notification: 1,
                ..
            }
        ));
    }
    assert_eq!(
        Command::decode(0x30, 0x30, &[1, 0, 0, 0, 1, 0]),
        Err(Error::EnumUnsupported)
    );
    Ok(())
}
