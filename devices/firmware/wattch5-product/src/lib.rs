//! Executable offline protocol checks shared by host validation and ARM entry.
#![no_std]
#![forbid(unsafe_code)]

#[cfg(feature = "runtime")]
pub mod runtime;

#[cfg(all(
    feature = "commercial",
    any(
        feature = "raw-data",
        feature = "self-test",
        feature = "engineering",
        feature = "factory-config"
    )
))]
compile_error!("commercial excludes protocol modules 70-73");
#[cfg(any(
    all(feature = "commercial", feature = "development"),
    all(feature = "commercial", feature = "factory"),
    all(feature = "development", feature = "factory")
))]
compile_error!("select exactly one product profile");
#[cfg(not(any(feature = "commercial", feature = "development", feature = "factory")))]
compile_error!("select exactly one product profile");

use device_protocol::{
    Crc32c, Fragmenter, Frame, Header, HelloRequest, Kind, Progress, ProtocolError, Reassembler,
};
use device_services::{BuildProfile, Config, ConfigError, DeviceInfo, Session, SessionError};

/// Explicit build profile; it never grants access or registers a module.
#[must_use]
pub const fn build_profile() -> BuildProfile {
    if cfg!(feature = "development") {
        BuildProfile::Development
    } else if cfg!(feature = "factory") {
        BuildProfile::Factory
    } else {
        BuildProfile::Commercial
    }
}

/// Literal WAP 1.2 HELLO request encoded in one 18-byte GATT value.
pub const HELLO_VECTOR: [u8; 18] = [0x10, 0, 1, 0, 1, 0, 0, 0, 8, 0, 1, 1, 128, 0, 128, 0, 3, 0];

/// Evidence of executed software checks, never evidence of hardware behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SelfCheckReport {
    /// Number of completed independent checks.
    pub checks_passed: u32,
    /// Fragments actually emitted and reassembled for a 21-byte body.
    pub fragments: u32,
    /// CRC32C of the standard `123456789` vector.
    pub crc32c_check: u32,
}

/// Failing software check, retained separately from any hardware effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelfCheckError {
    /// Actual protocol codec or lifecycle rejection.
    Protocol(ProtocolError),
    /// A decoded result differs from an independently specified vector.
    Mismatch,
    /// Invalid offline service configuration.
    Configuration(ConfigError),
    /// Real service session rejection.
    Session(SessionError),
}
impl From<ConfigError> for SelfCheckError {
    fn from(value: ConfigError) -> Self {
        Self::Configuration(value)
    }
}
impl From<SessionError> for SelfCheckError {
    fn from(value: SessionError) -> Self {
        Self::Session(value)
    }
}

