//! One logical history directory over two independently owned real stores.
use crate::{
    Directory, HistoryAccess, HistoryFailure as F, HistoryFilter, Preparation, ReadPlan,
    RecordContent, RecordStore, StoreLease,
};
use device_protocol::HistoryDomain;
use device_storage::StoredRecord;
// Keep both branches concrete; returning a trait object here erases the
// native store type and prevents proving the READ/ACK callee at compile time.
macro_rules! on_side {
    ($owner:ident, $side:expr, $method:ident($($arg:expr),*)) => {
        if $side { $owner.right.$method($($arg),*) } else { $owner.left.$method($($arg),*) }
    };
}
/// Joins disjoint domain ranges without sharing media ownership, leases or ACK proofs.
/// Exactly one frozen lease is active; opaque handles carry a checked store tag.
pub struct JoinedHistory<
    'a,
    const D: usize,
    L: RecordStore + ?Sized = dyn RecordStore + 'a,
    R: RecordStore + ?Sized = dyn RecordStore + 'a,
> {
    left: &'a mut L,
    right: &'a mut R,
    dataset: [u8; 16],
    domains: [HistoryDomain; D],
    len: usize,
    lease: LeaseSlot,
}
// Only the latest closed receipt is observable, and only without an active lease.
enum LeaseSlot {
    Empty,
    Active(StoreLease),
    Released(StoreLease),
}
impl LeaseSlot {
    fn active(&self) -> Option<&StoreLease> {
        if let Self::Active(lease) = self {
            Some(lease)
        } else {
            None
        }
    }
    fn released(&self) -> Option<StoreLease> {
        if let Self::Released(lease) = self {
            Some(*lease)
        } else {
            None
        }
    }
}
impl<L: RecordStore + ?Sized, R: RecordStore + ?Sized, const D: usize> core::fmt::Debug
    for JoinedHistory<'_, D, L, R>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("JoinedHistory")
            .field("domains", &self.len)
            .finish_non_exhaustive()
    }
}
impl<'a, L: RecordStore + ?Sized, R: RecordStore + ?Sized, const D: usize>
    JoinedHistory<'a, D, L, R>
{
    /// Pin the same dataset and a sorted, unique union of two ready domain directories.
    /// # Errors
    /// Rejects unavailable/foreign datasets, duplicate domains, empty or undersized bounds.
    pub fn new(left: &'a mut L, right: &'a mut R) -> Result<Self, F> {
        let a = left.directory()?;
        let b = right.directory()?;
        if a.dataset == [0; 16]
            || a.dataset != b.dataset
            || a.domains.is_empty()
            || b.domains.is_empty()
            || a.domains.len() + b.domains.len() > D
        {
            return Err(F::Invalid);
        }
        let mut domains = [HistoryDomain {
            domain: 0,
            epoch: 0,
            oldest_id: 0,
            newest_id: 0,
        }; D];
        let mut len = 0;
        for d in a.domains.iter().chain(b.domains) {
            if d.domain == 255
                || d.epoch == 0
                || domains[..len].iter().any(|x| x.domain == d.domain)
            {
                return Err(F::Invalid);
            }
            domains[len] = *d;
            len += 1;
        }
        // The bounded directory is tiny (nine entries on wattch 5). A
        // nonrecursive insertion sort avoids the generic sort's large scratch
        // frames while preserving support for independently ordered children.
        for i in 1..len {
            let mut at = i;
            while at > 0 && domains[at - 1].domain > domains[at].domain {
                domains.swap(at - 1, at);
                at -= 1;
            }
        }
        let dataset = a.dataset;
        Ok(Self {
            left,
            right,
            dataset,
            domains,
            len,
            lease: LeaseSlot::Empty,
        })
    }
    fn side(&self, domain: u8) -> Result<bool, F> {
        self.directory()?;
        let a = self.left.directory()?;
        let b = self.right.directory()?;
        if a.dataset != self.dataset || b.dataset != self.dataset {
            return Err(F::Identity);
        }
        if a.domains.iter().any(|d| d.domain == domain) {
            Ok(false)
        } else if b.domains.iter().any(|d| d.domain == domain) {
            Ok(true)
        } else {
            Err(F::NotFound)
        }
    }
    fn unwrap(&self, lease: &mut StoreLease) -> Result<bool, F> {
        if self.lease.active() != Some(&*lease) {
            return Err(F::Identity);
        }
        let side = lease.handle & 1 != 0;
        lease.handle /= 2;
        if lease.handle == 0 {
            return Err(F::Identity);
        }
        Ok(side)
    }
    fn preparation(&self, p: &mut Preparation) -> Result<bool, F> {
        match p {
            Preparation::Open { filter, .. } | Preparation::Count { filter } => {
                self.side(filter.domain)
            }
            Preparation::Plan { lease, .. } | Preparation::Record { lease, .. } => {
                self.unwrap(lease)
            }
            Preparation::Ack { lease, filter, .. } => {
                let side = self.unwrap(lease)?;
                if self.side(filter.domain)? != side {
                    return Err(F::Identity);
                }
                Ok(side)
            }
        }
    }
}
impl<L: RecordStore + ?Sized, R: RecordStore + ?Sized, const D: usize> RecordStore
    for JoinedHistory<'_, D, L, R>
{
    fn directory(&self) -> Result<Directory<'_>, F> {
        let left = self.left.directory()?;
        let right = self.right.directory()?;
        if left.dataset != self.dataset || right.dataset != self.dataset {
            return Err(F::Identity);
        }
        if left.domains.len() + right.domains.len() != self.len
            || left
                .domains
                .iter()
                .chain(right.domains)
                .any(|entry| !self.domains[..self.len].contains(entry))
        {
            return Err(F::CursorExpired);
        }
        Ok(Directory {
            dataset: self.dataset,
            domains: &self.domains[..self.len],
        })
    }
    fn prepare(&mut self, p: Preparation, n: u64, a: &dyn HistoryAccess) -> Result<bool, F> {
        self.prepare_ref(&p, n, a)
    }
    fn prepare_ref(&mut self, p: &Preparation, n: u64, a: &dyn HistoryAccess) -> Result<bool, F> {
        if self.lease.active().is_some() && matches!(p, Preparation::Open { .. }) {
            return Err(F::Busy);
        }
        let mut raw = *p;
        let side = self.preparation(&mut raw)?;
        on_side!(self, side, prepare_ref(&raw, n, a))
    }
    fn prefetch(&self, l: StoreLease, id: u64, a: &dyn HistoryAccess) -> Result<bool, F> {
        let mut l = l;
        let s = self.unwrap(&mut l)?;
        on_side!(self, s, prefetch(l, id, a))
    }
    fn retire_record(&mut self, l: StoreLease, id: u64, a: &dyn HistoryAccess) -> Result<bool, F> {
        let mut l = l;
        let s = self.unwrap(&mut l)?;
        on_side!(self, s, retire_record(l, id, a))
    }
    fn detach_preparation(&mut self) {
        self.left.detach_preparation();
        self.right.detach_preparation();
    }
    fn open(
        &mut self,
        id: u32,
        f: HistoryFilter,
        n: u64,
        a: &dyn HistoryAccess,
    ) -> Result<StoreLease, F> {
        let mut out = None;
        self.open_into(id, f, n, a, &mut out)?;
        out.ok_or(F::StoredIntegrity)
    }
    #[inline(never)]
    fn open_into(
        &mut self,
        id: u32,
        f: HistoryFilter,
        n: u64,
        a: &dyn HistoryAccess,
        out: &mut Option<StoreLease>,
    ) -> Result<(), F> {
        if self.lease.active().is_some() {
            return Err(F::Busy);
        }
        let side = self.side(f.domain)?;
        on_side!(self, side, open_into(id, f, n, a, out))?;
        let raw = out.as_mut().ok_or(F::StoredIntegrity)?;
        let Some(handle) = raw
            .handle
            .checked_mul(2)
            .and_then(|h| h.checked_add(u64::from(side)))
        else {
            on_side!(self, side, close(*raw))?;
            return Err(F::Capacity);
        };
        raw.handle = handle;
        self.lease = LeaseSlot::Active(*raw);
        Ok(())
    }
    fn plan(
        &mut self,
        l: StoreLease,
        id: u64,
        max: u16,
        n: u64,
        a: &dyn HistoryAccess,
    ) -> Result<ReadPlan, F> {
        let mut l = l;
        let s = self.unwrap(&mut l)?;
        on_side!(self, s, plan(l, id, max, n, a))
    }
    fn plan_into(
        &mut self,
        l: StoreLease,
        id: u64,
        max: u16,
        n: u64,
        a: &dyn HistoryAccess,
        out: &mut ReadPlan,
    ) -> Result<(), F> {
        let mut l = l;
        let s = self.unwrap(&mut l)?;
        on_side!(self, s, plan_into(l, id, max, n, a, out))
    }
    fn read(
        &self,
        l: StoreLease,
        id: u64,
        out: &mut [u8],
        a: &dyn HistoryAccess,
    ) -> Result<RecordContent, F> {
        let mut l = l;
        let s = self.unwrap(&mut l)?;
        on_side!(self, s, read(l, id, out, a))
    }
    fn validate_record(&self, l: StoreLease, r: StoredRecord<'_>) -> Result<(), F> {
        let mut l = l;
        let s = self.unwrap(&mut l)?;
        if self.side(r.domain)? != s {
            return Err(F::Identity);
        }
        on_side!(self, s, validate_record(l, r))
    }
    fn record_admitted(&mut self, l: StoreLease, id: u64, a: &dyn HistoryAccess) -> Result<(), F> {
        let mut l = l;
        let s = self.unwrap(&mut l)?;
        on_side!(self, s, record_admitted(l, id, a))
    }
    fn ack_atomic(
        &mut self,
        lease: StoreLease,
        filter: HistoryFilter,
        epoch: u32,
        id: u64,
        now: u64,
        access: &dyn HistoryAccess,
    ) -> Result<u64, F> {
        let mut lease = lease;
        let side = self.unwrap(&mut lease)?;
        if self.side(filter.domain)? != side {
            return Err(F::Identity);
        }
        on_side!(
            self,
            side,
            ack_atomic(lease, filter, epoch, id, now, access)
        )
    }
    fn count(&self, f: HistoryFilter, a: &dyn HistoryAccess) -> Result<(u32, u32), F> {
        on_side!(self, self.side(f.domain)?, count(f, a))
    }
    #[inline(never)]
    fn renew(&mut self, l: StoreLease, n: u64, a: &dyn HistoryAccess) -> Result<(), F> {
        let mut l = l;
        let s = self.unwrap(&mut l)?;
        on_side!(self, s, renew(l, n, a))
    }
    fn close(&mut self, l: StoreLease) -> Result<(), F> {
        if self.lease.active().is_none() && self.lease.released() == Some(l) {
            return Ok(());
        }
        let mut raw = l;
        let s = self.unwrap(&mut raw)?;
        on_side!(self, s, close(raw))?;
        self.lease = LeaseSlot::Released(l);
        Ok(())
    }
}
