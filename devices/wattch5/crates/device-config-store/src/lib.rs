//! One checked owner of durable configuration, with optional property adapters.
//!
//! Opening blank media does not initialize a product. Call [`ConfigStore::initialize`]
//! explicitly with product-selected defaults. Access providers are always supplied
//! by the integration at the commit boundary.
#![no_std]
#![forbid(unsafe_code)]

use core::{cell::RefCell, fmt};
use device_configuration::{HeartPolicy, ProductCapabilities, Zones};
use device_storage::persistence::{Error as StorageError, SnapshotStore};

mod adapter;
mod image;
pub use adapter::PropertyAdapter;
#[cfg(feature = "heart-rate")]
use image::validate_constraints;
use image::{defaults_state, load, persist};

/// Maximum encoded configuration size (all 24 sport selectors plus policy).
pub const MAX_ENCODED_LENGTH: usize = 523;
/// Configuration ownership, validation or durable storage failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// The medium was confirmed empty and requires explicit initialization.
    Uninitialized,
    /// Explicit defaults cannot replace an existing committed configuration.
    AlreadyInitialized,
    /// A checked borrow is already held, including a recursive callback.
    Busy,
    /// The selected build or trusted product facts do not supply this resource.
    Unavailable,
    /// A value fails the selected product's schema or selector constraints.
    InvalidValue,
    /// Stored format, schema, revision or product constraints are invalid.
    Integrity,
    /// Actual snapshot persistence failure; uncertain mutations require recovery.
    Storage(StorageError),
}

/// Explicit product defaults. Neither field grants capability or permission.
#[derive(Clone, Copy)]
pub struct Defaults<'a> {
    /// Policy is required exactly when the heart-rate feature is selected.
    pub heart: Option<HeartPolicy>,
    /// Unique sport selectors, only with both heart-rate and sport features.
    pub zones: &'a [Zones],
}
impl fmt::Debug for Defaults<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Defaults")
            .field("heart_present", &self.heart.is_some())
            .field("zones_length", &self.zones.len())
            .finish()
    }
}

#[cfg(feature = "heart-rate")]
#[derive(Clone, Copy)]
struct PolicyEntry {
    value: HeartPolicy,
    revision: u32,
}
#[cfg(all(feature = "heart-rate", feature = "sport"))]
#[derive(Clone, Copy)]
struct ZoneEntry {
    value: Zones,
    revision: u32,
}
#[derive(Clone, Copy)]
struct State {
    #[cfg(feature = "heart-rate")]
    heart: PolicyEntry,
    #[cfg(all(feature = "heart-rate", feature = "sport"))]
    zones: [Option<ZoneEntry>; 24],
}
struct Inner<S> {
    store: S,
    state: Option<State>,
    revision: u64,
    poisoned: bool,
}
#[cfg(feature = "heart-rate")]
impl fmt::Debug for PolicyEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PolicyEntry")
            .field("length", &7)
            .finish_non_exhaustive()
    }
}
#[cfg(all(feature = "heart-rate", feature = "sport"))]
impl fmt::Debug for ZoneEntry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ZoneEntry")
            .field("length", &17)
            .finish_non_exhaustive()
    }
}
impl fmt::Debug for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("State").finish_non_exhaustive()
    }
}
impl<S> fmt::Debug for Inner<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Inner").finish_non_exhaustive()
    }
}
impl<S> Inner<S> {
    fn ready(&self) -> Result<&State, Error> {
        if self.poisoned {
            return Err(Error::Storage(StorageError::NeedsRecovery));
        }
        self.state.as_ref().ok_or(Error::Uninitialized)
    }
}

/// Owns the actual store once and serializes all handles using checked borrowing.
///
/// This is a single execution-context owner, not interrupt or multi-core locking.
/// Board integration must supply any synchronization needed outside that model.
pub struct ConfigStore<'caps, S: SnapshotStore> {
    inner: RefCell<Inner<S>>,
    capabilities: ProductCapabilities<'caps>,
}
impl<S: SnapshotStore> fmt::Debug for ConfigStore<'_, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConfigStore").finish_non_exhaustive()
    }
}
impl<'caps, S: SnapshotStore> ConfigStore<'caps, S> {
    /// Load and validate existing configuration, or record confirmed empty media.
    /// Trusted capabilities are retained immutably; no backend is inferred.
    /// Here `capabilities.rmssd_allowed` declares a selected RMSSD backend, not
    /// the initial HR-enabled state. The owner checks the current/incoming HR
    /// dependency separately at each update. Backend replacement needs a new
    /// owner lifecycle with fresh trusted capabilities.
    /// # Errors
    /// Reject invalid product facts, storage failure, or stale/corrupt schemas.
    pub fn open(store: S, capabilities: ProductCapabilities<'caps>) -> Result<Self, Error> {
        #[cfg(feature = "heart-rate")]
        validate_constraints(&capabilities)?;
        let (state, revision) = load(&store, &capabilities)?;
        Ok(Self {
            inner: RefCell::new(Inner {
                store,
                state,
                revision,
                poisoned: false,
            }),
            capabilities,
        })
    }

