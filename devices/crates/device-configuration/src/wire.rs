use crate::Error;
pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
    max_text: usize,
}
impl<'a> Reader<'a> {
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            pos: 0,
            max_text: usize::from(u16::MAX),
        }
    }
    pub(crate) fn with_max_text(bytes: &'a [u8], max_text: usize) -> Self {
        Self {
            bytes,
            pos: 0,
            max_text,
        }
    }
    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        let end = self.pos.checked_add(n).ok_or(Error::Invalid)?;
        let b = self.bytes.get(self.pos..end).ok_or(Error::Invalid)?;
        self.pos = end;
        Ok(b)
    }
    pub(crate) fn array<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        self.take(N)?.try_into().map_err(|_| Error::Invalid)
    }
    pub(crate) fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.array::<1>()?[0])
    }
    pub(crate) fn boolean(&mut self) -> Result<bool, Error> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(Error::Invalid),
        }
    }
    pub(crate) fn u16(&mut self) -> Result<u16, Error> {
        Ok(u16::from_le_bytes(self.array()?))
    }
    pub(crate) fn i16(&mut self) -> Result<i16, Error> {
        Ok(i16::from_le_bytes(self.array()?))
    }
    pub(crate) fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.array()?))
    }
    pub(crate) fn i32(&mut self) -> Result<i32, Error> {
        Ok(i32::from_le_bytes(self.array()?))
    }
    pub(crate) fn u64(&mut self) -> Result<u64, Error> {
        Ok(u64::from_le_bytes(self.array()?))
    }
    pub(crate) fn i64(&mut self) -> Result<i64, Error> {
        Ok(i64::from_le_bytes(self.array()?))
    }
    pub(crate) fn text(&mut self) -> Result<&'a str, Error> {
        let n = usize::from(self.u16()?);
        if n > self.max_text {
            return Err(Error::Invalid);
        }
        let b = self.take(n)?;
        if b.contains(&0) {
            return Err(Error::Invalid);
        }
        core::str::from_utf8(b).map_err(|_| Error::Invalid)
    }
    pub(crate) fn end(&self) -> Result<(), Error> {
        if self.pos == self.bytes.len() {
            Ok(())
        } else {
            Err(Error::Invalid)
        }
    }
    pub(crate) fn slice(&self, start: usize, end: usize) -> Result<&'a [u8], Error> {
        self.bytes.get(start..end).ok_or(Error::Invalid)
    }
    pub(crate) fn position(&self) -> usize {
        self.pos
    }
}
pub(crate) struct Writer<'a> {
    bytes: &'a mut [u8],
    pos: usize,
}
impl<'a> Writer<'a> {
    pub(crate) fn new(bytes: &'a mut [u8]) -> Self {
        Self { bytes, pos: 0 }
    }
    pub(crate) fn put(&mut self, b: &[u8]) -> Result<(), Error> {
        let end = self.pos.checked_add(b.len()).ok_or(Error::Capacity)?;
        self.bytes
            .get_mut(self.pos..end)
            .ok_or(Error::Capacity)?
            .copy_from_slice(b);
        self.pos = end;
        Ok(())
    }
    pub(crate) fn u8(&mut self, v: u8) -> Result<(), Error> {
        self.put(&[v])
    }
    pub(crate) fn boolean(&mut self, v: bool) -> Result<(), Error> {
        self.u8(u8::from(v))
    }
    pub(crate) fn u16(&mut self, v: u16) -> Result<(), Error> {
        self.put(&v.to_le_bytes())
    }
    pub(crate) fn i16(&mut self, v: i16) -> Result<(), Error> {
        self.put(&v.to_le_bytes())
    }
    pub(crate) fn u32(&mut self, v: u32) -> Result<(), Error> {
        self.put(&v.to_le_bytes())
    }
    pub(crate) fn i64(&mut self, v: i64) -> Result<(), Error> {
        self.put(&v.to_le_bytes())
    }
    pub(crate) fn text(&mut self, v: &str) -> Result<(), Error> {
        if v.as_bytes().contains(&0) {
            return Err(Error::Invalid);
        }
        self.u16(u16::try_from(v.len()).map_err(|_| Error::Invalid)?)?;
        self.put(v.as_bytes())
    }
    pub(crate) fn len(&self) -> usize {
        self.pos
    }
}
pub(crate) fn canonical(mask: u8, values: &[i128]) -> Result<(), Error> {
    if values
        .iter()
        .enumerate()
        .any(|(i, v)| mask & (1 << i) == 0 && *v != 0)
    {
        Err(Error::Invalid)
    } else {
        Ok(())
    }
}
pub(crate) fn ensure(ok: bool) -> Result<(), Error> {
    if ok { Ok(()) } else { Err(Error::Invalid) }
}
