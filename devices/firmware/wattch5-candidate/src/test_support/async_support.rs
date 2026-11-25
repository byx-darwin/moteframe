#![allow(
    dead_code,
    clippy::expect_used,
    reason = "Test fixture assertions fail loudly; this module is cfg(test) only."
)]
use crate::support::Memory;
use core::task::Poll;
use device_property_store::{
    AsyncPropertyStore, DefaultValue, Error, ModuleCatalogue, MutationReceipt, MutationToken,
    PropertySchema,
};
use device_services::{PropertyDescriptor, PropertyFailure};
use device_storage::incremental::{
    AsyncMedium, Completion, IncrementalSnapshot, MediumOp, StartError,
};
use device_storage::persistence::{Error as StorageError, Geometry, Layout, Medium};
use std::vec::Vec;
use std::{cell::Cell, rc::Rc};
pub const LAYOUT: Layout = Layout {
    start: 0,
    bank_bytes: 1024,
};
pub const D: PropertyDescriptor = PropertyDescriptor {
    key: 1,
    access: 11,
    schema: 1,
    value_max: 8,
};
pub const CATALOGUE: &[ModuleCatalogue<'_>] = &[ModuleCatalogue {
    module: 4,
    properties: &[D],
}];
pub const DEFAULTS: &[DefaultValue<'_>] = &[DefaultValue {
    module: 4,
    key: 1,
    value: &[0],
}];
#[derive(Debug)]
pub struct Schema;
impl PropertySchema for Schema {
    fn descriptor(&self, m: u8, k: u16) -> Result<PropertyDescriptor, Error> {
        if (m, k) == (4, 1) {
            Ok(D)
        } else {
            Err(Error::Unavailable)
        }
    }
    fn validate(&self, m: u8, k: u16, b: &[u8]) -> Result<(), Error> {
        self.descriptor(m, k)?;
        if !b.is_empty() && b.len() <= 8 && b.iter().all(|n| *n <= 1) {
            Ok(())
        } else {
            Err(Error::InvalidValue)
        }
    }
}
#[derive(Clone, Debug)]
enum Owned {
    Read(u32, usize),
    Program(u32, Vec<u8>),
    Erase(u32),
    Sync,
}
#[derive(Clone, Debug)]
pub struct Nor {
    pub memory: Memory,
    actions: Rc<Cell<usize>>,
    pending: Option<Owned>,
    waited: bool,
    syncs: Rc<Cell<usize>>,
    fail_read_after_sync: Rc<Cell<Option<usize>>>,
    malformed_next: Rc<Cell<bool>>,
}
impl Nor {
    pub fn new() -> Self {
        Self {
            memory: Memory::new(),
            actions: Rc::new(Cell::new(0)),
            pending: None,
            waited: false,
            syncs: Rc::new(Cell::new(0)),
            fail_read_after_sync: Rc::new(Cell::new(None)),
            malformed_next: Rc::new(Cell::new(false)),
        }
    }
    pub fn actions(&self) -> usize {
        self.actions.get()
    }
    pub fn fail_final_readback(&self) {
        self.fail_read_after_sync.set(Some(self.syncs.get() + 3));
    }
    pub fn malformed_completion(&self) {
        self.malformed_next.set(true);
    }
}
impl AsyncMedium for Nor {
    fn geometry(&self) -> Geometry {
        self.memory.geometry()
    }
    fn begin(&mut self, op: MediumOp<'_>) -> Result<(), StartError> {
        self.actions.set(self.actions.get() + 1);
        if self.pending.is_some() {
            return Err(StartError::Busy);
        }
        self.pending = Some(match op {
            MediumOp::Read { offset, length } => Owned::Read(offset, length),
            MediumOp::Program { offset, bytes } => Owned::Program(offset, bytes.to_vec()),
            MediumOp::Erase { offset } => Owned::Erase(offset),
            MediumOp::Sync => Owned::Sync,
        });
        self.waited = false;
        Ok(())
    }
    fn poll(&mut self, out: &mut [u8]) -> Poll<Result<Completion, StorageError>> {
        self.actions.set(self.actions.get() + 1);
        if !self.waited {
            self.waited = true;
            return Poll::Pending;
        }
        let Some(op) = self.pending.take() else {
            return Poll::Ready(Err(StorageError::Io));
        };
        let result = match op {
            Owned::Read(offset, length) => {
                if self
                    .fail_read_after_sync
                    .get()
                    .is_some_and(|n| self.syncs.get() >= n)
                {
                    self.fail_read_after_sync.set(None);
                    Err(StorageError::Io)
                } else {
                    self.memory
                        .read(offset, &mut out[..length])
                        .map(|()| Completion::Read(length))
                }
            }
            Owned::Program(offset, bytes) => self
                .memory
                .program(offset, &bytes)
                .map(|()| Completion::Programmed),
            Owned::Erase(offset) => self
                .memory
                .erase(offset, self.memory.geometry().erase_size)
                .map(|()| Completion::Erased),
            Owned::Sync => {
                self.syncs.set(self.syncs.get() + 1);
                self.memory.sync().map(|()| Completion::Synced)
            }
        };
        if self.malformed_next.replace(false) {
            Poll::Ready(result.map(|_| Completion::Read(0)))
        } else {
            Poll::Ready(result)
        }
    }
}
impl Medium for Nor {
    fn geometry(&self) -> Geometry {
        self.memory.geometry()
    }
    fn read(&mut self, offset: u32, out: &mut [u8]) -> Result<(), StorageError> {
        self.memory.read(offset, out)
    }
    fn program(&mut self, offset: u32, bytes: &[u8]) -> Result<(), StorageError> {
        self.memory.program(offset, bytes)
    }
    fn erase(&mut self, offset: u32, length: u32) -> Result<(), StorageError> {
        self.memory.erase(offset, length)
    }
    fn sync(&mut self) -> Result<(), StorageError> {
        self.memory.sync()
    }
}
pub type Owner = AsyncPropertyStore<'static, Nor, Schema, 128>;
pub fn engine(nor: Nor) -> IncrementalSnapshot<Nor, 128> {
    let mut engine = IncrementalSnapshot::new_unrecovered(nor, LAYOUT).expect("engine");
    engine.begin_recover().expect("recover");
    for _ in 0..2000 {
        if engine.poll_recover().is_ready() {
            return engine;
        }
    }
    panic!("recovery never completed");
}
pub fn initialized(nor: Nor) -> Owner {
    let owner = Owner::open(engine(nor), CATALOGUE, Schema).expect("owner");
    let token = owner.begin_initialize(DEFAULTS).expect("initialize");
    settle(&owner, token).expect("initialize durable");
    owner.take_result(token).expect("token").expect("result");
    owner
}
pub fn settle<M: AsyncMedium, C: PropertySchema, const N: usize>(
    owner: &AsyncPropertyStore<'_, M, C, N>,
    token: MutationToken,
) -> Result<MutationReceipt, PropertyFailure> {
    for _ in 0..4000 {
        if let Poll::Ready(result) = owner.result(token).expect("token") {
            return result;
        }
        let _ = owner.step();
    }
    panic!("commit never completed");
}
