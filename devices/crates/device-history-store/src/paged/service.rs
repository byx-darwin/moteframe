use super::{
    HistoryClock, HistoryFailure, HistoryFilter, HistorySchema, Lease, Medium, PagedHistoryStore,
    Record, SnapshotStore, StoreLease, to_u16, to_u32,
};
use crate::repository::matches;
use device_history_service::{
    Directory, HistoryAccess, HistoryOperation, ReadPlan, RecordContent, RecordStore,
};
use device_protocol::{HistoryRequest, HistorySnapshot};
use device_storage::{CommitState, StoredRecord};
impl<
    S: SnapshotStore,
    L: Medium,
    V: HistorySchema,
    C: HistoryClock,
    const D: usize,
    const R: usize,
    const B: usize,
    const N: usize,
> PagedHistoryStore<S, L, V, C, D, R, B, N>
{
    fn authorize(
        &self,
        domain: u8,
        op: HistoryOperation,
        access: &dyn HistoryAccess,
    ) -> Result<(), HistoryFailure> {
        if access.module_allowed()
            && access.allowed(self.data.dataset, None, op)
            && access.allowed(self.data.dataset, Some(domain), op)
        {
            Ok(())
        } else {
            Err(HistoryFailure::Denied)
        }
    }
    fn observe(&self, supplied: Option<u64>) -> Result<u64, HistoryFailure> {
        let actual = match self.clock.now_ms() {
            Ok(time) => time,
            Err(error) => {
                self.release();
                return Err(error);
            }
        };
        if self.actual_time.get().is_some_and(|old| actual < old) {
            self.release();
            return Err(HistoryFailure::Clock);
        }
        self.actual_time.set(Some(actual));
        if supplied.is_some_and(|now| {
            now > actual || self.supplied_time.get().is_some_and(|old| now < old)
        }) {
            self.release();
            return Err(HistoryFailure::Clock);
        }
        if let Some(time) = supplied {
            self.supplied_time.set(Some(time));
        }
        Ok(actual)
    }
    fn bound(
        &self,
        public: StoreLease,
        supplied: Option<u64>,
        access: Option<&dyn HistoryAccess>,
    ) -> Result<Lease, HistoryFailure> {
        self.ready()?;
        let now = self.observe(supplied)?;
        let lease = self.lease.get().ok_or(HistoryFailure::CursorExpired)?;
        if public != lease.public {
            return Err(HistoryFailure::Identity);
        }
        if now >= lease.deadline {
            self.release();
            return Err(HistoryFailure::CursorExpired);
        }
        if let Some(access) = access {
            self.authorize(lease.filter.domain, HistoryOperation::Read, access)?;
        }
        Ok(lease)
    }
    fn check_filter(
        &self,
        filter: HistoryFilter,
        access: &dyn HistoryAccess,
    ) -> Result<usize, HistoryFailure> {
        self.ready()?;
        let mut bytes = [0; 28];
        HistoryRequest::OpenV2(filter)
            .encode(&mut bytes)
            .map_err(|_| HistoryFailure::Invalid)?;
        self.authorize(filter.domain, HistoryOperation::Read, access)?;
        self.data.domain(filter.domain)
    }
}
impl<
    S: SnapshotStore,
    L: Medium,
    V: HistorySchema,
    C: HistoryClock,
    const D: usize,
    const R: usize,
    const B: usize,
    const N: usize,
> PagedHistoryStore<S, L, V, C, D, R, B, N>
{
    fn scan(
        &self,
        d: usize,
        cut: u32,
        filter: HistoryFilter,
        mut visit: impl FnMut(usize, &Record<B>) -> Result<(), HistoryFailure>,
    ) -> Result<usize, HistoryFailure> {
        let mut ordinal = 0;
        let mut high = 0;
        for slot in 0..cut {
            if let Some(r) = self.load_record(d, slot)? {
                if r.id <= high {
                    return Err(self.corrupt());
                }
                high = r.id;
                if matches(&r, filter) {
                    visit(ordinal, &r)?;
                    ordinal += 1;
                }
            }
        }
        Ok(ordinal)
    }
    fn find(&self, l: Lease, id: u64) -> Result<Option<(usize, Record<B>)>, HistoryFailure> {
        let d = self.data.domain(l.filter.domain)?;
        let mut found = None;
        let n = self.scan(d, l.cut, l.filter, |i, r| {
            if r.id == id {
                found = Some((i, *r));
            }
            Ok(())
        })?;
        if n != l.count {
            return Err(self.corrupt());
        }
        Ok(found)
    }
}
impl<
    S: SnapshotStore,
    L: Medium,
    V: HistorySchema,
    C: HistoryClock,
    const D: usize,
    const R: usize,
    const B: usize,
    const N: usize,
