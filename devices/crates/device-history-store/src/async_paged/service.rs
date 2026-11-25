//! Zero-I/O prepared-result adapter for the existing CONTROL/DATA actor.
use super::{
    AsyncMedium, AsyncPagedHistoryStore, Cell, HistoryAccess, HistoryClock, HistoryDomain,
    HistoryFailure, HistoryFilter, HistoryOperation, HistorySchema, JobResult, JobToken, Kind,
    Phase, Preparation, ReadPlan, StoreLease,
};
use device_history_service::{Directory, RecordContent, RecordStore};
use device_storage::{CommitState, StoredRecord};
/// Connection-owned adapter. It owns only its pinned directory and preparation key;
/// physical work remains in the separately scheduled boot owner. Drop detaches it.
pub struct PreparedHistory<
    'a,
    M: AsyncMedium,
    L: AsyncMedium,
    V: HistorySchema,
    C: HistoryClock,
    const D: usize,
    const R: usize,
    const B: usize,
    const N: usize,
> {
    owner: &'a AsyncPagedHistoryStore<M, L, V, C, D, R, B, N>,
    generation: u64,
    dataset: [u8; 16],
    domains: [HistoryDomain; D],
    len: usize,
    pending: core::cell::RefCell<Option<(Preparation, u64)>>,
    rejection: Cell<Option<HistoryFailure>>,
}
impl<
    M: AsyncMedium,
    L: AsyncMedium,
    V: HistorySchema,
    C: HistoryClock,
    const D: usize,
    const R: usize,
    const B: usize,
    const N: usize,
> core::fmt::Debug for PreparedHistory<'_, M, L, V, C, D, R, B, N>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PreparedHistory")
            .field("generation", &self.generation)
            .field("domains", &self.len)
            .finish_non_exhaustive()
    }
}
impl<
    M: AsyncMedium,
    L: AsyncMedium,
    V: HistorySchema,
    C: HistoryClock,
    const D: usize,
    const R: usize,
    const B: usize,
    const N: usize,
> AsyncPagedHistoryStore<M, L, V, C, D, R, B, N>
{
    /// Create one connection generation with a copied, pinned O(D) directory.
    /// # Errors
    /// Unrecovered owner, reentry or exhausted generation is explicit.
    pub fn adapter(&self) -> Result<PreparedHistory<'_, M, L, V, C, D, R, B, N>, HistoryFailure> {
        let e = self.inner.try_borrow().map_err(|_| HistoryFailure::Busy)?;
        e.ready()?;
        let generation = self
            .generation
            .get()
            .checked_add(1)
            .ok_or(HistoryFailure::Capacity)?;
        self.generation.set(generation);
        Ok(PreparedHistory {
            owner: self,
            generation,
            dataset: e.data.dataset,
            domains: e.data.domains,
            len: e.data.len,
            pending: core::cell::RefCell::new(None),
            rejection: Cell::new(None),
        })
    }
    /// Recheck a pending preparation's live scope, then admit its durable ACK once.
    /// Performs no medium I/O; only `step` advances the accepted commit.
    /// # Errors
    /// Foreign token, stale lease/scope and preflight failures remain explicit.
    pub fn preparation_ready(
        &self,
        token: JobToken<'_>,
        now_ms: u64,
        access: &dyn HistoryAccess,
    ) -> Result<bool, HistoryFailure> {
        if !core::ptr::eq(token.owner, &raw const self.counter) {
            return Err(HistoryFailure::Identity);
        }
        let mut e = self
            .inner
            .try_borrow_mut()
            .map_err(|_| HistoryFailure::Busy)?;
        if e.token != token.id || e.detached {
            return Err(HistoryFailure::Identity);
        }
        if e.phase == Phase::AckAuthorize {
            let Kind::Prepare(request @ Preparation::Ack { upto, .. }) = e.kind else {
                return Err(HistoryFailure::InvalidState);
            };
            match e.preflight(&request, now_ms, access) {
                Ok((d, _)) => {
                    e.work = e.data;
                    e.work.ack[d] = upto;
                    e.phase = Phase::MetadataEncode;
                }
                Err(error) => e.fail(error),
            }
        }
        Ok(e.phase == Phase::Idle && e.terminal.is_some())
    }
}
impl<
    M: AsyncMedium,
    L: AsyncMedium,
    V: HistorySchema,
    C: HistoryClock,
    const D: usize,
    const R: usize,
    const B: usize,
    const N: usize,