    /// Recover ownership of the actual store for shutdown or reopening.
    pub fn into_store(self) -> S {
        self.inner.into_inner().store
    }

    /// Current full durable snapshot revision.
    /// # Errors
    /// Return uninitialized, busy, or recovery-required state explicitly.
    pub fn revision(&self) -> Result<u64, Error> {
        let inner = self.inner.try_borrow().map_err(|_| Error::Busy)?;
        inner.ready()?;
        Ok(inner.revision)
    }

    /// Persist product defaults only after opening confirmed empty media.
    /// # Errors
    /// Reject invalid defaults, existing data, recursion or real storage failure.
    pub fn initialize(&self, defaults: Defaults<'_>) -> Result<(), Error> {
        let mut inner = self.inner.try_borrow_mut().map_err(|_| Error::Busy)?;
        if inner.poisoned {
            return Err(Error::Storage(StorageError::NeedsRecovery));
        }
        if inner.state.is_some() {
            return Err(Error::AlreadyInitialized);
        }
        let state = defaults_state(defaults, &self.capabilities)?;
        persist(&mut inner, &state, &self.capabilities)
    }

    /// Explicitly recover storage and reload its whole validated configuration.
    /// RAM remains unavailable if recovery or schema validation fails.
    /// # Errors
    /// Return busy, actual recovery failure or stored integrity failure.
    pub fn recover_and_reload(&self) -> Result<(), Error> {
        let mut inner = self.inner.try_borrow_mut().map_err(|_| Error::Busy)?;
        inner.poisoned = true;
        inner.store.recover().map_err(Error::Storage)?;
        let (state, revision) = load(&inner.store, &self.capabilities)?;
        inner.state = state;
        inner.revision = revision;
        inner.poisoned = false;
        Ok(())
    }

    /// Bind an immutable module catalogue and optional sport selector.
    /// HR uses module 20h, sport 22h and RMSSD alias 2Ah. Zone GET requests
    /// carry no selector on the wire, so integration binds it here.
    /// # Errors
    /// Reject unselected modules, missing selectors or sport selectors above 23.
    pub fn adapter(
        &self,
        module: u8,
        sport_type: Option<u16>,
    ) -> Result<PropertyAdapter<'_, 'caps, S>, Error> {
        if sport_type.is_some_and(|sport| sport > 23) {
            return Err(Error::InvalidValue);
        }
        let available = match module {
            0x20 => {
                cfg!(feature = "heart-rate") && (sport_type.is_none() || cfg!(feature = "sport"))
            }
            0x22 => cfg!(all(feature = "heart-rate", feature = "sport")) && sport_type.is_some(),
            0x2a => {
                cfg!(all(feature = "heart-rate", feature = "wellbeing")) && sport_type.is_none()
            }
            _ => false,
        };
        if !available {
            return Err(Error::Unavailable);
        }
        Ok(PropertyAdapter {
            owner: self,
            module,
            sport_type,
        })
    }

    /// Capture immutable configuration for a new sport context, without samples.
    /// # Errors
    /// Reject absent selectors, unavailable state or recursive borrowing.
    #[cfg(all(feature = "heart-rate", feature = "sport"))]
    pub fn capture_sport(&self, context_id: u32, sport_type: u16) -> Result<SportSnapshot, Error> {
        let inner = self.inner.try_borrow().map_err(|_| Error::Busy)?;
        let state = inner.ready()?;
        let entry = state
            .zones
            .get(usize::from(sport_type))
            .and_then(Option::as_ref)
            .ok_or(Error::Unavailable)?;
        Ok(SportSnapshot {
            context_id,
            configuration_revision: inner.revision,
            policy_revision: state.heart.revision,
            zones_revision: entry.revision,
            sport_type,
            resting: entry.value.resting,
            maximum: entry.value.maximum,
            thresholds: entry.value.thresholds,
        })
    }
}

/// Frozen sport-start configuration; subsequent edits cannot alter this copy.
#[cfg(all(feature = "heart-rate", feature = "sport"))]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SportSnapshot {
    /// Integration-supplied session/context identifier.
    pub context_id: u32,
    /// Full durable configuration revision at capture.
    pub configuration_revision: u64,
    /// Shared HR/RMSSD policy revision at capture.
    pub policy_revision: u32,
    /// Shared HR/sport zones revision at capture.
    pub zones_revision: u32,
    /// Bound sport selector.
    pub sport_type: u16,
    /// Resting heart rate configured for this context.
    pub resting: u16,
    /// Maximum heart rate configured for this context.
    pub maximum: u16,
    /// Five immutable, strictly increasing configured thresholds.
    pub thresholds: [u16; 5],
}
#[cfg(all(feature = "heart-rate", feature = "sport"))]
impl fmt::Debug for SportSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SportSnapshot")
            .field("thresholds_length", &5)
            .finish_non_exhaustive()
    }
}
