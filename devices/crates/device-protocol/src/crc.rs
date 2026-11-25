//! Streaming CRC32C and explicit object/event domain separation.
use crate::ProtocolError;
/// CRC32C Castagnoli, reflected polynomial, init/xorout FFFFFFFF.
#[derive(Debug, Clone, Copy)]
pub struct Crc32c(u32);
impl Default for Crc32c {
    fn default() -> Self {
        Self::new()
    }
}
impl Crc32c {
    /// Initialize a new CRC stream.
    #[must_use]
    pub const fn new() -> Self {
        Self(u32::MAX)
    }
    /// Append bytes in wire order.
    pub fn update(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u32::from(*byte);
            for _ in 0..8 {
                self.0 = (self.0 >> 1) ^ (0x82f6_3b78 & 0u32.wrapping_sub(self.0 & 1));
            }
        }
    }
    /// Obtain the current finalized value without consuming the stream.
    #[must_use]
    pub const fn finish(self) -> u32 {
        self.0 ^ u32::MAX
    }
}
/// CRC for a complete object: WOBJ, module, kind LE16, length LE32, body.
/// # Errors
/// Returns invalid length if the object length is not representable by LE32.
pub fn object_crc(module: u8, kind: u16, body: &[u8]) -> Result<u32, ProtocolError> {
    let len = u32::try_from(body.len()).map_err(|_| ProtocolError::InvalidLength)?;
    let mut c = Crc32c::new();
    c.update(b"WOBJ");
    c.update(&[module]);
    c.update(&kind.to_le_bytes());
    c.update(&len.to_le_bytes());
    c.update(body);
    Ok(c.finish())
}
/// CRC for an integrity-protected event body, excluding its trailing CRC.
/// # Errors
/// Returns invalid length if the body cannot fit its LE16 length prefix.
pub fn event_crc(module: u8, opcode: u8, body: &[u8]) -> Result<u32, ProtocolError> {
    let len = u16::try_from(body.len()).map_err(|_| ProtocolError::InvalidLength)?;
    let mut c = Crc32c::new();
    c.update(b"WAP1");
    c.update(&[module, opcode]);
    c.update(&len.to_le_bytes());
    c.update(body);
    Ok(c.finish())
}
