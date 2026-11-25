use crate::Error;
#[cfg(any(
    feature = "phone",
    feature = "gimbal",
    feature = "find",
    feature = "notifications",
    feature = "wallet",
    feature = "watchface",
    feature = "companion-radio",
    feature = "weather",
    feature = "contacts",
    feature = "memo"
))]
pub(crate) fn ensure(ok: bool) -> Result<(), Error> {
    if ok { Ok(()) } else { Err(Error::Invalid) }
}
#[derive(Debug)]
#[cfg(any(
    feature = "phone",
    feature = "gimbal",
    feature = "find",
    feature = "notifications",
    feature = "wallet",
    feature = "watchface",
    feature = "companion-radio",
    feature = "contacts",
    feature = "memo"
))]
pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}
#[cfg(any(
    feature = "phone",
    feature = "gimbal",
    feature = "find",
    feature = "notifications",
    feature = "wallet",
    feature = "watchface",
    feature = "companion-radio",
    feature = "contacts",
    feature = "memo"
))]
impl<'a> Reader<'a> {
    pub(crate) const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }
    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        let end = self.pos.checked_add(n).ok_or(Error::Capacity)?;
        let b = self.bytes.get(self.pos..end).ok_or(Error::Invalid)?;
        self.pos = end;
        Ok(b)
    }
    pub(crate) fn array<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        self.take(N)?.try_into().map_err(|_| Error::Invalid)
    }
    #[cfg(any(
        feature = "phone",
        feature = "gimbal",
        feature = "find",
        feature = "notifications",
        feature = "wallet",
        feature = "watchface",
        feature = "companion-radio"
    ))]
    pub(crate) fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.array::<1>()?[0])
    }
    #[cfg(any(
        feature = "phone",
        feature = "find",
        feature = "notifications",
        feature = "watchface",
        feature = "companion-radio"
    ))]
    pub(crate) fn boolean(&mut self) -> Result<bool, Error> {
        let n = self.u8()?;
        ensure(n <= 1)?;
        Ok(n == 1)
    }
    #[cfg(any(
        feature = "phone",
        feature = "find",
        feature = "notifications",
        feature = "wallet",
        feature = "watchface",
        feature = "contacts",
        feature = "memo"
    ))]
    pub(crate) fn u16(&mut self) -> Result<u16, Error> {
        Ok(u16::from_le_bytes(self.array()?))
    }
    #[cfg(any(feature = "watchface", feature = "gimbal"))]
    pub(crate) fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.array()?))
    }
    #[cfg(feature = "phone")]
    pub(crate) fn i32(&mut self) -> Result<i32, Error> {
        Ok(i32::from_le_bytes(self.array()?))
    }
    #[cfg(any(
        feature = "phone",
        feature = "notifications",
        feature = "wallet",
        feature = "watchface",
        feature = "contacts",
        feature = "memo"
    ))]
    pub(crate) fn text(&mut self, max: u16) -> Result<&'a str, Error> {
        let n = self.u16()?;
        ensure(n <= max)?;
        let b = self.take(usize::from(n))?;
        ensure(!b.contains(&0))?;
        core::str::from_utf8(b).map_err(|_| Error::Invalid)
    }
    #[cfg(feature = "notifications")]
    pub(crate) fn rest(&self) -> &'a [u8] {
        &self.bytes[self.pos..]
    }
    pub(crate) fn end(&self) -> Result<(), Error> {
        ensure(self.pos == self.bytes.len())
    }
}
#[derive(Debug)]
#[cfg(any(
    feature = "phone",
    feature = "gimbal",
    feature = "media",
    feature = "find",
    feature = "notifications",
    feature = "wallet",
    feature = "watchface",
    feature = "companion-radio",
    feature = "contacts",
    feature = "memo"
))]
pub(crate) struct Writer<'a> {
    bytes: &'a mut [u8],
    pos: usize,
}
#[cfg(any(
    feature = "phone",
    feature = "gimbal",
    feature = "media",
    feature = "find",
    feature = "notifications",
    feature = "wallet",
    feature = "watchface",
    feature = "companion-radio",
    feature = "contacts",
    feature = "memo"
))]
impl<'a> Writer<'a> {
    pub(crate) fn new(bytes: &'a mut [u8]) -> Self {
        Self { bytes, pos: 0 }
    }
    pub(crate) fn bytes(&mut self, b: &[u8]) -> Result<(), Error> {
        let end = self.pos.checked_add(b.len()).ok_or(Error::Capacity)?;
        self.bytes
            .get_mut(self.pos..end)
            .ok_or(Error::Capacity)?
            .copy_from_slice(b);
        self.pos = end;
        Ok(())
    }
    #[cfg(any(
        feature = "phone",
        feature = "gimbal",
        feature = "media",
        feature = "find",
        feature = "notifications",
        feature = "wallet",
        feature = "watchface",
        feature = "companion-radio"
    ))]
    pub(crate) fn u8(&mut self, n: u8) -> Result<(), Error> {
        self.bytes(&[n])
    }
    #[cfg(any(
        feature = "phone",
        feature = "gimbal",
        feature = "find",
        feature = "notifications",
        feature = "wallet",
        feature = "watchface",
        feature = "contacts",
        feature = "memo"
    ))]
    pub(crate) fn u16(&mut self, n: u16) -> Result<(), Error> {
        self.bytes(&n.to_le_bytes())
    }
    #[cfg(any(
        feature = "phone",
        feature = "gimbal",
        feature = "media",
        feature = "find",
        feature = "notifications",
        feature = "watchface"
    ))]
    pub(crate) fn u32(&mut self, n: u32) -> Result<(), Error> {
        self.bytes(&n.to_le_bytes())
    }
    #[cfg(feature = "phone")]
    pub(crate) fn i32(&mut self, n: i32) -> Result<(), Error> {
        self.bytes(&n.to_le_bytes())
    }
    #[cfg(any(
        feature = "phone",
        feature = "wallet",
        feature = "watchface",
        feature = "contacts",
        feature = "memo"
    ))]
    pub(crate) fn text(&mut self, s: &str, max: u16) -> Result<(), Error> {
        let n = u16::try_from(s.len()).map_err(|_| Error::Capacity)?;
        ensure(n <= max && !s.as_bytes().contains(&0))?;
        self.u16(n)?;
        self.bytes(s.as_bytes())
    }
    pub(crate) const fn len(&self) -> usize {
        self.pos
    }
}