> PreparedHistory<'_, M, L, V, C, D, R, B, N>
{
    fn prepare_exact(
        &self,
        request: &Preparation,
        now: u64,
        access: &dyn HistoryAccess,
    ) -> Result<bool, HistoryFailure> {
        let prior_id = {
            let pending = self
                .pending
                .try_borrow()
                .map_err(|_| HistoryFailure::Busy)?;
            match pending.as_ref() {
                Some((prior, id)) if prior == request => Some(*id),
                Some(_) => return Err(HistoryFailure::Busy),
                None => None,
            }
        };
        if let Some(id) = prior_id {
            if self.rejection.get().is_some() {
                return Ok(true);
            }
            return self.owner.preparation_ready(
                JobToken {
                    owner: &self.owner.counter,
                    id,
                },
                now,
                access,
            );
        }
        match self
            .owner
            .prepare_generation(request, now, access, self.generation)
        {
            Ok(token) => {
                self.retain_preparation(request, token.id)?;
                Ok(false)
            }
            Err(error) => {
                self.retain_preparation(request, 0)?;
                self.rejection.set(Some(error));
                Ok(true)
            }
        }
    }
    // Key installation follows validation/admission. Keep its temporary enum
    // off the stack while preflight checks the backend's authority and clock.
    #[inline(never)]
    fn retain_preparation(&self, request: &Preparation, id: u64) -> Result<(), HistoryFailure> {
        *self
            .pending
            .try_borrow_mut()
            .map_err(|_| HistoryFailure::Busy)? = Some((*request, id));
        Ok(())
    }
    // Copy only the selected variant, rather than the largest result enum.
    #[inline]
    fn consume<T>(
        &self,
        matches: impl FnOnce(&Preparation) -> bool,
        select: impl FnOnce(&JobResult) -> Result<T, HistoryFailure>,
    ) -> Result<T, HistoryFailure> {
        let id = {
            let pending = self
                .pending
                .try_borrow()
                .map_err(|_| HistoryFailure::Busy)?;
            let (prior, id) = pending.as_ref().ok_or(HistoryFailure::Busy)?;
            if !matches(prior) {
                return Err(HistoryFailure::Busy);
            }
            *id
        };
        if let Some(error) = self.rejection.take() {
            *self
                .pending
                .try_borrow_mut()
                .map_err(|_| HistoryFailure::Busy)? = None;
            return Err(error);
        }
        let e = self
            .owner
            .inner
            .try_borrow()
            .map_err(|_| HistoryFailure::Busy)?;
        if e.token != id || e.generation != self.generation || e.detached {
            return Err(HistoryFailure::Identity);
        }
        if e.phase != Phase::Idle {
            return Err(HistoryFailure::Busy);
        }
        let result = e.terminal.as_ref().ok_or(HistoryFailure::Busy)?;
        *self
            .pending
            .try_borrow_mut()
            .map_err(|_| HistoryFailure::Busy)? = None;
        select(result.as_ref().map_err(|error| *error)?)
    }
    fn detach_inner(&self) {
        self.rejection.set(None);
        // This private cell is never borrowed across a callback or owner operation.
        let id = if let Ok(mut pending) = self.pending.try_borrow_mut() {
            let id = pending.as_ref().map(|(_, id)| *id);
            *pending = None;
            id
        } else {
            None
        };
        if let Some(id) = id
            && id != 0
        {
            let _ = self.owner.detach(JobToken {
                owner: &self.owner.counter,
                id,
            });
        }
    }
}
impl<
    M: AsyncMedium,
    L: AsyncMedium,
    V: HistorySchema,
    C: HistoryClock,
    const D: usize,
    const R: usize,
    const B: usize,
    const N: usize,
> Drop for PreparedHistory<'_, M, L, V, C, D, R, B, N>
{
    fn drop(&mut self) {
        self.detach_inner();
        if let Ok(mut e) = self.owner.inner.try_borrow_mut()
            && e.lease_generation == self.generation
        {
            e.release();
        }
    }
}
impl<
    M: AsyncMedium,
    L: AsyncMedium,
    V: HistorySchema,
    C: HistoryClock,
    const D: usize,
    const R: usize,
    const B: usize,
    const N: usize,
