//! Checked, exclusive physical-medium ownership and disjoint relative views.
use core::cell::RefCell;
use device_storage::persistence::{Error, Geometry, Layout, Medium};
/// Explicit disjoint physical ranges; no board partition is implied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MediaLayout {
    /// Two metadata banks.
    pub metadata: Layout,
    /// Absolute log start.
    pub log_start: u32,
    /// Log capacity.
    pub log_bytes: u32,
}
/// One physical owner; views cannot outlive its exclusive borrow.
pub struct PagedMedia<M> {
    medium: RefCell<M>,
    geometry: Geometry,
    layout: MediaLayout,
}
impl<M> core::fmt::Debug for PagedMedia<M> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PagedMedia")
            .field("geometry", &self.geometry)
            .field("layout", &self.layout)
            .finish_non_exhaustive()
    }
}
/// Nonclone, checked relative view into the single physical owner.
pub struct MediaView<'a, M> {
    medium: &'a RefCell<M>,
    geometry: Geometry,
    start: u32,
    log: bool,
}
impl<M> core::fmt::Debug for MediaView<'_, M> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("MediaView")
            .field("start", &self.start)
            .field("geometry", &self.geometry)
            .field("log", &self.log)
            .finish_non_exhaustive()
    }
}
impl<M: Medium> PagedMedia<M> {
    /// Validate all physical ranges before granting views.
    /// # Errors
    /// Invalid, overlapping, overflowing or misaligned ranges are rejected.
    pub fn new(medium: M, layout: MediaLayout) -> Result<Self, Error> {
        let g = medium.geometry();
        let m = layout.metadata;
        let end = m
            .bank_bytes
            .checked_mul(2)
            .and_then(|n| m.start.checked_add(n))
            .ok_or(Error::InvalidLayout)?;
        let le = layout
            .log_start
            .checked_add(layout.log_bytes)
            .ok_or(Error::InvalidLayout)?;
        if g.erase_size == 0
            || g.write_size == 0
            || g.write_size > 256
            || !g.erase_size.is_multiple_of(g.write_size)
            || m.bank_bytes == 0
            || layout.log_bytes == 0
            || end > g.capacity
            || le > g.capacity
            || !(end <= layout.log_start || le <= m.start)
            || [m.start, m.bank_bytes, layout.log_start, layout.log_bytes]
                .iter()
                .any(|n| !n.is_multiple_of(g.erase_size) || !n.is_multiple_of(g.write_size))
        {
            return Err(Error::InvalidLayout);
        }
        Ok(Self {
            medium: RefCell::new(medium),
            geometry: g,
            layout,
        })
    }
    /// Borrow the disjoint metadata and log views, both relative to zero.
    pub fn split(&mut self) -> (MediaView<'_, M>, MediaView<'_, M>) {
        let g = self.geometry;
        let m = self.layout.metadata;
        (
            MediaView {
                medium: &self.medium,
                geometry: Geometry {
                    capacity: m.bank_bytes * 2,
                    ..g
                },
                start: m.start,
                log: false,
            },
            MediaView {
                medium: &self.medium,
                geometry: Geometry {
                    capacity: self.layout.log_bytes,
                    ..g
                },
                start: self.layout.log_start,
                log: true,
            },
        )
    }
    /// Return the exclusively owned physical medium.
    pub fn into_medium(self) -> M {
        self.medium.into_inner()
    }
}
impl<M> MediaView<'_, M> {
    fn offset(&self, offset: u32, length: usize) -> Result<u32, Error> {
        let n = u32::try_from(length).map_err(|_| Error::InvalidLayout)?;
        if offset
            .checked_add(n)
            .is_none_or(|end| end > self.geometry.capacity)
        {
            return Err(Error::InvalidLayout);
        }
        self.start.checked_add(offset).ok_or(Error::InvalidLayout)
    }
}
impl<M: Medium> Medium for MediaView<'_, M> {
    fn geometry(&self) -> Geometry {
        self.geometry
    }
    fn read(&mut self, o: u32, out: &mut [u8]) -> Result<(), Error> {
        let at = self.offset(o, out.len())?;
        let mut m = self.medium.try_borrow_mut().map_err(|_| Error::Io)?;
        for (i, c) in out.chunks_mut(256).enumerate() {
            m.read(
                at + u32::try_from(i * 256).map_err(|_| Error::InvalidLayout)?,
                c,
            )?;
        }
        Ok(())
    }
    fn erase(&mut self, o: u32, n: u32) -> Result<(), Error> {
        if self.log {
            return Err(Error::InvalidLayout);
        }
        let at = self.offset(o, n as usize)?;
        if !o.is_multiple_of(self.geometry.erase_size)
            || !n.is_multiple_of(self.geometry.erase_size)
        {
            return Err(Error::InvalidLayout);
        }
        self.medium
            .try_borrow_mut()
            .map_err(|_| Error::Io)?
            .erase(at, n)
    }
    fn program(&mut self, o: u32, b: &[u8]) -> Result<(), Error> {
        let at = self.offset(o, b.len())?;
        if !o.is_multiple_of(self.geometry.write_size)
            || !u32::try_from(b.len())
                .map_err(|_| Error::InvalidLayout)?
                .is_multiple_of(self.geometry.write_size)
            || b.len() > 256
        {
            return Err(Error::InvalidLayout);
        }
        self.medium
            .try_borrow_mut()
            .map_err(|_| Error::Io)?
            .program(at, b)
    }
    fn sync(&mut self) -> Result<(), Error> {
        self.medium.try_borrow_mut().map_err(|_| Error::Io)?.sync()
    }
}
#[cfg(test)]
mod tests {
    use super::{MediaLayout, PagedMedia};
    use device_storage::persistence::{Error, Geometry, Layout, Medium};
    #[derive(Debug)]
    struct Reject;
    impl Medium for Reject {
        fn geometry(&self) -> Geometry {
            Geometry {
                capacity: 12288,
                erase_size: 4096,
                write_size: 1,
            }
        }
        fn read(&mut self, _: u32, _: &mut [u8]) -> Result<(), Error> {
            Err(Error::Io)
        }
        fn program(&mut self, _: u32, _: &[u8]) -> Result<(), Error> {
            Err(Error::Io)
        }
        fn erase(&mut self, _: u32, _: u32) -> Result<(), Error> {
            Err(Error::Io)
        }
        fn sync(&mut self) -> Result<(), Error> {
            Err(Error::Io)
        }
    }
    #[test]
    fn test_should_return_explicit_error_on_shared_medium_reentrancy_and_bad_ranges() {
        let mut owner = PagedMedia::new(
            Reject,
            MediaLayout {
                metadata: Layout {
                    start: 0,
                    bank_bytes: 4096,
                },
                log_start: 8192,
                log_bytes: 4096,
            },
        )
        .unwrap_or_else(|e| panic!("{e:?}"));
        let (meta, mut log) = owner.split();
        let held = meta
            .medium
            .try_borrow_mut()
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(log.sync(), Err(Error::Io));
        drop(held);
        assert_eq!(log.read(4096, &mut [0; 1]), Err(Error::InvalidLayout));
        assert_eq!(log.program(u32::MAX, &[0]), Err(Error::InvalidLayout));
        assert_eq!(log.erase(0, 4096), Err(Error::InvalidLayout));
    }
}