> RecordStore for PagedHistoryStore<S, L, V, C, D, R, B, N>
{
    fn directory(&self) -> Result<Directory<'_>, HistoryFailure> {
        self.ready()?;
        Ok(Directory {
            dataset: self.data.dataset,
            domains: &self.data.domains[..self.data.len],
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
        let mut first = 0;
        let mut last = 0;
        let mut covered = 0;
        let cut = self.data.tail[d];
        let count = self.scan(d, cut, filter, |_, r| {
            if first == 0 {
                first = r.id;
            }
            last = r.id;
            if filter.filter_flags == 0 && r.id <= self.data.ack[d] {
                covered += 1;
            }
            Ok(())
        })?;
        if count > R {
            return Err(self.corrupt());
        }
        let public = StoreLease {
            handle,
            snapshot: HistorySnapshot {
                snapshot,
                epoch: self.data.domains[d].epoch,
                first_id: first,
                last_id: last,
                count: to_u32(count)?,
                lease_s: self.lease_s,
            },
            committed_id: self.data.ack[d],
        };
        self.next_handle = next;
        self.lease.set(Some(Lease {
            public,
            filter,
            cut,
            count,
            admitted: [0; 32],
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
        let d = self.data.domain(l.filter.domain)?;
        let mut ids = [0; 16];
        let mut count = 0;
        let mut remaining = 0;
        let n = self.scan(d, l.cut, l.filter, |_, r| {
            if r.id > after_id {
                remaining += 1;
                if count < max as usize {
                    ids[count] = r.id;
                    count += 1;
                }
            }
            Ok(())
        })?;
        if n != l.count {
            return Err(self.corrupt());
        }
        Ok(ReadPlan {
            ids,
            count: to_u16(count)?,
            end_after_id: if count == 0 { after_id } else { ids[count - 1] },
            eof: count == remaining,
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
        let (_, r) = self.find(l, id)?.ok_or(HistoryFailure::NotFound)?;
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
        if record.domain != l.filter.domain || record.epoch != l.public.snapshot.epoch {
            return Err(HistoryFailure::Identity);
        }
        let (_, actual) = self
            .find(l, record.record_id)?
            .ok_or(HistoryFailure::Identity)?;
        if record != actual.stored() {
            return Err(HistoryFailure::StoredIntegrity);
        }
        Ok(())
    }
    fn record_admitted(
        &mut self,
        lease: StoreLease,
        id: u64,
        access: &dyn HistoryAccess,
    ) -> Result<(), HistoryFailure> {
        let mut l = self.bound(lease, None, Some(access))?;
        let (index, _) = self.find(l, id)?.ok_or(HistoryFailure::InvalidState)?;
        if index >= 255 {
            return Err(self.corrupt());
        }
        l.admitted[index / 8] |= 1 << (index % 8);
        while l.covered < l.count && l.admitted[l.covered / 8] & (1 << (l.covered % 8)) != 0 {
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
        if upto == 0 || upto > l.public.snapshot.last_id {
            return Err(HistoryFailure::Invalid);
        }
        let (index, _) = self.find(l, upto)?.ok_or(HistoryFailure::Invalid)?;
        let d = self.data.domain(filter.domain)?;
        if self.data.domains[d].epoch != epoch {
            return Err(HistoryFailure::Conflict);
        }
        if upto <= self.data.ack[d] {
            return Ok(self.data.ack[d]);
        }
        if index >= l.covered {
            return Err(HistoryFailure::InvalidState);
        }
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
        let count = self.scan(d, self.data.tail[d], filter, |_, _| Ok(()))?;
        Ok((self.data.domains[d].epoch, to_u32(count)?))
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
        let observed = self.observe(None);
        self.release();
        observed.map(|_| ())
    }
}
