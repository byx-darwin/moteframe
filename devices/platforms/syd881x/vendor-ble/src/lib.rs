//! Version-specific `SYD881x` BLE integration boundary.
//!
//! Safe event decoding is available on every target. Raw vendor calls require
//! ARM plus `vendor-ffi`; board startup and synchronization remain explicit.
#![no_std]

use device_ble::{Channel, Event, Packet};

/// Actual write capacity from the pinned library's DWARF, not the 188-byte header.
pub const WRITE_CAPACITY: usize = 185;
/// SHA256 of `syd8811_ble_mtu_lib20240912_175614.lib`.
pub const LIBRARY_SHA256: &str = "4692ee450f800eb747efc981798c82c6223d60526259df052ca6a14ee98ed8a7";

/// Invalid event data; never silently truncates a packet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// Missing event prefix or bytes required by the active variant.
    Truncated,
    /// Write length exceeds the actual library's bound.
    PayloadTooLarge,
}

fn word(bytes: &[u8], offset: usize) -> Result<u16, DecodeError> {
    let value = bytes
        .get(offset..offset + 2)
        .ok_or(DecodeError::Truncated)?;
    Ok(u16::from_le_bytes([value[0], value[1]]))
}

/// Decodes a copied SDK event. Unknown events are ignored explicitly.
///
/// ATT handles become `Channel(handle)` here; a board adapter must map them to
/// its stable public channels. This function does not assume a product profile.
/// # Errors
/// Rejects truncated prefixes/payloads and lengths beyond the pinned SDK capacity.
pub fn decode_event(bytes: &[u8]) -> Result<Option<Event>, DecodeError> {
    let prefix = bytes.get(1..5).ok_or(DecodeError::Truncated)?;
    if bytes.first() != Some(&0) {
        return Ok(None);
    }
    let code = u32::from_le_bytes([prefix[0], prefix[1], prefix[2], prefix[3]]);
    match code {
        2 => Ok(Some(Event::Connected)),
        4 => Ok(Some(Event::Disconnected {
            reason: *bytes.get(5).ok_or(DecodeError::Truncated)?,
        })),
        0x200 => {
            let channel = Channel(word(bytes, 9)?);
            let length = usize::from(word(bytes, 11)?);
            if length > WRITE_CAPACITY {
                return Err(DecodeError::PayloadTooLarge);
            }
            let data = bytes.get(13..13 + length).ok_or(DecodeError::Truncated)?;
            let packet = Packet::try_from_slice(data).map_err(|_| DecodeError::PayloadTooLarge)?;
            Ok(Some(Event::Received { channel, packet }))
        }
        _ => Ok(None),
    }
}

#[cfg(all(feature = "vendor-ffi", target_arch = "arm"))]
pub mod ffi;

pub mod runtime;
