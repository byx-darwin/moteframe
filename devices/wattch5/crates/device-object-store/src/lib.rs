//! Durable, bounded object identity and repository storage.
//!
//! One owner serializes one snapshot partition. Integrators supply the actual
//! schema and live access providers; no resource is supplied by default.
#![no_std]
#![forbid(unsafe_code)]

mod image;
mod owner;
mod repository;
pub use owner::{DurableObjects, Error};

use device_object_service::Birth;
use device_storage::Begin;

#[derive(Debug, Clone, Copy)]
struct Module {
    id: u8,
    high: u32,
    revision: u32,
}
#[derive(Clone, Copy)]
struct Entry<const B: usize> {
    begin: Begin,
    birth: Birth,
    revision: u32,
    total: u32,
    crc: u32,
    alive: bool,
    body: [u8; B],
}
#[derive(Clone, Copy)]
struct State<const M: usize, const E: usize, const B: usize> {
    modules: [Option<Module>; M],
    entries: [Option<Entry<B>>; E],
}
impl<const M: usize, const E: usize, const B: usize> State<M, E, B> {
    fn empty() -> Self {
        Self {
            modules: [None; M],
            entries: [None; E],
        }
    }
    fn module(&self, id: u8) -> Result<usize, device_storage::StoreError> {
        self.modules
            .iter()
            .position(|m| m.is_some_and(|m| m.id == id))
            .ok_or(device_storage::StoreError::Invalid)
    }
    fn entry(&self, module: u8, id: u32) -> Result<usize, device_storage::StoreError> {
        self.entries
            .iter()
            .position(|e| e.is_some_and(|e| e.begin.module == module && e.begin.object_id == id))
            .ok_or(device_storage::StoreError::NotFound)
    }
}

impl<const B: usize> core::fmt::Debug for Entry<B> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Entry")
            .field("encoded_length", &self.total)
            .field("alive", &self.alive)
            .finish_non_exhaustive()
    }
}
impl<const M: usize, const E: usize, const B: usize> core::fmt::Debug for State<M, E, B> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("State")
            .field("modules", &M)
            .field("reservations", &E)
            .finish_non_exhaustive()
    }
}
