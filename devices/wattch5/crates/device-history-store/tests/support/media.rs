//! Test-only logical medium; never linked into production.
use core::cell::{Cell, RefCell};
use device_storage::persistence::{Error, Geometry, Medium};
use std::rc::Rc;
#[derive(Clone, Debug)]
pub struct TestMedium {
    pub bytes: Rc<RefCell<Vec<u8>>>,
    pub syncs: Rc<Cell<u32>>,
    pub fail_sync: Rc<Cell<Option<u32>>>,
}
impl TestMedium {
    pub fn blank() -> Self {
        Self {
            bytes: Rc::new(RefCell::new(vec![0xff; 8192])),
            syncs: Rc::new(Cell::new(0)),
            fail_sync: Rc::new(Cell::new(None)),
        }
    }
}
impl Medium for TestMedium {
    fn geometry(&self) -> Geometry {
        Geometry {
            capacity: 8192,
            erase_size: 256,
            write_size: 8,
        }
    }
    fn read(&mut self, offset: u32, out: &mut [u8]) -> Result<(), Error> {
        let at = usize::try_from(offset).map_err(|_| Error::Io)?;
        let bytes = self.bytes.borrow();
        let end = at.checked_add(out.len()).ok_or(Error::Io)?;
        out.copy_from_slice(bytes.get(at..end).ok_or(Error::Io)?);
        Ok(())
    }
    fn erase(&mut self, offset: u32, length: u32) -> Result<(), Error> {
        if !offset.is_multiple_of(256) || !length.is_multiple_of(256) {
            return Err(Error::Io);
        }
        let at = usize::try_from(offset).map_err(|_| Error::Io)?;
        let end =
            usize::try_from(offset.checked_add(length).ok_or(Error::Io)?).map_err(|_| Error::Io)?;
        self.bytes
            .borrow_mut()
            .get_mut(at..end)
            .ok_or(Error::Io)?
            .fill(0xff);
        Ok(())
    }
    fn program(&mut self, offset: u32, input: &[u8]) -> Result<(), Error> {
        if !offset.is_multiple_of(8) || !input.len().is_multiple_of(8) {
            return Err(Error::Io);
        }
        let at = usize::try_from(offset).map_err(|_| Error::Io)?;
        let end = at.checked_add(input.len()).ok_or(Error::Io)?;
        let mut bytes = self.bytes.borrow_mut();
        for (a, b) in bytes
            .get_mut(at..end)
            .ok_or(Error::Io)?
            .iter_mut()
            .zip(input)
        {
            if *a & *b != *b {
                return Err(Error::Io);
            }
            *a &= *b;
        }
        Ok(())
    }
    fn sync(&mut self) -> Result<(), Error> {
        let n = self.syncs.get() + 1;
        self.syncs.set(n);
        if self.fail_sync.get() == Some(n) {
            self.fail_sync.set(None);
            return Err(Error::Io);
        }
        Ok(())
    }
}
