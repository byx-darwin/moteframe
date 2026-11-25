//! Serialize one real producer append between actual admission and handler recheck.
//! The wrapper delegates every storage/query/auth operation unchanged.
use super::paged_service_support::{Store, input};
use device_history_service::{Directory, ReadPlan, RecordContent, RecordStore, StoreLease};
use device_history_service::{HistoryAccess, HistoryFailure};
use device_protocol::HistoryFilter;
use device_storage::StoredRecord;

#[derive(Debug)]
pub struct ProducerOnAdmission<'a> {
    pub store: Store<'a>,
    pub append_once: bool,
}
impl RecordStore for ProducerOnAdmission<'_> {
    fn directory(&self) -> Result<Directory<'_>, HistoryFailure> {
        self.store.directory()
    }
    fn open(
        &mut self,
        snapshot: u32,
        filter: HistoryFilter,
        now: u64,
        access: &dyn HistoryAccess,
    ) -> Result<StoreLease, HistoryFailure> {
        self.store.open(snapshot, filter, now, access)
    }
    fn plan(
        &mut self,
        lease: StoreLease,
        after: u64,
        max: u16,
        now: u64,
        access: &dyn HistoryAccess,
    ) -> Result<ReadPlan, HistoryFailure> {
        self.store.plan(lease, after, max, now, access)
    }
    fn read(
        &self,
        lease: StoreLease,
        id: u64,
        out: &mut [u8],
        access: &dyn HistoryAccess,
    ) -> Result<RecordContent, HistoryFailure> {
        self.store.read(lease, id, out, access)
    }
    fn validate_record(
        &self,
        lease: StoreLease,
        record: StoredRecord<'_>,
    ) -> Result<(), HistoryFailure> {
        self.store.validate_record(lease, record)
    }
    fn record_admitted(
        &mut self,
        lease: StoreLease,
        id: u64,
        access: &dyn HistoryAccess,
    ) -> Result<(), HistoryFailure> {
        self.store.record_admitted(lease, id, access)?;
        if self.append_once {
            self.append_once = false;
            self.store.append(input(&[99, 0, 0, 1]))?;
        }
        Ok(())
    }
    fn ack_atomic(
        &mut self,
        lease: StoreLease,
        filter: HistoryFilter,
        epoch: u32,
        upto: u64,
        now: u64,
        access: &dyn HistoryAccess,
    ) -> Result<u64, HistoryFailure> {
        self.store
            .ack_atomic(lease, filter, epoch, upto, now, access)
    }
    fn count(
        &self,
        filter: HistoryFilter,
        access: &dyn HistoryAccess,
    ) -> Result<(u32, u32), HistoryFailure> {
        self.store.count(filter, access)
    }
    fn renew(
        &mut self,
        lease: StoreLease,
        now: u64,
        access: &dyn HistoryAccess,
    ) -> Result<(), HistoryFailure> {
        self.store.renew(lease, now, access)
    }
    fn close(&mut self, lease: StoreLease) -> Result<(), HistoryFailure> {
        self.store.close(lease)
    }
}
