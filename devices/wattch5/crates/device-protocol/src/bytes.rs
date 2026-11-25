//! Internal checked byte cursors; no native-layout serialization.
use crate::ProtocolError;
pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
    used: usize,
}
impl<'a> Reader<'a> {
    pub(crate) const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, used: 0 }
    }
    pub(crate) fn bytes(&mut self, len: usize) -> Result<&'a [u8], ProtocolError> {
        let end = self
            .used
            .checked_add(len)
            .ok_or(ProtocolError::InvalidLength)?;
        let value = self
            .bytes
            .get(self.used..end)
            .ok_or(ProtocolError::Truncated)?;
        self.used = end;
        Ok(value)
    }
    pub(crate) fn u8(&mut self) -> Result<u8, ProtocolError> {
        Ok(self.bytes(1)?[0])
    }
    pub(crate) fn u16(&mut self) -> Result<u16, ProtocolError> {
        let b = self.bytes(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    pub(crate) fn u32(&mut self) -> Result<u32, ProtocolError> {
        let b = self.bytes(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    pub(crate) fn finish(self) -> Result<(), ProtocolError> {
        if self.used == self.bytes.len() {
            Ok(())
        } else {
            Err(ProtocolError::InvalidLength)
        }
    }
}
pub(crate) struct Writer<'a> {
    bytes: &'a mut [u8],
    used: usize,
}
impl<'a> Writer<'a> {
    pub(crate) fn new(bytes: &'a mut [u8]) -> Self {
        Self { bytes, used: 0 }
    }
    pub(crate) fn bytes(&mut self, value: &[u8]) -> Result<(), ProtocolError> {
        let end = self
            .used
            .checked_add(value.len())
            .ok_or(ProtocolError::Capacity)?;
        self.bytes
            .get_mut(self.used..end)
            .ok_or(ProtocolError::Capacity)?
            .copy_from_slice(value);
        self.used = end;
        Ok(())
    }
    pub(crate) fn u8(&mut self, v: u8) -> Result<(), ProtocolError> {
        self.bytes(&[v])
    }
    pub(crate) fn u16(&mut self, v: u16) -> Result<(), ProtocolError> {
        self.bytes(&v.to_le_bytes())
    }
    pub(crate) fn u32(&mut self, v: u32) -> Result<(), ProtocolError> {
        self.bytes(&v.to_le_bytes())
    }
    pub(crate) const fn len(&self) -> usize {
        self.used
    }
}
