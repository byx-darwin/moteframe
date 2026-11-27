use sha2::{Digest, Sha256};

/// Encode the trusted raw application length, IEEE CRC-32 and SHA-256.
/// Returns None if the length exceeds the protocol field.
#[must_use]
pub fn manifest(raw: &[u8]) -> Option<[u8; 40]> {
    let mut bytes = [0; 40];
    bytes[..4].copy_from_slice(&u32::try_from(raw.len()).ok()?.to_le_bytes());
    bytes[4..8].copy_from_slice(&(!crc_update(u32::MAX, raw)).to_le_bytes());
    bytes[8..].copy_from_slice(&Sha256::digest(raw));
    Some(bytes)
}
pub(crate) fn crc_update(mut crc: u32, bytes: &[u8]) -> u32 {
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    crc
}
