//! Core dataset identity and history commands borrow one checked actual owner.
#![cfg(feature = "history")]
#![allow(clippy::unwrap_used)]
use core::{cell::Cell, num::NonZeroU64};
use device_application::{CreditPort, DatasetPort, HistoryPort, ServiceCell};
use device_history_service::*;
use device_protocol::{HistoryFilter, ModuleEntry, SharedDataLane};
use device_services::{ResourceFailure, ServiceHandler};
use device_storage::StoredRecord;
#[derive(Debug)]
struct Access(Cell<bool>);
impl HistoryAccess for Access {
    fn module_allowed(&self) -> bool {
        self.0.get()
    }
    fn allowed(&self, _: [u8; 16], _: Option<u8>, _: HistoryOperation) -> bool {
        self.0.get()
    }
}
#[derive(Debug)]
struct Store<'a>(&'a Cell<[u8; 16]>);
impl RecordStore for Store<'_> {
    fn directory(&self) -> Result<Directory<'_>, HistoryFailure> {
        Ok(Directory {
            dataset: self.0.get(),
            domains: &[],
        })
    }
    fn open(
        &mut self,
        _: u32,
        _: HistoryFilter,
        _: u64,
        _: &dyn HistoryAccess,
    ) -> Result<StoreLease, HistoryFailure> {
        Err(HistoryFailure::Storage)
    }
    fn plan(
        &mut self,
        _: StoreLease,
        _: u64,
        _: u16,
        _: u64,
        _: &dyn HistoryAccess,
    ) -> Result<ReadPlan, HistoryFailure> {
        Err(HistoryFailure::Storage)
    }
    fn read(
        &self,
        _: StoreLease,
        _: u64,
        _: &mut [u8],
        _: &dyn HistoryAccess,
    ) -> Result<RecordContent, HistoryFailure> {
        Err(HistoryFailure::Storage)
    }
    fn validate_record(&self, _: StoreLease, _: StoredRecord<'_>) -> Result<(), HistoryFailure> {
        Err(HistoryFailure::Storage)
    }
    fn record_admitted(
        &mut self,
        _: StoreLease,
        _: u64,
        _: &dyn HistoryAccess,
    ) -> Result<(), HistoryFailure> {
        Err(HistoryFailure::Storage)
    }
    fn ack_atomic(
        &mut self,
        _: StoreLease,
        _: HistoryFilter,
        _: u32,
        _: u64,
        _: u64,
        _: &dyn HistoryAccess,
    ) -> Result<u64, HistoryFailure> {
        Err(HistoryFailure::Storage)
    }
    fn count(&self, _: HistoryFilter, _: &dyn HistoryAccess) -> Result<(u32, u32), HistoryFailure> {
        Err(HistoryFailure::Storage)
    }
    fn renew(
        &mut self,
        _: StoreLease,
        _: u64,
        _: &dyn HistoryAccess,
    ) -> Result<(), HistoryFailure> {
        Err(HistoryFailure::Storage)
    }
    fn close(&mut self, _: StoreLease) -> Result<(), HistoryFailure> {
        Err(HistoryFailure::Storage)
    }
}
#[test]
fn test_should_share_live_dataset_owner_and_distinguish_missing_stream_from_denied_scope() {
    let identity = Cell::new([7; 16]);
    let mut store = Store(&identity);
    let access = Access(Cell::new(true));
    let lane = SharedDataLane::new(NonZeroU64::new(1).unwrap());
    let metadata = ModuleEntry {
        module: 0x10,
        major: 1,
        minor: 2,
        availability: 1,
        features: 0,
        input_max: 128,
        output_max: 128,
    };
    let service =
        HistoryHandler::<_, 1, 16>::new(metadata, &mut store, &access, &lane, 128, true).unwrap();
    let owner = ServiceCell::new(service);
    let port = HistoryPort::new(&owner);
    assert!(CreditPort::authorized(&port));
    assert_eq!(port.validate(1, 1), Err(ResourceFailure::InvalidState));
    assert_eq!(port.identity().map(|id| *id.bytes()), Ok([7; 16]));
    access.0.set(false);
    assert!(!DatasetPort::authorized(&port));
    assert_eq!(port.identity(), Err(ResourceFailure::AccessDenied));
    access.0.set(true);
    identity.set([8; 16]);
    assert!(port.identity().is_err());
    let mut handle = owner.handle();
    assert_eq!(handle.connection_closed(), Ok(()));
    assert!(port.identity().is_err());
}
