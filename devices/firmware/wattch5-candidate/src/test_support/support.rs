#![allow(dead_code)]
use device_storage::persistence::{Error, Geometry, Medium, Snapshot, SnapshotStore};
use std::vec;
use std::vec::Vec;
use std::{cell::RefCell, rc::Rc};
#[derive(Clone, Debug)]
pub struct Memory(Rc<RefCell<Disk>>);
#[derive(Debug)]
struct Disk {
    bytes: Vec<u8>,
    mutations: usize,
    syncs: usize,
    sync_fail: Option<usize>,
    program_cut: Option<usize>,
    erase_cut: Option<usize>,
    read_fail: bool,
}
impl Memory {
    pub fn new() -> Self {
        Self(Rc::new(RefCell::new(Disk {
            bytes: vec![0xff; 8192],
            mutations: 0,
            syncs: 0,
            sync_fail: None,
            program_cut: None,
            erase_cut: None,
            read_fail: false,
        })))
    }
    pub fn mutations(&self) -> usize {
        self.0.borrow().mutations
    }
    pub fn arm_sync(&self, index: usize) {
        let mut d = self.0.borrow_mut();
        d.syncs = 0;
        d.sync_fail = Some(index);
    }
    pub fn arm_program(&self, bytes: usize) {
        self.0.borrow_mut().program_cut = Some(bytes);
    }
    pub fn arm_erase(&self, bytes: usize) {
        self.0.borrow_mut().erase_cut = Some(bytes);
    }
    pub fn fail_reads(&self, fail: bool) {
        self.0.borrow_mut().read_fail = fail;
    }
    pub fn disarm(&self) {
        let mut d = self.0.borrow_mut();
        d.sync_fail = None;
        d.program_cut = None;
        d.erase_cut = None;
        d.read_fail = false;
    }
    pub fn corrupt(&self, offset: usize) {
        self.0.borrow_mut().bytes[offset] ^= 1;
    }
}
impl Medium for Memory {
    fn geometry(&self) -> Geometry {
        Geometry {
            capacity: 8192,
            erase_size: 256,
            write_size: 1,
        }
    }
    fn read(&mut self, offset: u32, out: &mut [u8]) -> Result<(), Error> {
        let disk = self.0.borrow();
        if disk.read_fail {
            return Err(Error::Io);
        }
        out.copy_from_slice(
            disk.bytes
                .get(offset as usize..offset as usize + out.len())
                .ok_or(Error::Io)?,
        );
        Ok(())
    }
    fn erase(&mut self, offset: u32, length: u32) -> Result<(), Error> {
        let mut disk = self.0.borrow_mut();
        disk.mutations += 1;
        let allowed = disk
            .erase_cut
            .map_or(length as usize, |n| n.min(length as usize));
        let fail = disk.erase_cut.is_some_and(|n| n <= length as usize);
        disk.erase_cut = disk.erase_cut.map(|n| n.saturating_sub(length as usize));
        disk.bytes
            .get_mut(offset as usize..offset as usize + allowed)
            .ok_or(Error::Io)?
            .fill(0xff);
        if fail { Err(Error::Io) } else { Ok(()) }
    }
    fn program(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Error> {
        let mut disk = self.0.borrow_mut();
        disk.mutations += 1;
        let allowed = disk.program_cut.map_or(bytes.len(), |n| n.min(bytes.len()));
        let fail = disk.program_cut.is_some_and(|n| n <= bytes.len());
        disk.program_cut = disk.program_cut.map(|n| n.saturating_sub(bytes.len()));
        let target = disk
            .bytes
            .get_mut(offset as usize..offset as usize + allowed)
            .ok_or(Error::Io)?;
        for (old, value) in target.iter_mut().zip(bytes) {
            if *old & value != *value {
                return Err(Error::Io);
            }
            *old &= value;
        }
        if fail { Err(Error::Io) } else { Ok(()) }
    }
    fn sync(&mut self) -> Result<(), Error> {
        let mut d = self.0.borrow_mut();
        let index = d.syncs;
        d.syncs += 1;
        if d.sync_fail == Some(index) {
            Err(Error::Io)
        } else {
            Ok(())
        }
    }
}
#[derive(Clone, Debug, Default)]
pub struct Controlled(pub Rc<RefCell<Control>>);
#[derive(Debug, Default)]
pub struct Control {
    pub bytes: Vec<u8>,
    pub revision: u64,
    pub commit_error: Option<Error>,
    pub ack_revision: Option<u64>,
    pub ack_length: Option<usize>,
    pub load_length: Option<usize>,
    pub recover_error: Option<Error>,
    pub load_error: Option<Error>,
    pub commits: usize,
}
impl SnapshotStore for Controlled {
    fn load(&self, out: &mut [u8]) -> Result<Option<Snapshot>, Error> {
        let c = self.0.borrow();
        if let Some(e) = c.load_error {
            return Err(e);
        }
        if c.revision == 0 {
            return Ok(None);
        }
        if out.len() < c.bytes.len() {
            return Err(Error::Capacity);
        }
        out[..c.bytes.len()].copy_from_slice(&c.bytes);
        Ok(Some(Snapshot {
            revision: c.revision,
            length: c.load_length.unwrap_or(c.bytes.len()),
        }))
    }
    fn commit(&mut self, expected: u64, bytes: &[u8]) -> Result<Snapshot, Error> {
        let mut c = self.0.borrow_mut();
        c.commits += 1;
        if let Some(e) = c.commit_error {
            return Err(e);
        }
        if c.revision != expected {
            return Err(Error::Conflict);
        }
        c.revision = c.revision.checked_add(1).ok_or(Error::RevisionExhausted)?;
        c.bytes = bytes.to_vec();
        Ok(Snapshot {
            revision: c.ack_revision.unwrap_or(c.revision),
            length: c.ack_length.unwrap_or(bytes.len()),
        })
    }
    fn recover(&mut self) -> Result<(), Error> {
        self.0.borrow().recover_error.map_or(Ok(()), Err)
    }
}
