//! Literal common control events carry no status prefix, DATA credit or application CRC.
use device_application::{Change, ControlEvent};
#[test]
fn test_should_match_frozen_property_and_object_change_bytes() {
    let mut out = [0; 20];
    assert_eq!(
        ControlEvent::PropertyChanged {
            module: 4,
            key: 1,
            revision: 2
        }
        .encode(7, &mut out),
        Ok(16)
    );
    assert_eq!(
        &out[..16],
        &[0x12, 4, 0x90, 0, 7, 0, 0, 0, 6, 0, 1, 0, 2, 0, 0, 0]
    );
    assert_eq!(
        ControlEvent::ObjectChanged {
            module: 5,
            object_id: 1,
            revision: 2,
            change: Change::Deleted
        }
        .encode(8, &mut out),
        Ok(19)
    );
    assert_eq!(
        &out[..19],
        &[
            0x12, 5, 0x91, 0, 8, 0, 0, 0, 9, 0, 1, 0, 0, 0, 2, 0, 0, 0, 2
        ]
    );
}
#[test]
fn test_should_require_nonzero_revision_and_stable_identity_before_writing_output() {
    let mut out = [0xa5; 20];
    assert!(
        ControlEvent::CapabilityChanged { revision: 0 }
            .encode(1, &mut out)
            .is_err()
    );
    assert_eq!(out, [0xa5; 20]);
    assert!(
        ControlEvent::ObjectChanged {
            module: 5,
            object_id: u32::MAX,
            revision: 1,
            change: Change::Updated
        }
        .encode(1, &mut out)
        .is_err()
    );
    assert!(
        ControlEvent::CapabilityChanged { revision: 1 }
            .encode(0, &mut out)
            .is_err()
    );
    assert_eq!(
        ControlEvent::CapabilityChanged { revision: 2 }.encode(1, &mut out),
        Ok(14)
    );
    assert_eq!(
        &out[..14],
        &[0x12, 0, 0x80, 0, 1, 0, 0, 0, 4, 0, 2, 0, 0, 0]
    );
}
#[cfg(feature = "device")]
#[test]
fn test_should_emit_validated_battery_body_without_extra_crc_or_status() {
    let mut out = [0xa5; 20];
    let battery = device_application::BatteryState {
        level: 80,
        charging: 0,
        charge_complete: 0,
        since_charge_s: 3600,
        valid_mask: 15,
    };
    assert_eq!(
        ControlEvent::Battery { state: battery }.encode(9, &mut out),
        Ok(18)
    );
    assert_eq!(
        &out[..18],
        &[
            0x12, 2, 0x80, 0, 9, 0, 0, 0, 8, 0, 80, 0, 0, 0x10, 0x0e, 0, 0, 15
        ]
    );
    let before = out;
    let invalid = device_application::BatteryState {
        level: 101,
        ..battery
    };
    assert!(
        ControlEvent::Battery { state: invalid }
            .encode(10, &mut out)
            .is_err()
    );
    assert_eq!(out, before);
}
