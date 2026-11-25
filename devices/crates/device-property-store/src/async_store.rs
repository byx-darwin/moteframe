//! Boot-owned asynchronous property persistence and detached delivery ownership.
use crate::adapter::property_error;
use crate::{AsyncPropertyAdapter, DefaultValue, Error, ModuleCatalogue, PropertySchema, image};
use core::{
    cell::{Cell, RefCell},
    fmt,
    task::Poll,
};
use device_services::{PropertyAccess, PropertyFailure, PropertyOperation, RevisionChange};
use device_storage::incremental::{AsyncError, AsyncMedium, CommitId, IncrementalSnapshot};
use device_storage::persistence::{Error as StorageError, Snapshot};
#[cfg(test)]
#[path = "async_store_tests.rs"]
mod tests;

/// Checked owner-local delivery token; never a wire request identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MutationToken(u64);
/// Durable metadata for one accepted initialization or property update.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MutationReceipt {
    /// Exact durable whole-image metadata.
    pub snapshot: Snapshot,
    /// Property transition, absent for explicit initialization.
    pub change: Option<RevisionChange>,
}
/// Delivery-token failure, separate from the terminal property outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationError {
    /// Token is stale, detached, consumed or belongs to another lifecycle.
    InvalidToken,
    /// Persistence has not reached a terminal result.
    NotReady,
    /// Another call temporarily holds the checked owner borrow.
    Busy,
}
#[derive(Debug, Clone, Copy)]
struct Mutation {
    commit: CommitId,
    expected: Snapshot,
    change: Option<RevisionChange>,
    result: Option<Result<MutationReceipt, PropertyFailure>>,
}
pub(crate) struct AsyncInner<M, const N: usize> {
    engine: IncrementalSnapshot<M, N>,
    revision: u64,
    floor: u64,
    initialized: bool,
    poisoned: bool,
    generation: u64,
    mutation: Option<Mutation>,
    recovering: bool,
    recovery_result: Option<Result<(), Error>>,
}
impl<M, const N: usize> AsyncInner<M, N> {
    pub(crate) fn ready(&self) -> Result<(), Error> {
        if self.poisoned {
            return Err(Error::Storage(StorageError::NeedsRecovery));
        }
        if !self.initialized {
            return Err(Error::Uninitialized);
        }
        Ok(())
    }
}
/// One serialized boot owner for incremental storage, UI and BLE mutations.
///
/// The engine owns both image arrays. Read-only validation borrows confirmed
/// bytes; mutation preparation still owns one N-byte candidate. Generic N and
/// schema CPU cost must be budgeted by the integration.
/// Accepted writes continue after delivery detaches. Checked borrows support
/// one execution context; interrupts/multiple cores need external synchronization.
pub struct AsyncPropertyStore<'a, M: AsyncMedium, C: PropertySchema, const N: usize> {
    pub(crate) inner: RefCell<AsyncInner<M, N>>,
    pub(crate) catalogue: &'a [ModuleCatalogue<'a>],
    pub(crate) schemas: C,
    active: Cell<Option<MutationToken>>,
    detached: Cell<bool>,
}
impl<M: AsyncMedium, C: PropertySchema, const N: usize> fmt::Debug
    for AsyncPropertyStore<'_, M, C, N>
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AsyncPropertyStore")
            .field("capacity", &N)
            .field("has_delivery", &self.active.get().is_some())
            .finish_non_exhaustive()
    }
}
impl<'a, M: AsyncMedium, C: PropertySchema, const N: usize> AsyncPropertyStore<'a, M, C, N> {
    /// Validate an already recovered engine; blank storage requires explicit defaults.
    /// # Errors
    /// Reject unrecovered storage, declarations, schema drift or malformed images.
    pub fn open(
        engine: IncrementalSnapshot<M, N>,
        catalogue: &'a [ModuleCatalogue<'a>],
        schemas: C,
    ) -> Result<Self, Error> {
        if engine.is_busy() {
            return Err(Error::Busy);
        }
        image::validate_catalogue(catalogue, &schemas, N)?;
        let snapshot = inspect(&engine, catalogue, &schemas, 0, |snapshot, _| snapshot)?;
        let revision = snapshot.map_or(0, |s| s.revision);
        Ok(Self {
            inner: RefCell::new(AsyncInner {
                engine,
                revision,
                floor: revision,
                initialized: snapshot.is_some(),
                poisoned: false,
                generation: 0,
                mutation: None,
                recovering: false,
                recovery_result: None,
            }),
            catalogue,
            schemas,
            active: Cell::new(None),
            detached: Cell::new(false),
        })
    }
    /// Accept complete trusted defaults on confirmed blank storage, without medium I/O.
    /// # Errors
    /// Reject initialized/busy/uncertain storage, incomplete defaults or capacity.
    pub fn begin_initialize(&self, defaults: &[DefaultValue<'_>]) -> Result<MutationToken, Error> {
        let mut inner = self.inner.try_borrow_mut().map_err(|_| Error::Busy)?;
        self.available(&mut inner)?;
        if inner.poisoned {
            return Err(Error::Storage(StorageError::NeedsRecovery));
        }
        if inner.initialized {
            return Err(Error::AlreadyInitialized);
        }
        let mut bytes = [0; N];
        let length = image::defaults(defaults, self.catalogue, &self.schemas, &mut bytes)?;
        self.admit(&mut inner, &bytes[..length], None)
    }
    /// Validate and accept one property update without medium I/O.
    ///
    /// Successful engine admission, immediately after live authority validation,
    /// is the authorized commit boundary. Later revocation suppresses delivery
    /// and cannot cancel accepted persistence. The caller must not report success
    /// until the retained token yields its exact durable receipt.
    /// # Errors
    /// Return actual schema, CAS, scope, capacity, busy or storage rejection.
    pub fn begin_set(
        &self,
        module: u8,
        key: u16,
        expected: u32,
        value: &[u8],
        access: &dyn PropertyAccess,
    ) -> Result<MutationToken, PropertyFailure> {
        let mut inner = self
            .inner
            .try_borrow_mut()
            .map_err(|_| PropertyFailure::Busy)?;
        self.available(&mut inner).map_err(property_error)?;
        inner.ready().map_err(property_error)?;
        let d = self.descriptor(module, key).map_err(property_error)?;
        if d.access & 2 == 0 {
            return Err(PropertyFailure::AccessDenied);
        }
        if d.access & 4 != 0 || value.len() > usize::from(d.value_max) {
            return Err(PropertyFailure::InvalidValue);
        }
        self.schemas
            .validate(module, key, value)
            .map_err(property_error)?;
        let mut bytes = [0; N];
        let snapshot = self
            .load_image(&mut inner, &mut bytes)
            .map_err(property_error)?
            .ok_or(PropertyFailure::NotFound)?;
        let entry = image::find(&bytes[..snapshot.length], module, key).map_err(property_error)?;
        if expected != 0 && expected != entry.revision {
            return Err(PropertyFailure::Conflict);
        }
        let current = entry
            .revision
            .checked_add(1)
            .ok_or(PropertyFailure::RevisionExhausted)?;
        let length = image::replace_in_place(&mut bytes, snapshot.length, entry, value, current)
            .map_err(property_error)?;
        let next = inner
            .revision
            .checked_add(1)
            .ok_or(PropertyFailure::RevisionExhausted)?;
        bytes[8..16].copy_from_slice(&next.to_le_bytes());
        if !access.allowed(module, PropertyOperation::Set, None)
            || !access.allowed(module, PropertyOperation::Set, Some(key))
        {
            return Err(PropertyFailure::AccessDenied);
        }
        self.admit(
            &mut inner,
            &bytes[..length],
            Some(RevisionChange {
                previous: entry.revision,
                current,
            }),
        )
        .map_err(property_error)
    }
    fn available(&self, inner: &mut AsyncInner<M, N>) -> Result<(), Error> {
        self.reap(inner);
        if inner.mutation.is_some() || inner.recovering {
            Err(Error::Busy)
        } else {
            Ok(())
        }
    }
    fn admit(
        &self,
        inner: &mut AsyncInner<M, N>,
        bytes: &[u8],
        change: Option<RevisionChange>,
    ) -> Result<MutationToken, Error> {
        let generation = inner
            .generation
            .checked_add(1)
            .ok_or(Error::Storage(StorageError::RevisionExhausted))?;
        let revision = inner
            .revision
            .checked_add(1)
            .ok_or(Error::Storage(StorageError::RevisionExhausted))?;
        let commit = inner
            .engine
            .begin_commit(inner.revision, bytes)
            .map_err(async_error)?;
        let token = MutationToken(generation);
        inner.generation = generation;
        inner.mutation = Some(Mutation {
            commit,
            expected: Snapshot {
                revision,
                length: bytes.len(),
            },
            change,
            result: None,
        });
        self.active.set(Some(token));
        self.detached.set(false);
        Ok(token)
    }
    /// Advance at most one engine slice, including work whose actor detached.
    /// # Errors
    /// Return actual terminal storage/schema failure; its delivery result is retained.
    pub fn step(&self) -> Result<(), Error> {
        self.step_with_scratch(&mut [0; 256])
    }
    /// Advance one accepted mutation with a scheduler-owned transfer page.
    /// The page is borrowed only for this call, including Pending/Busy.
    /// # Errors
    /// Same reentry, storage and verification failures as `step`.
    #[inline(never)]
    pub fn step_with_scratch(&self, scratch: &mut [u8; 256]) -> Result<(), Error> {
        let mut inner = self.inner.try_borrow_mut().map_err(|_| Error::Busy)?;
        self.reap(&mut inner);
        if inner.recovering {
            return match self.advance_recovery(&mut inner, scratch) {
                Poll::Pending => Ok(()),
                Poll::Ready(result) => result,
            };
        }
        let Some(mutation) = inner.mutation else {
            return Ok(());
        };
        if mutation.result.is_some() {
            return Ok(());
        }
        let Poll::Ready(result) = inner
            .engine
            .poll_commit_with_scratch(mutation.commit, scratch)
        else {
            return Ok(());
        };
        let result = match result {
            Ok(receipt)
                if receipt.id == mutation.commit && receipt.snapshot == mutation.expected =>
            {
                inspect(
                    &inner.engine,
                    self.catalogue,
                    &self.schemas,
                    inner.floor,
                    |snapshot, _| snapshot,
                )
                .and_then(|actual| {
                    if actual == Some(mutation.expected) {
                        Ok(())
                    } else {
                        Err(Error::Integrity)
                    }
                })
                .map(|()| MutationReceipt {
                    snapshot: receipt.snapshot,
                    change: mutation.change,
                })
            }
            Ok(_) => Err(Error::Storage(StorageError::NeedsRecovery)),
            Err(error) => Err(async_error(error)),
        };
        match result {
            Ok(receipt) => {
                inner.revision = receipt.snapshot.revision;
                inner.floor = inner.revision;
                inner.initialized = true;
            }
            Err(_) => inner.poisoned = true,
        }
        if let Some(pending) = inner.mutation.as_mut() {
            pending.result = Some(result.map_err(property_error));
        }
        self.reap(&mut inner);
        result.map(|_| ())
    }
    /// Inspect a retained delivery result without advancing storage or consuming it.
    /// # Errors
    /// Reject detached/stale tokens or a recursive checked borrow.
    pub fn result(
        &self,
        token: MutationToken,
    ) -> Result<Poll<Result<MutationReceipt, PropertyFailure>>, MutationError> {
        self.check_token(token)?;
        let inner = self.inner.try_borrow().map_err(|_| MutationError::Busy)?;
        let mutation = inner.mutation.ok_or(MutationError::InvalidToken)?;
        Ok(mutation.result.map_or(Poll::Pending, Poll::Ready))
    }
    /// Consume one terminal result once; an error outcome is still a consumed result.
    /// # Errors
    /// Reject pending, stale/detached tokens or a recursive checked borrow.
    pub fn take_result(
        &self,
        token: MutationToken,
    ) -> Result<Result<MutationReceipt, PropertyFailure>, MutationError> {
        self.check_token(token)?;
        let mut inner = self
            .inner
            .try_borrow_mut()
            .map_err(|_| MutationError::Busy)?;
        let result = inner
            .mutation
            .and_then(|m| m.result)
            .ok_or(MutationError::NotReady)?;
        inner.mutation = None;
        self.active.set(None);
        Ok(result)
    }
    fn check_token(&self, token: MutationToken) -> Result<(), MutationError> {
        if self.active.get() != Some(token) || self.detached.get() {
            Err(MutationError::InvalidToken)
        } else {
            Ok(())
        }
    }
    /// Detach delivery without borrowing storage or cancelling accepted work.
    ///
    /// A separate Cell records detachment even during a recursive schema/access
    /// callback holding the storage borrow. The next step/admission reaps a
    /// detached terminal result, so Drop cannot strand the single mutation slot.
    /// # Errors
    /// Reject stale, consumed or already detached tokens.
    pub fn detach(&self, token: MutationToken) -> Result<(), MutationError> {
        self.check_token(token)?;
        self.detached.set(true);
        Ok(())
    }
    fn reap(&self, inner: &mut AsyncInner<M, N>) {
        if self.detached.get() && inner.mutation.is_some_and(|m| m.result.is_some()) {
            inner.mutation = None;
            self.active.set(None);
            self.detached.set(false);
        }
    }
    /// Bind a confirmed-read module adapter to this same owner.
    /// # Errors
    /// Reject modules absent from the explicitly selected catalogue.
    pub fn adapter(&self, module: u8) -> Result<AsyncPropertyAdapter<'_, 'a, M, C, N>, Error> {
        let properties = self
            .catalogue
            .iter()
            .find(|m| m.module == module)
            .ok_or(Error::Unavailable)?
            .properties;
        Ok(AsyncPropertyAdapter {
            owner: self,
            module,
            properties,
        })
    }
    /// Current confirmed durable whole-image revision.
    /// # Errors
    /// Return actual blank, recovery-required or recursive borrow failure.
    pub fn revision(&self) -> Result<u64, Error> {
        let inner = self.inner.try_borrow().map_err(|_| Error::Busy)?;
        inner.ready()?;
        Ok(inner.revision)
    }
    /// Begin explicit recovery only after old actors/results have detached or ended.
    /// # Errors
    /// Return busy or actual engine admission failure.
    pub fn begin_recover(&self) -> Result<(), Error> {
        let mut inner = self.inner.try_borrow_mut().map_err(|_| Error::Busy)?;
        self.available(&mut inner)?;
        inner.engine.begin_recover().map_err(async_error)?;
        inner.poisoned = true;
        inner.recovering = true;
        inner.recovery_result = None;
        Ok(())
    }
    /// Poll one bounded recovery slice, retaining the terminal recovery result.
    /// # Errors
    /// Return actual storage/schema/floor failure; Unavailable means no recovery began.
    pub fn poll_recover(&self) -> Poll<Result<(), Error>> {
        let Ok(mut inner) = self.inner.try_borrow_mut() else {
            return Poll::Ready(Err(Error::Busy));
        };
        if !inner.recovering {
            return Poll::Ready(inner.recovery_result.unwrap_or(Err(Error::Unavailable)));
        }
        self.advance_recovery(&mut inner, &mut [0; 256])
    }
    fn advance_recovery(
        &self,
        inner: &mut AsyncInner<M, N>,
        scratch: &mut [u8; 256],
    ) -> Poll<Result<(), Error>> {
        let Poll::Ready(result) = inner.engine.poll_recover_with_scratch(scratch) else {
            return Poll::Pending;
        };
        let result = result.map_err(async_error).and_then(|()| {
            let snapshot = inspect(
                &inner.engine,
                self.catalogue,
                &self.schemas,
                inner.floor,
                |snapshot, _| snapshot,
            )?;
            inner.revision = snapshot.map_or(0, |s| s.revision);
            inner.floor = inner.revision;
            inner.initialized = snapshot.is_some();
            inner.poisoned = false;
            Ok(())
        });
        inner.recovering = false;
        inner.recovery_result = Some(result);
        Poll::Ready(result)
    }
    pub(crate) fn descriptor(
        &self,
        module: u8,
        key: u16,
    ) -> Result<device_services::PropertyDescriptor, Error> {
        self.catalogue
            .iter()
            .find(|m| m.module == module)
            .and_then(|m| m.properties.iter().find(|d| d.key == key))
            .copied()
            .ok_or(Error::Unavailable)
    }
    pub(crate) fn load_image(
        &self,
        inner: &mut AsyncInner<M, N>,
        out: &mut [u8; N],
    ) -> Result<Option<Snapshot>, Error> {
        self.inspect_image(inner, |snapshot, bytes| {
            out[..bytes.len()].copy_from_slice(bytes);
            snapshot
        })
    }
    pub(crate) fn inspect_image<R>(
        &self,
        inner: &mut AsyncInner<M, N>,
        read: impl for<'image> FnOnce(Option<Snapshot>, &'image [u8]) -> R,
    ) -> Result<R, Error> {
        let result = inspect(
            &inner.engine,
            self.catalogue,
            &self.schemas,
            inner.floor,
            read,
        );
        if result.is_err() {
            inner.poisoned = true;
        }
        result
    }
    /// Transfer exclusive engine ownership; this neither cancels nor acknowledges work.
    pub fn into_engine(self) -> IncrementalSnapshot<M, N> {
        self.inner.into_inner().engine
    }
}
fn inspect<M: AsyncMedium, C: PropertySchema, const N: usize, R>(
    engine: &IncrementalSnapshot<M, N>,
    catalogue: &[ModuleCatalogue<'_>],
    schemas: &C,
    floor: u64,
    read: impl for<'image> FnOnce(Option<Snapshot>, &'image [u8]) -> R,
) -> Result<R, Error> {
    engine
        .inspect_committed(|snapshot, bytes| {
            if let Some(snapshot) = snapshot {
                if snapshot.revision < floor || snapshot.length != bytes.len() {
                    return Err(Error::Integrity);
                }
                image::validate(bytes, snapshot.revision, catalogue, schemas)?;
            } else if floor != 0 {
                return Err(Error::Integrity);
            }
            Ok(read(snapshot, bytes))
        })
        .map_err(Error::Storage)?
}
fn async_error(error: AsyncError) -> Error {
    match error {
        AsyncError::Busy => Error::Busy,
        AsyncError::TokenExhausted => Error::Storage(StorageError::RevisionExhausted),
        AsyncError::InvalidOperation | AsyncError::InvalidCompletion => {
            Error::Storage(StorageError::NeedsRecovery)
        }
        AsyncError::Storage(error) => Error::Storage(error),
    }
}
