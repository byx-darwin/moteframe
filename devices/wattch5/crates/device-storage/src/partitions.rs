//! Exclusive asynchronous ranges over one retained physical command owner.
//!
//! Boot recovery drains any abandoned command, then Syncs the medium. This
//! opens reads and owner recovery Syncs only. A trusted boot coordinator must
//! attest every actual owner's recovery before enabling program/erase.
#![forbid(unsafe_code)]
use crate::{
    incremental::{AsyncMedium, Completion, MediumOp, StartError},
    persistence::{Error, Geometry, MAX_WRITE},
};
use core::{cell::RefCell, fmt, task::Poll};

/// One disjoint range in the underlying medium's logical address space.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Range {
    /// Absolute start, aligned to erase and program units.
    pub start: u32,
    /// Nonzero length, aligned to erase and program units.
    pub length: u32,
    /// Whether the range holder may erase its own erase units.
    pub erase: bool,
}
/// Explicit broker/coordinator failures.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PartitionError {
    /// This broker has already issued its sole set of handles.
    AlreadySplit,
    /// A physical command remains owned by a partition.
    Busy,
    /// A nested call encountered a currently borrowed broker.
    Reentrant,
    /// Global Sync recovery has not established read-only access.
    NeedsRecovery,
    /// Physical I/O or an invalid recovery completion failed.
    Storage(Error),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Gate {
    Closed,
    ReadOnly,
    Normal,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Recovery {
    Idle,
    Drain,
    SyncBegin,
    SyncPoll,
    Ready,
}
#[derive(Clone, Copy, Debug)]
struct Active {
    owner: usize,
    expected: Completion,
}
struct Inner<M> {
    medium: M,
    active: Option<Active>,
    gate: Gate,
    recovery: Recovery,
}
impl<M> Inner<M> {
    fn fault(&mut self) {
        self.gate = Gate::Closed;
        self.recovery = Recovery::Idle;
    }
}
/// Heap-free owner of one non-cloned physical medium and validated ranges.
///
/// The medium is never exposed or replaced. The exclusive `split` borrow pins
/// its lifetime until all handles are gone; splitting a second time is refused.
/// Dropping any handle does not release an outstanding physical operation.
pub struct AsyncPartitions<M, const K: usize> {
    inner: RefCell<Inner<M>>,
    ranges: [Range; K],
    geometry: Geometry,
    split: bool,
}
impl<M, const K: usize> fmt::Debug for AsyncPartitions<M, K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AsyncPartitions")
            .field("ranges", &self.ranges)
            .field("geometry", &self.geometry)
            .field("split", &self.split)
            .finish_non_exhaustive()
    }
}
/// Sole boot authority for bounded global draining, Sync and owner attestation.
pub struct Coordinator<'a, M> {
    inner: &'a RefCell<Inner<M>>,
}
impl<M> fmt::Debug for Coordinator<'_, M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Coordinator").finish_non_exhaustive()
    }
}
/// Non-clone asynchronous capability with fixed range-relative geometry.
///
/// A terminal result consumes its command. Polling without ownership returns
/// `Conflict`; nested polling returns `Conflict`, with no physical I/O. Invalid
/// admission returns `StartError::Io` without effects. All uncertainty is global.
pub struct Partition<'a, M> {
    inner: &'a RefCell<Inner<M>>,
    owner: usize,
    range: Range,
    geometry: Geometry,
}
impl<M> fmt::Debug for Partition<'_, M> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Partition")
            .field("owner", &self.owner)
            .field("range", &self.range)
            .field("geometry", &self.geometry)
            .finish_non_exhaustive()
    }
}
impl<M: AsyncMedium, const K: usize> AsyncPartitions<M, K> {
    /// Validate all geometry/ranges before any I/O and close the recovery gate.
    ///
    /// # Errors
    /// `InvalidLayout` for zero/unsupported geometry, empty/unaligned ranges,
    /// overflow, overlap, or out-of-bounds ranges. No I/O is performed.
    pub fn new_unrecovered(medium: M, ranges: [Range; K]) -> Result<Self, Error> {
        let geometry = medium.geometry();
        validate(geometry, &ranges)?;
        Ok(Self {
            inner: RefCell::new(Inner {
                medium,
                active: None,
                gate: Gate::Closed,
                recovery: Recovery::Idle,
            }),
            ranges,
            geometry,
            split: false,
        })
    }
    /// Issue exactly one coordinator and one exclusive handle per range.
    ///
    /// # Errors
    /// `AlreadySplit` if handles have already been issued, even if dropped.
    pub fn split(&mut self) -> Result<(Coordinator<'_, M>, [Partition<'_, M>; K]), PartitionError> {
        if self.split {
            return Err(PartitionError::AlreadySplit);
        }
        self.split = true;
        let inner = &self.inner;
        let partitions = core::array::from_fn(|owner| Partition {
            inner,
            owner,
            range: self.ranges[owner],
            geometry: Geometry {
                capacity: self.ranges[owner].length,
                ..self.geometry
            },
        });
        Ok((Coordinator { inner }, partitions))
    }
}
fn validate(g: Geometry, ranges: &[Range]) -> Result<(), Error> {
    if g.capacity == 0
        || g.erase_size == 0
        || g.write_size == 0
        || g.write_size > 256
        || !g.erase_size.is_multiple_of(g.write_size)
        || !g.capacity.is_multiple_of(g.erase_size)
        || ranges.is_empty()
    {
        return Err(Error::InvalidLayout);
    }
    for (index, r) in ranges.iter().enumerate() {
        let end = r.start.checked_add(r.length).ok_or(Error::InvalidLayout)?;
        if r.length == 0
            || !r.start.is_multiple_of(g.erase_size)
            || !r.length.is_multiple_of(g.erase_size)
            || end > g.capacity
        {
            return Err(Error::InvalidLayout);
        }
        for other in &ranges[..index] {
            let other_end = other
                .start
                .checked_add(other.length)
                .ok_or(Error::InvalidLayout)?;
            if r.start < other_end && other.start < end {
                return Err(Error::InvalidLayout);
            }
        }
    }
    Ok(())
}
impl<M: AsyncMedium> Coordinator<'_, M> {
    /// Advance recovery by at most one physical begin OR poll, with 256-byte I/O.
    ///
    /// Calling during normal operation deliberately closes access, takes drain
    /// responsibility for any accepted command, then establishes a fresh Sync
    /// barrier. A successful result opens only Read/Sync. Repeated success is
    /// inert until a shared fault or a later normal-mode recovery request.
    ///
    /// # Errors
    /// `Reentrant` on nested access; `Storage` on recovery Sync failure or wrong
    /// completion. Failures keep access closed; another call retries recovery.
    pub fn recover_step(&mut self) -> Poll<Result<(), PartitionError>> {
        let Ok(mut inner) = self.inner.try_borrow_mut() else {
            return Poll::Ready(Err(PartitionError::Reentrant));
        };
        if inner.recovery == Recovery::Ready && inner.gate == Gate::ReadOnly {
            return Poll::Ready(Ok(()));
        }
        if inner.recovery == Recovery::Idle || inner.gate == Gate::Normal {
            inner.gate = Gate::Closed;
            inner.recovery = Recovery::Drain;
        }
        if inner.recovery == Recovery::Drain {
            if inner.active.is_some() {
                let mut scratch = [0; MAX_WRITE];
                if inner.medium.poll(&mut scratch).is_pending() {
                    return Poll::Pending;
                }
                // A terminal poll releases the command even when its result is
                // malformed/failed; a fresh Sync still must settle effects.
                inner.active = None;
                inner.recovery = Recovery::SyncBegin;
                return Poll::Pending;
            }
            inner.recovery = Recovery::SyncBegin;
        }
        if inner.recovery == Recovery::SyncBegin {
            match inner.medium.begin(MediumOp::Sync) {
                Ok(()) => {
                    inner.active = Some(Active {
                        owner: usize::MAX,
                        expected: Completion::Synced,
                    });
                    inner.recovery = Recovery::SyncPoll;
                }
                Err(StartError::Busy) => {}
                Err(StartError::Io) => {
                    // Admission may have started a physical operation. Retain
                    // drain responsibility before attempting another Sync.
                    inner.active = Some(Active {
                        owner: usize::MAX,
                        expected: Completion::Synced,
                    });
                    inner.fault();
                    return Poll::Ready(Err(PartitionError::Storage(Error::Io)));
                }
            }
            return Poll::Pending;
        }
        let mut scratch = [0; MAX_WRITE];
        match inner.medium.poll(&mut scratch) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(Completion::Synced)) => {
                inner.active = None;
                inner.recovery = Recovery::Ready;
                inner.gate = Gate::ReadOnly;
                Poll::Ready(Ok(()))
            }
            Poll::Ready(result) => {
                // Ready ends physical ownership; the global gate still requires
                // a fresh Sync, without depending on idle terminal replay.
                inner.active = None;
                inner.fault();
                Poll::Ready(Err(PartitionError::Storage(
                    result.err().unwrap_or(Error::Corrupt),
                )))
            }
        }
    }
    /// Trust the caller's attestation that every actual owner recovered/validated.
    ///
    /// This does not scan owner formats or infer readiness. The boot integration
    /// must complete every owner recovery in read-only mode before calling it.
    ///
    /// # Errors
    /// `Reentrant` on nested access, `Busy` while a command remains active, or
    /// `NeedsRecovery` before successful coordinator Sync recovery.
    pub fn attest_normal(&mut self) -> Result<(), PartitionError> {
        let mut inner = self
            .inner
            .try_borrow_mut()
            .map_err(|_| PartitionError::Reentrant)?;
        if inner.active.is_some() {
            return Err(PartitionError::Busy);
        }
        if inner.gate != Gate::ReadOnly || inner.recovery != Recovery::Ready {
            return Err(PartitionError::NeedsRecovery);
        }
        inner.gate = Gate::Normal;
        inner.recovery = Recovery::Idle;
        Ok(())
    }
}
impl<M: AsyncMedium> Partition<'_, M> {
    fn translate<'b>(&self, op: &MediumOp<'b>) -> Result<(MediumOp<'b>, Completion), StartError> {
        let checked = |offset: u32, length: usize| -> Result<u32, StartError> {
            let length = u32::try_from(length).map_err(|_| StartError::Io)?;
            if length == 0
                || offset
                    .checked_add(length)
                    .is_none_or(|end| end > self.range.length)
            {
                return Err(StartError::Io);
            }
            self.range.start.checked_add(offset).ok_or(StartError::Io)
        };
        match *op {
            MediumOp::Read { offset, length } => {
                if length > MAX_WRITE {
                    return Err(StartError::Io);
                }
                Ok((
                    MediumOp::Read {
                        offset: checked(offset, length)?,
                        length,
                    },
                    Completion::Read(length),
                ))
            }
            MediumOp::Program { offset, bytes } => {
                if bytes.len() != self.geometry.write_size as usize
                    || !offset.is_multiple_of(self.geometry.write_size)
                {
                    return Err(StartError::Io);
                }
                Ok((
                    MediumOp::Program {
                        offset: checked(offset, bytes.len())?,
                        bytes,
                    },
                    Completion::Programmed,
                ))
            }
            MediumOp::Erase { offset } => {
                if !self.range.erase || !offset.is_multiple_of(self.geometry.erase_size) {
                    return Err(StartError::Io);
                }
                Ok((
                    MediumOp::Erase {
                        offset: checked(offset, self.geometry.erase_size as usize)?,
                    },
                    Completion::Erased,
                ))
            }
            MediumOp::Sync => Ok((MediumOp::Sync, Completion::Synced)),
        }
    }
}
impl<M: AsyncMedium> AsyncMedium for Partition<'_, M> {
    fn geometry(&self) -> Geometry {
        self.geometry
    }
    fn begin(&mut self, operation: MediumOp<'_>) -> Result<(), StartError> {
        let mut inner = self.inner.try_borrow_mut().map_err(|_| StartError::Busy)?;
        if inner.active.is_some() {
            return Err(StartError::Busy);
        }
        if inner.gate == Gate::Closed
            || (inner.gate == Gate::ReadOnly
                && matches!(operation, MediumOp::Program { .. } | MediumOp::Erase { .. }))
        {
            return Err(StartError::Io);
        }
        let (operation, expected) = self.translate(&operation)?;
        match inner.medium.begin(operation) {
            Ok(()) => {
                inner.active = Some(Active {
                    owner: self.owner,
                    expected,
                });
                Ok(())
            }
            Err(StartError::Busy) => Err(StartError::Busy),
            Err(StartError::Io) => {
                inner.active = Some(Active {
                    owner: self.owner,
                    expected,
                });
                inner.fault();
                Err(StartError::Io)
            }
        }
    }
    fn poll(&mut self, out: &mut [u8]) -> Poll<Result<Completion, Error>> {
        let Ok(mut inner) = self.inner.try_borrow_mut() else {
            return Poll::Ready(Err(Error::Conflict));
        };
        if inner
            .active
            .is_some_and(|active| active.owner != self.owner)
        {
            return Poll::Ready(Err(Error::Conflict));
        }
        if inner.gate == Gate::Closed {
            return Poll::Ready(Err(Error::NeedsRecovery));
        }
        let Some(active) = inner.active else {
            return Poll::Ready(Err(Error::Conflict));
        };
        if matches!(active.expected,Completion::Read(length) if out.len()<length) {
            return Poll::Ready(Err(Error::Capacity));
        }
        let result = match inner.medium.poll_expected(active.expected, out) {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(result) => result,
        };
        // Even an error or malformed Ready is an observed terminal boundary.
        // Keep the shared fault gate, but never repoll an already-ended command.
        inner.active = None;
        match result {
            Ok(completion) if completion == active.expected => Poll::Ready(Ok(completion)),
            other => {
                inner.fault();
                Poll::Ready(Err(other.err().unwrap_or(Error::Corrupt)))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::cell::Cell;
    #[derive(Debug)]
    struct Count<'a>(&'a Cell<usize>);
    impl AsyncMedium for Count<'_> {
        fn geometry(&self) -> Geometry {
            Geometry {
                capacity: 64,
                erase_size: 64,
                write_size: 4,
            }
        }
        fn begin(&mut self, _: MediumOp<'_>) -> Result<(), StartError> {
            self.0.set(self.0.get() + 1);
            Ok(())
        }
        fn poll(&mut self, _: &mut [u8]) -> Poll<Result<Completion, Error>> {
            self.0.set(self.0.get() + 1);
            Poll::Ready(Ok(Completion::Synced))
        }
    }
    #[test]
    fn test_should_reject_nested_access_without_panic_or_physical_io() {
        let count = Cell::new(0);
        let mut b = AsyncPartitions::new_unrecovered(
            Count(&count),
            [Range {
                start: 0,
                length: 64,
                erase: true,
            }],
        )
        .unwrap_or_else(|e| panic!("{e:?}"));
        let (mut c, [mut p]) = b.split().unwrap_or_else(|e| panic!("{e:?}"));
        let _borrow = p.inner.try_borrow_mut().unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(p.begin(MediumOp::Sync), Err(StartError::Busy));
        assert_eq!(p.poll(&mut [0; 256]), Poll::Ready(Err(Error::Conflict)));
        assert_eq!(
            c.recover_step(),
            Poll::Ready(Err(PartitionError::Reentrant))
        );
        assert_eq!(c.attest_normal(), Err(PartitionError::Reentrant));
        assert_eq!(count.get(), 0);
    }
}
