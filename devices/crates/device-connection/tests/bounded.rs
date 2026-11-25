//! Bounded CONTROL capacity is independent of complete DATA body capacity.
#![allow(clippy::unwrap_used)]
use device_connection::{BoundedControlConnection, ControlConnection};
use device_protocol::{ModuleEntry, ProtocolLimits};
use device_services::{BuildProfile, Config, DeviceInfo};
const MODULES: [ModuleEntry; 1] = [ModuleEntry {
    module: 0,
    major: 1,
    minor: 2,
    availability: 1,
    features: 0,
    input_max: 8,
    output_max: 23,
}];
fn config(control: u16) -> Config<'static> {
    Config {
        session: 1,
        capability_rev: 1,
        profile: BuildProfile::Development,
        info: DeviceInfo {
            product_id: 0,
            fw_major: 0,
            fw_minor: 1,
            fw_patch: 0,
            build_id: 1,
        },
        modules: &MODULES,
        limits: ProtocolLimits {
            rx_frame_max: 20,
            tx_frame_max: 20,
            control_max: control,
            data_max: 512,
            idle_ms: 2000,
            fragment_ms: 100,
            response_ms: 10000,
        },
    }
}
#[test]
fn test_should_retain_full_data_limit_with_small_control_and_reject_overcapacity() {
    let mut c = BoundedControlConnection::<128>::new(config(128)).unwrap();
    c.notifications_enabled().unwrap();
    assert!(BoundedControlConnection::<128>::new(config(512)).is_err());
    assert_eq!(
        core::mem::size_of::<ControlConnection<'_>>()
            - core::mem::size_of::<BoundedControlConnection<'_, 128>>(),
        1536
    );
}
