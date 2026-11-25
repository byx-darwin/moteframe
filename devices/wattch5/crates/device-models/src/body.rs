//! Small checked payload cursors shared by variable-size optional models.
use device_protocol::ProtocolError;
pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}
impl<'a> Reader<'a> {
    pub(crate) const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }
    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8], ProtocolError> {
        let end = self
            .offset
            .checked_add(n)
            .ok_or(ProtocolError::InvalidLength)?;
        let value = self
            .bytes
            .get(self.offset..end)
            .ok_or(ProtocolError::Truncated)?;
        self.offset = end;
        Ok(value)
    }
    pub(crate) fn u8(&mut self) -> Result<u8, ProtocolError> {
        Ok(self.take(1)?[0])
    }
    pub(crate) fn u16(&mut self) -> Result<u16, ProtocolError> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }
    #[cfg(any(feature = "reminders", feature = "notifications"))]
    pub(crate) fn u32(&mut self) -> Result<u32, ProtocolError> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
    pub(crate) fn i64(&mut self) -> Result<i64, ProtocolError> {
        let b = self.take(8)?;
        Ok(i64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }
    #[cfg(any(feature = "reminders", feature = "notifications"))]
    pub(crate) fn string(&mut self, max: usize) -> Result<&'a str, ProtocolError> {
        let n = usize::from(self.u16()?);
        if n > max {
            return Err(ProtocolError::Capacity);
        }
        let value = core::str::from_utf8(self.take(n)?).map_err(|_| ProtocolError::InvalidState)?;
        if value.as_bytes().contains(&0) {
            return Err(ProtocolError::InvalidState);
        }
        Ok(value)
    }
    pub(crate) fn finish(self) -> Result<(), ProtocolError> {
        if self.offset == self.bytes.len() {
            Ok(())
        } else {
            Err(ProtocolError::InvalidLength)
        }
    }
}
pub(crate) struct Writer<'a> {
    bytes: &'a mut [u8],
    offset: usize,
}
impl<'a> Writer<'a> {
    pub(crate) fn new(bytes: &'a mut [u8]) -> Self {
        Self { bytes, offset: 0 }
    }
    pub(crate) fn put(&mut self, value: &[u8]) -> Result<(), ProtocolError> {
        let end = self
            .offset
            .checked_add(value.len())
            .ok_or(ProtocolError::Capacity)?;
        self.bytes
            .get_mut(self.offset..end)
            .ok_or(ProtocolError::Capacity)?
            .copy_from_slice(value);
        self.offset = end;
        Ok(())
    }
    pub(crate) fn u8(&mut self, v: u8) -> Result<(), ProtocolError> {
        self.put(&[v])
    }
    pub(crate) fn u16(&mut self, v: u16) -> Result<(), ProtocolError> {
        self.put(&v.to_le_bytes())
    }
    #[cfg(any(feature = "reminders", feature = "notifications"))]
    pub(crate) fn u32(&mut self, v: u32) -> Result<(), ProtocolError> {
        self.put(&v.to_le_bytes())
    }
    pub(crate) fn i64(&mut self, v: i64) -> Result<(), ProtocolError> {
        self.put(&v.to_le_bytes())
    }
    #[cfg(any(feature = "reminders", feature = "notifications"))]
    pub(crate) fn string(&mut self, v: &str, max: usize) -> Result<(), ProtocolError> {
        if v.len() > max || v.as_bytes().contains(&0) {
            return Err(ProtocolError::InvalidLength);
        }
        self.u16(u16::try_from(v.len()).map_err(|_| ProtocolError::InvalidLength)?)?;
        self.put(v.as_bytes())
    }
    pub(crate) const fn len(&self) -> usize {
        self.offset
    }
}
