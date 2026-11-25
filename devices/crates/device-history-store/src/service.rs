use crate::{HistoryClock, HistorySchema, HistoryStore, Lease, repository::matches};
use device_history_service::{
    Directory, HistoryAccess, HistoryFailure, HistoryOperation, ReadPlan, RecordContent,
    RecordStore, StoreLease,
};
use device_protocol::{HistoryFilter, HistorySnapshot};
use device_storage::{CommitState, StoredRecord, persistence::SnapshotStore};

impl<
    S: SnapshotStore,
    V: HistorySchema,
    C: HistoryClock,
    const D: usize,
    const R: usize,
    const B: usize,
    const I: usize,
> RecordStore for HistoryStore<S, V, C, D, R, B, I>
{
    fn directory(&self) -> Result<Directory<'_>, HistoryFailure> {
        self.ready()?;
        Ok(Directory {
            dataset: self.data.dataset,
            domains: &self.data.domains[..self.data.domains_len],
        })
    }
    fn open(
        &mut self,
        snapshot: u32,
        filter: HistoryFilter,
        now_ms: u64,
        access: &dyn HistoryAccess,
    ) -> Result<StoreLease, HistoryFailure> {
        let d = self.check_filter(filter, access)?;
        let now = self.observe(Some(now_ms))?;
        if snapshot == 0 {
            return Err(HistoryFailure::Invalid);
        }
        if self.lease.get().is_some_and(|l| now < l.deadline) {
            return Err(HistoryFailure::Busy);
        }
        self.release();
        let deadline = now
            .checked_add(u64::from(self.lease_s) * 1000)
            .ok_or(HistoryFailure::Clock)?;
        let handle = self.next_handle;
        let next = handle.checked_add(1).ok_or(HistoryFailure::Capacity)?;
        let mut ids = [0; R];
        let mut count = 0;
        for r in &self.data.records[..self.data.records_len] {
            if matches(r, filter) {
                ids[count] = r.id;
                count += 1;
            }
        }
        let public = StoreLease {
            handle,
            snapshot: HistorySnapshot {
                snapshot,
                epoch: self.data.domains[d].epoch,
                first_id: ids.first().copied().filter(|_| count > 0).unwrap_or(0),
                last_id: if count == 0 { 0 } else { ids[count - 1] },
                count: u32::try_from(count).map_err(|_| HistoryFailure::Capacity)?,
                lease_s: self.lease_s,
            },
            committed_id: self.data.ack[d],
        };
        // An unfiltered resumed export only needs new admission above its durable
        // watermark. Retained acknowledged IDs are already a validated prefix.
        let covered = if filter.filter_flags == 0 {
            ids[..count].partition_point(|id| *id <= self.data.ack[d])
        } else {
            0
        };
        self.next_handle = next;
        self.lease.set(Some(Lease {
            public,
            filter,
            ids,
            count,
            admitted: [false; R],
            covered,
            deadline,
        }));
        Ok(public)
    }
    fn plan(
        &mut self,
        lease: StoreLease,
        after_id: u64,
        max: u16,
        now_ms: u64,
        access: &dyn HistoryAccess,
    ) -> Result<ReadPlan, HistoryFailure> {
        let l = self.bound(lease, Some(now_ms), Some(access))?;
        if !(1..=16).contains(&max) || after_id > l.public.snapshot.last_id {
            return Err(HistoryFailure::Invalid);
        }
        let start = l.ids[..l.count].partition_point(|id| *id <= after_id);
        let count = (l.count - start).min(usize::from(max));
        let mut ids = [0; 16];
        ids[..count].copy_from_slice(&l.ids[start..start + count]);
        Ok(ReadPlan {
            ids,
            count: u16::try_from(count).map_err(|_| HistoryFailure::Capacity)?,
            end_after_id: if count == 0 { after_id } else { ids[count - 1] },
            eof: start + count == l.count,
        })
    }
    fn read(
        &self,
        lease: StoreLease,
        id: u64,
        out: &mut [u8],
        access: &dyn HistoryAccess,
    ) -> Result<RecordContent, HistoryFailure> {
        let l = self.bound(lease, None, Some(access))?;
        if !l.ids[..l.count].contains(&id) {
            return Err(HistoryFailure::NotFound);
        }
        let r = self.data.records[..self.data.records_len]
            .iter()
            .find(|r| r.domain == l.filter.domain && r.id == id)
            .ok_or_else(|| self.corrupt())?;
        r.stored()
            .verify(CommitState::Committed, r.crc)
            .map_err(|_| self.corrupt())?;
        self.schema
            .validate(r.stored(), r.parent)
            .map_err(|_| self.corrupt())?;
        if out.len() < r.len {
            return Err(HistoryFailure::Capacity);
        }
        out[..r.len].copy_from_slice(&r.body[..r.len]);
        Ok(RecordContent {
            domain: r.domain,
            epoch: r.epoch,
            record_id: r.id,
            record_type: r.kind,
            schema: r.schema,
            utc_ms: r.utc,
            flags: r.flags,
            length: r.len,
            commit: CommitState::Committed,
            stored_crc: r.crc,
        })
    }
    fn validate_record(
        &self,
        lease: StoreLease,
        record: StoredRecord<'_>,
    ) -> Result<(), HistoryFailure> {
        let l = self.bound(lease, None, None)?;
        if !l.ids[..l.count].contains(&record.record_id)
            || record.domain != l.filter.domain
            || record.epoch != l.public.snapshot.epoch
        {
            return Err(HistoryFailure::Identity);
        }
        let actual = self.data.records[..self.data.records_len]
            .iter()
            .find(|r| r.domain == record.domain && r.id == record.record_id)
            .ok_or_else(|| self.corrupt())?;
        if record != actual.stored() {
            return Err(HistoryFailure::StoredIntegrity);
        }
        record
            .verify(CommitState::Committed, actual.crc)
            .map_err(|_| self.corrupt())?;
        self.schema
            .validate(record, actual.parent)
            .map_err(|_| self.corrupt())
    }
    fn record_admitted(
        &mut self,
        lease: StoreLease,
        id: u64,
        access: &dyn HistoryAccess,
    ) -> Result<(), HistoryFailure> {
        let mut l = self.bound(lease, None, Some(access))?;
        let index = l.ids[..l.count]
            .binary_search(&id)
            .map_err(|_| HistoryFailure::InvalidState)?;
        l.admitted[index] = true;
        while l.covered < l.count && l.admitted[l.covered] {
            l.covered += 1;
        }
        self.lease.set(Some(l));
        Ok(())
    }
    fn ack_atomic(
        &mut self,
        lease: StoreLease,
        filter: HistoryFilter,
        epoch: u32,
        upto: u64,
        now_ms: u64,
        access: &dyn HistoryAccess,
    ) -> Result<u64, HistoryFailure> {
        let l = self.bound(lease, Some(now_ms), Some(access))?;
        self.authorize(l.filter.domain, HistoryOperation::Acknowledge, access)?;
        if filter != l.filter
            || filter.filter_flags != 0
            || filter.start_utc_ms != 0
            || filter.end_utc_ms != 0
            || filter.record_type != 0
            || filter.parent_session != 0
        {
            return Err(HistoryFailure::Invalid);
        }
        if epoch != l.public.snapshot.epoch {
            return Err(HistoryFailure::Conflict);
        }
        if upto == 0 || upto > l.public.snapshot.last_id || !l.ids[..l.count].contains(&upto) {
            return Err(HistoryFailure::Invalid);
        }
        let d = self.data.domain(filter.domain)?;
        if self.data.domains[d].epoch != epoch {
            return Err(HistoryFailure::Conflict);
        }
        if upto <= self.data.ack[d] {
            return Ok(self.data.ack[d]);
        }
        if !l.ids[..l.covered].contains(&upto) {
            return Err(HistoryFailure::InvalidState);
        }
        // A fully unfiltered actual ordered prefix, not arithmetic continuity.
        for r in &self.data.records[..self.data.records_len] {
            if r.domain == filter.domain
                && r.id > self.data.ack[d]
                && r.id <= upto
                && !l.ids[..l.covered].contains(&r.id)
            {
                return Err(HistoryFailure::InvalidState);
            }
        }
        // Recheck all live scopes immediately before the durable transaction.
        self.bound(lease, None, Some(access))?;
        self.authorize(filter.domain, HistoryOperation::Acknowledge, access)?;
        let mut data = self.data;
        data.ack[d] = upto;
        self.persist(data)?;
        Ok(upto)
    }
    fn count(
        &self,
        filter: HistoryFilter,
        access: &dyn HistoryAccess,
    ) -> Result<(u32, u32), HistoryFailure> {
        let d = self.check_filter(filter, access)?;
        let count = self.data.records[..self.data.records_len]
            .iter()
            .filter(|r| matches(r, filter))
            .count();
        Ok((
            self.data.domains[d].epoch,
            u32::try_from(count).map_err(|_| HistoryFailure::Capacity)?,
        ))
    }
    fn renew(
        &mut self,
        lease: StoreLease,
        now_ms: u64,
        access: &dyn HistoryAccess,
    ) -> Result<(), HistoryFailure> {
        let mut l = self.bound(lease, Some(now_ms), Some(access))?;
        let now = self.observe(None)?;
        l.deadline = now
            .checked_add(u64::from(self.lease_s) * 1000)
            .ok_or(HistoryFailure::Clock)?;
        self.lease.set(Some(l));
        Ok(())
    }
    fn close(&mut self, lease: StoreLease) -> Result<(), HistoryFailure> {
        self.ready()?;
        let Some(l) = self.lease.get() else {
            return if self.released.get() == Some(lease) {
                Ok(())
            } else {
                Err(HistoryFailure::Identity)
            };
        };
        if l.public != lease {
            return Err(HistoryFailure::Identity);
        }
        // Cleanup never extends a lease or acknowledges anything, even on clock errors.
        let observed = self.observe(None);
        self.release();
        observed.map(|_| ())
    }
}
