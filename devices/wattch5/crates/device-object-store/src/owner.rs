use crate::{Module, State, image};
use device_storage::persistence::{Error as StorageError, SnapshotStore};
use device_storage::{ObjectSchema, StoreError};

/// Repository lifecycle or persistence failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Confirmed blank storage needs explicit initialization.
    Uninitialized,
    /// Existing durable data cannot be initialized again.
    AlreadyInitialized,
    /// Invalid capacities or module configuration.
    Invalid,
    /// Stored image violates its version, schema or invariants.
    Integrity,
    /// Actual snapshot failure, possibly requiring recovery.
    Storage(StorageError),
}
/// Exclusive bounded repository owner.
///
/// `M` bounds modules, `E` bounds all lifetime creation reservations, `B` bounds
/// each body, and `I` bounds encoded snapshot bytes. Ledger entries are never
/// reclaimed. This is single-context serialization, not interrupt locking.
pub struct DurableObjects<S, V, const M: usize, const E: usize, const B: usize, const I: usize> {
    pub(crate) store: S,
    pub(crate) schema: V,
    pub(crate) state: Option<State<M, E, B>>,
    pub(crate) revision: u64,
    pub(crate) poisoned: bool,
}
impl<S, V, const M: usize, const E: usize, const B: usize, const I: usize> core::fmt::Debug
    for DurableObjects<S, V, M, E, B, I>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DurableObjects")
            .field("revision", &self.revision)
            .field("poisoned", &self.poisoned)
            .finish_non_exhaustive()
    }
}
impl<
    S: SnapshotStore,
    V: ObjectSchema,
    const M: usize,
    const E: usize,
    const B: usize,
    const I: usize,
> DurableObjects<S, V, M, E, B, I>
{
    /// Validate and load existing storage, leaving confirmed blank media uninitialized.
    /// # Errors
    /// Returns invalid configuration, stored integrity, or actual storage failure.
    pub fn open(store: S, schema: V) -> Result<Self, Error> {
        if M == 0
            || E == 0
            || B > 4096
            || I < 44
            || u32::try_from(M).is_err()
            || u32::try_from(E).is_err()
        {
            return Err(Error::Invalid);
        }
        let mut owner = Self {
            store,
            schema,
            state: None,
            revision: 0,
            poisoned: false,
        };
        owner.reload()?;
        Ok(owner)
    }
    /// Explicitly initialize sorted unique module IDs on confirmed blank media.
    /// # Errors
    /// Rejects existing data, invalid modules, capacity or persistence failure.
    pub fn initialize(&mut self, modules: &[u8]) -> Result<(), Error> {
        if self.poisoned {
            return Err(Error::Storage(StorageError::NeedsRecovery));
        }
        if self.state.is_some() {
            return Err(Error::AlreadyInitialized);
        }
        if modules.is_empty()
            || modules.len() > M
            || modules.contains(&0)
            || modules.windows(2).any(|p| p[0] >= p[1])
        {
            return Err(Error::Invalid);
        }
        let mut state = State::empty();
        for (slot, id) in state.modules.iter_mut().zip(modules) {
            *slot = Some(Module {
                id: *id,
                high: 0,
                revision: 1,
            });
        }
        self.persist(state)
    }
    /// Reload after explicit snapshot recovery. Rebuild connection actors afterward.
    /// # Errors
    /// Returns storage or image validation failure and keeps the owner poisoned.
    pub fn recover(&mut self) -> Result<(), Error> {
        self.poisoned = true;
        self.store.recover().map_err(Error::Storage)?;
        self.reload()?;
        self.poisoned = false;
        Ok(())
    }
    /// Current complete durable image revision.
    /// # Errors
    /// Returns uninitialized or recovery-required state.
    pub fn revision(&self) -> Result<u64, Error> {
        if self.poisoned {
            Err(Error::Storage(StorageError::NeedsRecovery))
        } else if self.state.is_none() {
            Err(Error::Uninitialized)
        } else {
            Ok(self.revision)
        }
    }
    /// Release the exclusively owned snapshot store.
    pub fn into_store(self) -> S {
        self.store
    }
    pub(crate) fn ready(&self) -> Result<&State<M, E, B>, StoreError> {
        if self.poisoned {
            return Err(StoreError::Backend);
        }
        self.state.as_ref().ok_or(StoreError::InvalidState)
    }
    fn reload(&mut self) -> Result<(), Error> {
        let mut bytes = [0; I];
        match self.store.load(&mut bytes).map_err(Error::Storage)? {
            None => {
                if self.revision != 0 {
                    return Err(Error::Integrity);
                }
                self.state = None;
                self.revision = 0;
            }
            Some(s) => {
                if s.length > I || s.revision == 0 || s.revision < self.revision {
                    return Err(Error::Integrity);
                }
                self.state = Some(image::decode(&bytes[..s.length], s.revision, &self.schema)?);
                self.revision = s.revision;
            }
        }
        Ok(())
    }
    pub(crate) fn persist(&mut self, state: State<M, E, B>) -> Result<(), Error> {
        let next = self
            .revision
            .checked_add(1)
            .ok_or(Error::Storage(StorageError::RevisionExhausted))?;
        let mut bytes = [0; I];
        let length = image::encode(&state, next, &mut bytes)?;
        match self.store.commit(self.revision, &bytes[..length]) {
            Ok(s) if s.revision == next && s.length == length => {
                self.state = Some(state);
                self.revision = next;
                Ok(())
            }
            Ok(_) => {
                self.poisoned = true;
                Err(Error::Storage(StorageError::NeedsRecovery))
            }
            Err(e) => {
                if !matches!(
                    e,
                    StorageError::Capacity
                        | StorageError::Conflict
                        | StorageError::RevisionExhausted
                ) {
                    self.poisoned = true;
                }
                Err(Error::Storage(e))
            }
        }
    }
}
pub(crate) fn service_error(error: Error) -> StoreError {
    match error {
        Error::Storage(StorageError::Capacity | StorageError::RevisionExhausted) => {
            StoreError::Capacity
        }
        Error::Storage(StorageError::Conflict) => StoreError::Conflict,
        _ => StoreError::Backend,
    }
}