> RecordStore for PreparedHistory<'_, M, L, V, C, D, R, B, N>
{
    fn prepare_ref(
        &mut self,
        request: &Preparation,
        now: u64,
        access: &dyn HistoryAccess,
    ) -> Result<bool, HistoryFailure> {
        self.prepare_exact(request, now, access)
    }
    fn prepare(
        &mut self,
        request: Preparation,
        now: u64,
        access: &dyn HistoryAccess,
    ) -> Result<bool, HistoryFailure> {
        self.prepare_exact(&request, now, access)
    }
    fn prefetch(
        &self,
        lease: StoreLease,
        id: u64,
        access: &dyn HistoryAccess,
    ) -> Result<bool, HistoryFailure> {
        {
            let mut e = self
                .owner
                .inner
                .try_borrow_mut()
                .map_err(|_| HistoryFailure::Busy)?;
            if e.lease_generation != self.generation {
                return Err(HistoryFailure::Identity);
            }
            e.bound(&lease, None, Some(access))?;
            if e.generation == self.generation
                && e.cache
                    .is_some_and(|(l, i, _)| l == lease.handle && i == id)
            {
                drop(e);
                if self
                    .pending
                    .try_borrow()
                    .map_err(|_| HistoryFailure::Busy)?
                    .is_some()
                {
                    self.consume(|p| matches!(p, Preparation::Record { lease: l, id: i } if *l == lease && *i == id), |result| {
                        if matches!(result, JobResult::RecordReady) {
                            Ok(())
                        } else {
                            Err(HistoryFailure::StoredIntegrity)
                        }
                    })?;
                }
                return Ok(true);
            }
        }
        let ready = self.prepare_exact(&Preparation::Record { lease, id }, 0, access)?;
        if ready {
            self.consume(
                |p| matches!(p, Preparation::Record { lease: l, id: i } if *l == lease && *i == id),
                |result| {
                    if matches!(result, JobResult::RecordReady) {
                        Ok(true)
                    } else {
                        Err(HistoryFailure::StoredIntegrity)
                    }
                },
            )
        } else {
            Ok(false)
        }
    }
    fn retire_record(
        &mut self,
        lease: StoreLease,
        id: u64,
        _access: &dyn HistoryAccess,
    ) -> Result<bool, HistoryFailure> {
        let mut e = self
            .owner
            .inner
            .try_borrow_mut()
            .map_err(|_| HistoryFailure::Busy)?;
        let pending_token = {
            let pending = self
                .pending
                .try_borrow()
                .map_err(|_| HistoryFailure::Busy)?;
            match pending.as_ref() {
                Some((
                    Preparation::Record {
                        lease: pending_lease,
                        id: pending_id,
                    },
                    token,
                )) => {
                    if *pending_lease != lease || *pending_id != id {
                        return Err(HistoryFailure::Identity);
                    }
                    Some(*token)
                }
                Some(_) => return Ok(true),
                None => None,
            }
        };
        if let Some(token) = pending_token {
            if let Some(error) = self.rejection.take() {
                *self
                    .pending
                    .try_borrow_mut()
                    .map_err(|_| HistoryFailure::Busy)? = None;
                return Err(error);
            }
            if e.token != token
                || e.generation != self.generation
                || e.detached
                || e.kind != Kind::Prepare(Preparation::Record { lease, id })
            {
                return Err(HistoryFailure::Identity);
            }
            if e.phase != Phase::Idle {
                e.phase = Phase::RetireRecord;
                if e.command.is_some() {
                    return Ok(false);
                }
                e.fail(HistoryFailure::Closed);
            }
            let result = e.terminal.ok_or(HistoryFailure::InvalidState)?;
            *self
                .pending
                .try_borrow_mut()
                .map_err(|_| HistoryFailure::Busy)? = None;
            if e.lease.active().is_some_and(|l| l.public == lease)
                && e.cache
                    .is_some_and(|(cached, record, _)| cached == lease.handle && record == id)
            {
                e.cache = None;
            }
            return match result {
                Ok(JobResult::RecordReady) => {
                    e.fail(HistoryFailure::Closed);
                    Ok(true)
                }
                Err(HistoryFailure::Closed) => Ok(true),
                Err(error) => Err(error),
                _ => Err(HistoryFailure::InvalidState),
            };
        }
        if e.generation == self.generation
            && e.lease.active().is_some_and(|l| l.public == lease)
            && e.cache
                .is_some_and(|(cached, record, _)| cached == lease.handle && record == id)
        {
            e.cache = None;
            e.fail(HistoryFailure::Closed);
        }
        Ok(true)
    }
    fn detach_preparation(&mut self) {
        self.detach_inner();
    }
    fn directory(&self) -> Result<Directory<'_>, HistoryFailure> {
        let e = self
            .owner
            .inner
            .try_borrow()
            .map_err(|_| HistoryFailure::Busy)?;
        e.ready()?;
        if e.data.dataset != self.dataset
            || e.data.len != self.len
            || e.data.domains[..self.len]
                .iter()
                .zip(&self.domains[..self.len])
                .any(|(live, pinned)| {
                    // Appends can advance retained bounds without changing this connection's
                    // advertised directory. Each actual lease independently pins and validates
                    // its own range; recovering a foreign/older generation still invalidates.
                    live.domain != pinned.domain
                        || live.epoch != pinned.epoch
                        || live.oldest_id < pinned.oldest_id
                        || live.newest_id < pinned.newest_id
                })
        {
            return Err(HistoryFailure::CursorExpired);
        }
        Ok(Directory {
            dataset: self.dataset,
            domains: &self.domains[..self.len],
        })
    }
    fn open(
        &mut self,
        snapshot: u32,
        filter: HistoryFilter,
        now: u64,
        access: &dyn HistoryAccess,
    ) -> Result<StoreLease, HistoryFailure> {
        let mut out = None;
        self.open_into(snapshot, filter, now, access, &mut out)?;
        out.ok_or(HistoryFailure::StoredIntegrity)
    }
    fn open_into(
        &mut self,
        snapshot: u32,
        filter: HistoryFilter,
        now: u64,
        access: &dyn HistoryAccess,
        out: &mut Option<StoreLease>,
    ) -> Result<(), HistoryFailure> {
        {
            let mut e = self
                .owner
                .inner
                .try_borrow_mut()
                .map_err(|_| HistoryFailure::Busy)?;
            e.authorize(filter.domain, HistoryOperation::Read, access)?;
            e.observe(Some(now))?;
        }
        self.consume(
            |p| matches!(p, Preparation::Open { snapshot: s, filter: f } if *s == snapshot && *f == filter),
            |result| match result {
                JobResult::Opened(lease) => { *out = Some(*lease); Ok(()) }
                _ => Err(HistoryFailure::StoredIntegrity),
            },
        )?;
        let lease = out.as_ref().ok_or(HistoryFailure::StoredIntegrity)?;
        self.owner
            .inner
            .try_borrow_mut()
            .map_err(|_| HistoryFailure::Busy)?
            .bound(lease, Some(now), Some(access))?;
        Ok(())
    }
    fn plan(
        &mut self,
        lease: StoreLease,
        after_id: u64,
        max: u16,
        now: u64,
        access: &dyn HistoryAccess,
    ) -> Result<ReadPlan, HistoryFailure> {
        let mut plan = ReadPlan {
            ids: [0; 16],
            count: 0,
            end_after_id: 0,
            eof: false,
        };
        self.plan_into(lease, after_id, max, now, access, &mut plan)?;
        Ok(plan)
    }
    fn plan_into(
        &mut self,
        lease: StoreLease,
        after_id: u64,
        max: u16,
        now: u64,
        access: &dyn HistoryAccess,
        out: &mut ReadPlan,
    ) -> Result<(), HistoryFailure> {
        self.owner
            .inner
            .try_borrow_mut()
            .map_err(|_| HistoryFailure::Busy)?
            .bound(&lease, Some(now), Some(access))?;
        self.consume(
            |p| matches!(p, Preparation::Plan { lease: l, after_id: a, max: m } if *l == lease && *a == after_id && *m == max),
            |result| match result {
                JobResult::Planned(plan) => { *out = *plan; Ok(()) }
                _ => Err(HistoryFailure::StoredIntegrity),
            },
        )
    }
    fn count(
        &self,
        filter: HistoryFilter,
        access: &dyn HistoryAccess,
    ) -> Result<(u32, u32), HistoryFailure> {
        self.owner
            .inner
            .try_borrow_mut()
            .map_err(|_| HistoryFailure::Busy)?
            .check_filter(filter, access)?;
        self.consume(
            |p| matches!(p, Preparation::Count { filter: f } if *f == filter),
            |result| match result {
                JobResult::Counted { epoch, count } => Ok((*epoch, *count)),
                _ => Err(HistoryFailure::StoredIntegrity),
            },
        )
    }
    fn read(
        &self,
        lease: StoreLease,
        id: u64,
        out: &mut [u8],
        access: &dyn HistoryAccess,
    ) -> Result<RecordContent, HistoryFailure> {
        let mut e = self
            .owner
            .inner
            .try_borrow_mut()
            .map_err(|_| HistoryFailure::Busy)?;
        if e.lease_generation != self.generation {
            return Err(HistoryFailure::Identity);
        }
        e.bound(&lease, None, Some(access))?;
        if !e
            .cache
            .is_some_and(|(l, i, _)| l == lease.handle && i == id)
        {
            return Err(HistoryFailure::Busy);
        }
        let r = &e.record;
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
        let mut e = self
            .owner
            .inner
            .try_borrow_mut()
            .map_err(|_| HistoryFailure::Busy)?;
        if e.lease_generation != self.generation {
            return Err(HistoryFailure::Identity);
        }
        e.bound(&lease, None, None)?;
        if !e
            .cache
            .is_some_and(|(l, id, _)| l == lease.handle && id == record.record_id)
        {
            return Err(HistoryFailure::Busy);
        }
        if e.record.stored() != record {
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
        let mut e = self
            .owner
            .inner
            .try_borrow_mut()
            .map_err(|_| HistoryFailure::Busy)?;
        if e.lease_generation != self.generation {
            return Err(HistoryFailure::Identity);
        }
        e.bound(&lease, None, Some(access))?;
        let (cached, record, index) = e
            .cache
            .filter(|(cached, record, _)| *cached == lease.handle && *record == id)
            .or(e
                .proof
                .filter(|(cached, record, _)| *cached == lease.handle && *record == id))
            .ok_or(HistoryFailure::Busy)?;
        if cached != lease.handle || record != id || index >= 255 {
            return Err(HistoryFailure::Identity);
        }
        // All fallible authority/proof checks precede mutation. Avoid copying
        // the complete bitmap and public lease merely to set one ordinal bit.
        let l = e.lease.active_mut().ok_or(HistoryFailure::CursorExpired)?;
        l.admitted[index / 8] |= 1 << (index % 8);
        while l.covered < l.count && l.admitted[l.covered / 8] & (1 << (l.covered % 8)) != 0 {
            l.covered += 1;
        }
        e.proof = Some((cached, record, index));
        e.cache = None;
        Ok(())
    }
    fn ack_atomic(
        &mut self,
        lease: StoreLease,
        filter: HistoryFilter,
        epoch: u32,
        upto: u64,
        _now: u64,
        access: &dyn HistoryAccess,
    ) -> Result<u64, HistoryFailure> {
        // This consumes only the exact retained durable receipt. It cannot submit I/O.
        self.owner
            .inner
            .try_borrow()
            .map_err(|_| HistoryFailure::Busy)?
            .authorize(filter.domain, HistoryOperation::Acknowledge, access)?;
        self.consume(
            |p| matches!(p, Preparation::Ack { lease: l, filter: f, epoch: e, upto: u } if *l == lease && *f == filter && *e == epoch && *u == upto),
            |result| match result {
                JobResult::Acknowledged { upto } => Ok(*upto),
                _ => Err(HistoryFailure::StoredIntegrity),
            },
        )
    }
    fn renew(
        &mut self,
        lease: StoreLease,
        now: u64,
        access: &dyn HistoryAccess,
    ) -> Result<(), HistoryFailure> {
        let mut e = self
            .owner
            .inner
            .try_borrow_mut()
            .map_err(|_| HistoryFailure::Busy)?;
        if e.lease_generation != self.generation {
            return Err(HistoryFailure::Identity);
        }
        e.bound(&lease, Some(now), Some(access))?;
        let actual = e.observe(None)?;
        let deadline = actual
            .checked_add(u64::from(e.lease_s) * 1000)
            .ok_or(HistoryFailure::Clock)?;
        e.lease
            .active_mut()
            .ok_or(HistoryFailure::CursorExpired)?
            .deadline = deadline;
        Ok(())
    }
    fn close(&mut self, lease: StoreLease) -> Result<(), HistoryFailure> {
        self.detach_inner();
        let mut e = self
            .owner
            .inner
            .try_borrow_mut()
            .map_err(|_| HistoryFailure::Busy)?;
        if e.lease.active().is_some() && e.lease_generation != self.generation {
            return Err(HistoryFailure::Identity);
        }
        // Compare through the borrow; the admission bitmap and full lease are
        // not needed while observing/releasing the same owner below.
        if let Some(l) = e.lease.active() {
            if l.public != lease {
                return Err(HistoryFailure::Identity);
            }
            let observed = e.observe(None);
            e.release();
            observed.map(|_| ())
        } else if e.lease.released() == Some(lease) {
            Ok(())
        } else {
            Err(HistoryFailure::Identity)
        }
    }
}
