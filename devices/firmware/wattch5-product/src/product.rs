//! Offline source-selected module catalogue. Unbound backends are availability0.
use crate::{SelfCheckError, service_request};
use device_protocol::ModuleEntry;
use device_services::{Config, DeviceInfo, Session, SessionError};
const COUNT: usize = device_application::COMPILED_MODULES.len();
const fn entries() -> [ModuleEntry; COUNT] {
    let mut entries = [ModuleEntry {
        module: 0,
        major: 1,
        minor: 0,
        availability: 0,
        features: 0,
        input_max: 128,
        output_max: 128,
    }; COUNT];
    let mut index = 0;
    while index < COUNT {
        let module = device_application::COMPILED_MODULES[index];
        entries[index].module = module.module;
        entries[index].major = module.major;
        entries[index].minor = module.minor;
        if module.module == 0 {
            entries[index].availability = 1;
        }
        index += 1;
    }
    entries
}
/// Actual compile-selected catalogue for this offline image; business backends remain unavailable.
pub const MODULES: [ModuleEntry; COUNT] = entries();
/// Execute literal source-selection and real Session MODULES pagination checks on the host/ARM self-check.
/// # Errors
/// A leaked/omitted module, wrong version/availability, broken pagination or unintended available backend.
#[inline(never)]
pub fn source_self_check() -> Result<u32, SelfCheckError> {
    let expected = [
        0, 1, 2, 3, 4, 5, 6, 0x10, 0x20, 0x21, 0x22, 0x23, 0x25, 0x26, 0x27, 0x28, 0x30, 0x31,
        0x32, 0x33,
    ];
    if MODULES.len() != expected.len() {
        return Err(SelfCheckError::Mismatch);
    }
    for (entry, id) in MODULES.iter().zip(expected) {
        let version = if matches!(id, 0 | 0x10) {
            (1, 2)
        } else {
            (1, 0)
        };
        if entry.module != id
            || (entry.major, entry.minor) != version
            || entry.availability != u8::from(id == 0)
        {
            return Err(SelfCheckError::Mismatch);
        }
    }
    let config = Config {
        session: 1,
        capability_rev: 1,
        profile: crate::build_profile(),
        info: DeviceInfo {
            product_id: 0,
            fw_major: 0,
            fw_minor: 1,
            fw_patch: 0,
            build_id: 0,
        },
        modules: &MODULES,
        limits: device_protocol::ProtocolLimits {
            rx_frame_max: 20,
            tx_frame_max: 20,
            control_max: 128,
            data_max: 0,
            idle_ms: 2000,
            fragment_ms: 100,
            response_ms: 10000,
        },
    };
    let mut session = Session::new(config)?;
    session.control_notifications_enabled()?;
    let mut out = [0; 128];
    session.dispatch(
        service_request(1, 1, &[1, 1, 128, 0, 0, 0, 3, 0])?,
        &mut out,
    )?;
    let mut start = 0u16;
    let mut id = 2u16;
    let mut found = 0usize;
    loop {
        let body = [start.to_le_bytes()[0], start.to_le_bytes()[1], 8];
        let length = session.dispatch(service_request(3, id, &body)?, &mut out)?;
        if length < 11 || out[..4] != [0; 4] {
            return Err(SelfCheckError::Mismatch);
        }
        let count = usize::from(out[10]);
        if count == 0 || length != 11 + count * 12 {
            return Err(SelfCheckError::Mismatch);
        }
        for bytes in out[11..length].chunks_exact(12) {
            let entry = ModuleEntry::decode(bytes)?;
            if MODULES.get(found) != Some(&entry) {
                return Err(SelfCheckError::Mismatch);
            }
            found += 1;
        }
        start = u16::from_le_bytes([out[8], out[9]]);
        if start == u16::MAX {
            break;
        }
        id = id.checked_add(1).ok_or(SelfCheckError::Mismatch)?;
    }
    if found != expected.len() {
        return Err(SelfCheckError::Mismatch);
    }
    // This image does not have a trusted resource owner, hardware store or BLE runtime.
    session.disconnect();
    if session.dispatch(service_request(2, id + 1, &[])?, &mut out) != Err(SessionError::Closed) {
        return Err(SelfCheckError::Mismatch);
    }
    u32::try_from(found).map_err(|_| SelfCheckError::Mismatch)
}
