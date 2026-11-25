//! Host-only real snapshot medium and trusted clock inputs.
#![allow(
    dead_code,
    reason = "Each integration test selects the relevant fixture controls."
)]

use core::cell::{Cell, RefCell};
use device_services::{ClockSourceError, TimeAccess};
use device_storage::persistence::{AtomicSnapshot, Error, Geometry, Layout, Medium};
use device_time_store::{ClockReading, TrustedClock};
use std::rc::Rc;

pub fn must<T, E: core::fmt::Debug>(result: Result<T, E>) -> T {
    result.unwrap_or_else(|error| panic!("fixture operation failed: {error:?}"))
}

#[derive(Clone, Debug)]
pub struct Source(pub Rc<Cell<Result<ClockReading, ClockSourceError>>>);
impl Source {
    pub fn new(mono_us: u64) -> Self {
        Self(Rc::new(Cell::new(Ok(ClockReading {
            source_id: 11,
            epoch: 22,
            mono_us,
        }))))
    }
    pub fn at(&self, mono_us: u64) {
        self.0.set(Ok(ClockReading {
            source_id: 11,
            epoch: 22,
            mono_us,
        }));
    }
}
impl TrustedClock for Source {
    fn reading(&self) -> Result<ClockReading, ClockSourceError> {
        self.0.get()
    }
}
#[derive(Debug)]
pub struct Access {
    pub read: Cell<bool>,
    pub set: Cell<bool>,
}
impl Access {
    pub fn all() -> Self {
        Self {
            read: Cell::new(true),
            set: Cell::new(true),
        }
    }
}
impl TimeAccess for Access {
    fn allowed(&self, set: bool) -> bool {
        if set { self.set.get() } else { self.read.get() }
    }
}

#[derive(Clone, Debug)]
pub struct Nor(pub Rc<RefCell<Media>>);
#[derive(Debug)]
pub struct Media {
    pub bytes: [u8; 512],
    pub effects: usize,
    pub syncs: usize,
    pub cut: Option<usize>,
    pub sync_cut: Option<usize>,
    pub read_fail: bool,
}
impl Nor {
    pub fn blank() -> Self {
        Self(Rc::new(RefCell::new(Media {
            bytes: [255; 512],
            effects: 0,
            syncs: 0,
            cut: None,
            sync_cut: None,
            read_fail: false,
        })))
    }
    pub fn heal(&self) {
        let mut media = self.0.borrow_mut();
        media.effects = 0;
        media.syncs = 0;
        media.cut = None;
        media.sync_cut = None;
        media.read_fail = false;
    }
}
impl Medium for Nor {
    fn geometry(&self) -> Geometry {
        Geometry {
            capacity: 512,
            erase_size: 64,
            write_size: 4,
        }
    }
    fn read(&mut self, offset: u32, out: &mut [u8]) -> Result<(), Error> {
        let media = self.0.borrow();
        if media.read_fail {
            return Err(Error::Io);
        }
        let offset = must(usize::try_from(offset));
        out.copy_from_slice(&media.bytes[offset..offset + out.len()]);
        Ok(())
    }
    fn erase(&mut self, offset: u32, length: u32) -> Result<(), Error> {
        assert_eq!(offset % 64, 0);
        assert_eq!(length % 64, 0);
        let mut media = self.0.borrow_mut();
        for at in must(usize::try_from(offset))..must(usize::try_from(offset + length)) {
            if media.cut == Some(media.effects) {
                return Err(Error::Io);
            }
            media.effects += 1;
            media.bytes[at] = 255;
        }
        Ok(())
    }
    fn program(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Error> {
        assert_eq!(offset % 4, 0);
        assert_eq!(bytes.len() % 4, 0);
        let mut media = self.0.borrow_mut();
        let offset = must(usize::try_from(offset));
        for (relative, byte) in bytes.iter().enumerate() {
            if media.cut == Some(media.effects) {
                return Err(Error::Io);
            }
            media.effects += 1;
            assert_eq!(media.bytes[offset + relative] & byte, *byte);
            media.bytes[offset + relative] = *byte;
        }
        Ok(())
    }
    fn sync(&mut self) -> Result<(), Error> {
        let mut media = self.0.borrow_mut();
        let at = media.syncs;
        media.syncs += 1;
        if media.sync_cut == Some(at) {
            Err(Error::Io)
        } else {
            Ok(())
        }
    }
}
pub type Store = AtomicSnapshot<Nor, 64>;
pub fn store(media: Nor) -> Store {
    must(Store::open(
        media,
        Layout {
            start: 0,
            bank_bytes: 256,
        },
    ))
}