/// Exercise actual core session dispatch and its restricted-access lifecycle.
///
/// Product ID zero is an explicit unassigned candidate identity. No trusted
/// access provider, persisted state, BLE runtime or hardware operation exists.
/// # Errors
/// Fails if any core reply differs from its fixed wire contract.
#[inline(never)]
pub fn service_self_check() -> Result<u32, SelfCheckError> {
    let modules = [device_protocol::ModuleEntry {
        module: 0,
        major: 1,
        minor: 2,
        availability: 1,
        features: 0,
        input_max: 128,
        output_max: 128,
    }];
    let config = Config {
        session: 1,
        capability_rev: 1,
        profile: build_profile(),
        info: DeviceInfo {
            product_id: 0,
            fw_major: 0,
            fw_minor: 1,
            fw_patch: 0,
            build_id: 0,
        },
        modules: &modules,
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
    let mut hello_response = [
        0, 0, 0, 0, 1, 2, 1, 0, 0, 0, 128, 0, 0, 0, 1, 0, 1, 0, 1, 0, 0, 0,
    ];
    hello_response[15] = build_profile() as u8;
    for (opcode, id, body, expected) in [
        (1, 1, &[1, 1, 128, 0, 0, 0, 3, 0][..], &hello_response[..]),
        (
            2,
            2,
            &[][..],
            &[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0][..],
        ),
        (
            5,
            3,
            &[0x78, 0x56, 0x34, 0x12][..],
            &[0, 0, 0, 0, 0x78, 0x56, 0x34, 0x12][..],
        ),
        (6, 4, &[][..], &[0, 0, 0, 0, 1, 0, 0][..]),
        (7, 5, &[1, 2, 0, 1, 0, 0, 0][..], &[5, 0, 3, 255][..]),
        (8, 6, &[][..], &[5, 0, 3, 255][..]),
    ] {
        let frame = service_request(opcode, id, body)?;
        let length = session.dispatch(frame, &mut out)?;
        if &out[..length] != expected {
            return Err(SelfCheckError::Mismatch);
        }
    }
    session.disconnect();
    if session.dispatch(service_request(2, 7, &[])?, &mut out) != Err(SessionError::Closed) {
        return Err(SelfCheckError::Mismatch);
    }
    Ok(6)
}

fn service_request(
    opcode: u8,
    message_id: u16,
    payload: &[u8],
) -> Result<Frame<'_>, ProtocolError> {
    Ok(Frame {
        header: Header {
            kind: Kind::Request,
            module: 0,
            opcode,
            message_id,
            offset: 0,
            total_length: u16::try_from(payload.len()).map_err(|_| ProtocolError::InvalidLength)?,
        },
        payload,
    })
}
impl From<ProtocolError> for SelfCheckError {
    fn from(value: ProtocolError) -> Self {
        Self::Protocol(value)
    }
}

/// Run real frame/HELLO, malformed-header, fragmentation and CRC32C checks.
///
/// # Errors
/// Fails on invalid input or any mismatch; no successful hardware result is made.
#[inline(never)]
pub fn self_check(vector: &[u8]) -> Result<SelfCheckReport, SelfCheckError> {
    let frame = Frame::decode(vector, 20, 128)?;
    let hello = HelloRequest::decode(frame.payload)?;
    if frame.header
        != (Header {
            kind: Kind::Request,
            module: 0,
            opcode: 1,
            message_id: 1,
            offset: 0,
            total_length: 8,
        })
        || hello
            != (HelloRequest {
                min_major: 1,
                max_major: 1,
                rx_control_max: 128,
                rx_data_max: 128,
                flags: 3,
            })
    {
        return Err(SelfCheckError::Mismatch);
    }
    let mut encoded = [0; 20];
    let length = frame.encode(&mut encoded)?;
    if &encoded[..length] != vector {
        return Err(SelfCheckError::Mismatch);
    }

    let mut invalid = HELLO_VECTOR;
    invalid[3] = 1;
    if Frame::decode(&invalid, 20, 128) != Err(ProtocolError::Reserved) {
        return Err(SelfCheckError::Mismatch);
    }

    let body = b"123456789012345678901";
    let header = Header {
        kind: Kind::Request,
        module: 0,
        opcode: 2,
        message_id: 2,
        offset: 0,
        total_length: 21,
    };
    let mut sender = Fragmenter::new(header, body, 20)?;
    let mut receiver = Reassembler::<128>::new(20, 128)?;
    let mut fragments = 0;
    while !sender.is_complete() {
        let length = sender.write_next(&mut encoded)?;
        let decoded = Frame::decode(&encoded[..length], 20, 128)?;
        let progress = receiver.push(decoded, u64::from(fragments) * 100)?;
        fragments += 1;
        sender.advance()?;
        if (progress == Progress::Complete) != sender.is_complete() {
            return Err(SelfCheckError::Mismatch);
        }
    }
    if fragments != 3 || receiver.message()?.payload != body {
        return Err(SelfCheckError::Mismatch);
    }

    let mut crc = Crc32c::new();
    crc.update(b"123456789");
    let crc32c_check = crc.finish();
    if crc32c_check != 0xe306_9283 {
        return Err(SelfCheckError::Mismatch);
    }
    Ok(SelfCheckReport {
        checks_passed: 4,
        fragments,
        crc32c_check,
    })
}

#[cfg(feature = "wattch5-source")]
pub mod product;
