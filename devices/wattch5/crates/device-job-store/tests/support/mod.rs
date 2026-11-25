//! Test-only NOR medium and explicitly trusted host fixtures.
#![allow(
    dead_code,
    reason = "Integration suites select different fixture controls."
)]
use core::cell::{Cell, RefCell};
use device_job_store::{JobStore, ResetIsolation, TrustedBoot, TrustedJobPolicy};
use device_maintenance_service::{DeviceJobAccess, DeviceJobRecord, Error, NamedRequest};
use device_storage::persistence::{
    AtomicSnapshot, Error as StorageError, Geometry, Layout, Medium,
};
use std::rc::Rc;

pub fn must<T, E: core::fmt::Debug>(result: Result<T, E>) -> T {
    result.unwrap_or_else(|error| panic!("fixture failed: {error:?}"))
}
#[derive(Clone, Debug)]
pub struct Boot(pub Rc<Cell<Result<u64, Error>>>);
impl Boot {
    pub fn new() -> Self {
        Self(Rc::new(Cell::new(Ok(1))))
    }
}
impl TrustedBoot for Boot {
    fn current_boot(&self) -> Result<u64, Error> {
        self.0.get()
    }
}
#[derive(Clone, Debug)]
pub struct Policy {
    pub revision: Rc<Cell<Result<u16, Error>>>,
    pub allowed: Rc<Cell<bool>>,
    pub historical: Rc<Cell<bool>>,
}
impl Policy {
    pub fn new() -> Self {
        Self {
            revision: Rc::new(Cell::new(Ok(1))),
            allowed: Rc::new(Cell::new(true)),
            historical: Rc::new(Cell::new(true)),
        }
    }
}
impl TrustedJobPolicy for Policy {
    fn supports(&self, op: u16) -> bool {
        self.allowed.get() && matches!(op, 1 | 2)
    }
    fn current_revision(&self) -> Result<u16, Error> {
        self.revision.get()
    }
    fn permits(&self, _: [u8; 16], op: u16, captured: u16) -> bool {
        self.supports(op) && (self.historical.get() || self.revision.get() == Ok(captured))
    }
}
#[derive(Clone, Debug)]
pub struct Isolation(pub Rc<Cell<bool>>);
impl Isolation {
    pub fn new() -> Self {
        Self(Rc::new(Cell::new(true)))
    }
}
impl ResetIsolation for Isolation {
    fn preserves_ledger(&self, _: u16) -> bool {
        self.0.get()
    }
}
#[derive(Debug)]
pub struct Access {
    pub identity: Cell<Option<[u8; 16]>>,
    pub start: Cell<bool>,
    pub danger: Cell<bool>,
    pub read: Cell<bool>,
    pub foreign: Cell<bool>,
    pub deny_own: Cell<bool>,
}
impl Access {
    pub fn new(principal: u8) -> Self {
        Self {
            identity: Cell::new(Some([principal; 16])),
            start: Cell::new(true),
            danger: Cell::new(true),
            read: Cell::new(true),
            foreign: Cell::new(false),
            deny_own: Cell::new(false),
        }
    }
}
impl DeviceJobAccess for Access {
    fn identity(&self) -> Option<[u8; 16]> {
        self.identity.get()
    }
    fn start_allowed(&self, _: u16) -> bool {
        self.start.get()
    }
    fn dangerous_allowed(&self) -> bool {
        self.danger.get()
    }
    fn read_scope(&self) -> bool {
        self.read.get()
    }
    fn may_read(&self, r: DeviceJobRecord) -> bool {
        if Some(r.principal) == self.identity.get() {
            !self.deny_own.get()
        } else {
            self.foreign.get()
        }
    }
}
pub fn request(id: u8, operation: u16) -> NamedRequest<'static> {
    NamedRequest {
        operation_id: [id; 16],
        operation,
        params: &[1, 0],
    }
}
#[derive(Clone, Debug)]
pub struct Nor(pub Rc<RefCell<Media>>);
#[derive(Clone, Debug)]
pub struct Media {
    pub bytes: [u8; 16384],
    pub effects: usize,
    pub syncs: usize,
    pub cut: Option<usize>,
    pub sync_cut: Option<usize>,
    pub read_fail: bool,
}
impl Nor {
    pub fn blank() -> Self {
        Self(Rc::new(RefCell::new(Media {
            bytes: [255; 16384],
            effects: 0,
            syncs: 0,
            cut: None,
            sync_cut: None,
            read_fail: false,
        })))
    }
    pub fn heal(&self) {
        let mut m = self.0.borrow_mut();
        m.effects = 0;
        m.syncs = 0;
        m.cut = None;
        m.sync_cut = None;
        m.read_fail = false;
    }
    pub fn duplicate(&self) -> Self {
        Self(Rc::new(RefCell::new(self.0.borrow().clone())))
    }
}
impl Medium for Nor {
    fn geometry(&self) -> Geometry {
        Geometry {
            capacity: 16384,
            erase_size: 4096,
            write_size: 4,
        }
    }
    fn read(&mut self, offset: u32, out: &mut [u8]) -> Result<(), StorageError> {
        let m = self.0.borrow();
        if m.read_fail {
            return Err(StorageError::Io);
        }
        let at = must(usize::try_from(offset));
        out.copy_from_slice(&m.bytes[at..at + out.len()]);
        Ok(())
    }
    fn erase(&mut self, offset: u32, length: u32) -> Result<(), StorageError> {
        assert_eq!(offset % 4096, 0);
        assert_eq!(length % 4096, 0);
        let mut m = self.0.borrow_mut();
        for at in must(usize::try_from(offset))..must(usize::try_from(offset + length)) {
            if m.cut == Some(m.effects) {
                return Err(StorageError::Io);
            }
            m.effects += 1;
            m.bytes[at] = 255;
        }
        Ok(())
    }
    fn program(&mut self, offset: u32, bytes: &[u8]) -> Result<(), StorageError> {
        assert_eq!(offset % 4, 0);
        assert_eq!(bytes.len() % 4, 0);
        let mut m = self.0.borrow_mut();
        let at = must(usize::try_from(offset));
        for (i, b) in bytes.iter().enumerate() {
            if m.cut == Some(m.effects) {
                return Err(StorageError::Io);
            }
            m.effects += 1;
            assert_eq!(m.bytes[at + i] & b, *b);
            m.bytes[at + i] = *b;
        }
        Ok(())
    }
    fn sync(&mut self) -> Result<(), StorageError> {
        let mut m = self.0.borrow_mut();
        let n = m.syncs;
        m.syncs += 1;
        if m.sync_cut == Some(n) {
            Err(StorageError::Io)
        } else {
            Ok(())
        }
    }
}
pub type Store = AtomicSnapshot<Nor, 4096>;
pub type Ledger = JobStore<Store, Boot, Policy, Isolation>;
pub fn snapshot(media: Nor) -> Store {
    must(Store::open(
        media,
        Layout {
            start: 0,
            bank_bytes: 8192,
        },
    ))
}
pub fn open(media: Nor, boot: Boot, policy: Policy, isolation: Isolation) -> Ledger {
    must(JobStore::open(snapshot(media), boot, policy, isolation))
}
pub fn initialized() -> Ledger {
    let mut l = open(Nor::blank(), Boot::new(), Policy::new(), Isolation::new());
    must(l.initialize());
    l
}
