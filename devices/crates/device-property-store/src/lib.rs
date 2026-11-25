//! Bounded durable configuration over an exclusively owned snapshot store.
//!
//! Opening blank storage never creates business defaults. Catalogues, schema
//! facts and initial values are supplied explicitly by the trusted integration.
#![no_std]
#![forbid(unsafe_code)]

use core::{cell::RefCell, fmt};
use device_services::PropertyDescriptor;
use device_storage::persistence::{Error as StorageError, SnapshotStore};
mod adapter;
mod async_adapter;
mod async_handler;
mod async_store;
#[cfg(feature = "codecs")]
mod codecs;
mod image;
pub use adapter::PropertyAdapter;
pub use async_adapter::AsyncPropertyAdapter;
pub use async_handler::AsyncPropertyHandler;
pub use async_store::{AsyncPropertyStore, MutationError, MutationReceipt, MutationToken};
#[cfg(feature = "codecs")]
pub use codecs::{CodecSchemas, ModuleCapabilities};

/// Immutable declarations for one module; both module and key order are strict.
#[derive(Debug, Clone, Copy)]
pub struct ModuleCatalogue<'a> {
    /// Actual module number, excluding core and reserved modules.
    pub module: u8,
    /// Readable/writable persistent plain properties selected by the product.
    pub properties: &'a [PropertyDescriptor],
}
/// Explicit initial value; Debug reveals only its encoded length.
#[derive(Clone, Copy)]
pub struct DefaultValue<'a> {
    /// Selected module number.
    pub module: u8,
    /// Selected property key.
    pub key: u16,
    /// Complete actual-schema encoded value.
    pub value: &'a [u8],
}
impl fmt::Debug for DefaultValue<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DefaultValue")
            .field("module", &self.module)
            .field("key", &self.key)
            .field("length", &self.value.len())
            .finish()
    }
}
/// Configuration, validation or storage failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Confirmed blank storage requires explicit complete defaults.
    Uninitialized,
    /// Existing committed data cannot be initialized again.
    AlreadyInitialized,
    /// Another operation or recursive callback holds the owner.
    Busy,
    /// The trusted selected schema does not provide this configuration.
    Unavailable,
    /// Unsorted, duplicate, reserved, excluded or incompatible declarations.
    InvalidCatalogue,
    /// A complete value or explicit defaults fail validation.
    InvalidValue,
    /// Stored metadata, schema, layout or revision is invalid.
    Integrity,
    /// The actual snapshot operation failed.
    Storage(StorageError),
}
/// Trusted actual schema source; never implement this using peer claims.
///
/// Implementations must provide immutable descriptors and complete semantic
/// validation, including actual product dictionaries and bounds.
pub trait PropertySchema {
    /// Actual selected schema, with access 0x0b and its supported maximum.
    /// # Errors
    /// Reject unknown, uncompiled, dynamic or otherwise unavailable resources.
    fn descriptor(&self, module: u8, key: u16) -> Result<PropertyDescriptor, Error>;
    /// Validate the entire encoded value without side effects.
    /// # Errors
    /// Reject invalid encodings, fields, product ranges and unavailable schemas.
    fn validate(&self, module: u8, key: u16, value: &[u8]) -> Result<(), Error>;
}
struct Inner<S, const N: usize> {
    store: S,
    bytes: [u8; N],
    length: usize,
    revision: u64,
    floor: u64,
    initialized: bool,
    poisoned: bool,
}
impl<S, const N: usize> fmt::Debug for Inner<S, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Inner")
            .field("length", &self.length)
            .field("revision", &self.revision)
            .field("poisoned", &self.poisoned)
            .finish_non_exhaustive()
    }
}
impl<S, const N: usize> Inner<S, N> {
    fn ready(&self) -> Result<(), Error> {
        if self.poisoned {
            return Err(Error::Storage(StorageError::NeedsRecovery));
        }
        if !self.initialized {
            return Err(Error::Uninitialized);
        }
        Ok(())
    }
}
/// One serialized owner and one bounded image cache for all selected modules.
///
/// Checked borrowing supports one execution context. External synchronization
/// is required for interrupts or multiple cores. Recovery requires rebuilding
/// connection actors; it never promises authenticated rollback protection.
pub struct PropertyStore<'a, S: SnapshotStore, C: PropertySchema, const N: usize> {
    inner: RefCell<Inner<S, N>>,
    catalogue: &'a [ModuleCatalogue<'a>],
    schemas: C,
}
impl<S: SnapshotStore, C: PropertySchema, const N: usize> fmt::Debug
    for PropertyStore<'_, S, C, N>
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PropertyStore")
            .field("capacity", &N)
            .finish_non_exhaustive()
    }
}
impl<'a, S: SnapshotStore, C: PropertySchema, const N: usize> PropertyStore<'a, S, C, N> {
    /// Load existing configuration or retain confirmed blank state.
    /// # Errors
    /// Reject invalid declarations, actual storage errors and incompatible images.
    pub fn open(store: S, catalogue: &'a [ModuleCatalogue<'a>], schemas: C) -> Result<Self, Error> {
        image::validate_catalogue(catalogue, &schemas, N)?;
        let mut bytes = [0; N];
        let snapshot = image::load(&store, catalogue, &schemas, &mut bytes, 0)?;
        let (initialized, revision, length) =
            snapshot.map_or((false, 0, 0), |s| (true, s.revision, s.length));
        Ok(Self {
            inner: RefCell::new(Inner {
                store,
                bytes,
                length,
                revision,
                floor: revision,
                initialized,
                poisoned: false,
            }),
            catalogue,
            schemas,
        })
    }
    /// Persist exactly one explicit value for every selected declaration.
    /// # Errors
    /// Reject existing data, incomplete defaults, validation and real persistence failure.
    pub fn initialize(&self, defaults: &[DefaultValue<'_>]) -> Result<(), Error> {
        let mut inner = self.inner.try_borrow_mut().map_err(|_| Error::Busy)?;
        if inner.poisoned {
            return Err(Error::Storage(StorageError::NeedsRecovery));
        }
        if inner.initialized {
            return Err(Error::AlreadyInitialized);
        }
        let mut bytes = [0; N];
        let length = image::defaults(defaults, self.catalogue, &self.schemas, &mut bytes)?;
        image::persist(&mut inner, bytes, length)
    }
    /// Bind a module to the same owner and immutable catalogue.
    /// # Errors
    /// Reject modules absent from the explicitly selected catalogue.
    pub fn adapter(&self, module: u8) -> Result<PropertyAdapter<'_, 'a, S, C, N>, Error> {
        let properties = self
            .catalogue
            .iter()
            .find(|m| m.module == module)
            .ok_or(Error::Unavailable)?
            .properties;
        Ok(PropertyAdapter {
            owner: self,
            module,
            properties,
        })
    }
    /// Current whole-image durable revision.
    /// # Errors
    /// Return busy, blank or recovery-required state explicitly.
    pub fn revision(&self) -> Result<u64, Error> {
        let inner = self.inner.try_borrow().map_err(|_| Error::Busy)?;
        inner.ready()?;
        Ok(inner.revision)
    }
    /// Recover and revalidate the complete image without lowering confirmed revision.
    /// # Errors
    /// Return recovery, schema, rollback, blank-after-confirmation or borrow failure.
    pub fn recover_and_reload(&self) -> Result<(), Error> {
        let mut inner = self.inner.try_borrow_mut().map_err(|_| Error::Busy)?;
        inner.poisoned = true;
        inner.store.recover().map_err(Error::Storage)?;
        let mut bytes = [0; N];
        let snapshot = image::load(
            &inner.store,
            self.catalogue,
            &self.schemas,
            &mut bytes,
            inner.floor,
        )?;
        let (initialized, revision, length) =
            snapshot.map_or((false, 0, 0), |s| (true, s.revision, s.length));
        inner.bytes = bytes;
        inner.length = length;
        inner.revision = revision;
        inner.floor = revision;
        inner.initialized = initialized;
        inner.poisoned = false;
        Ok(())
    }
    /// Release exclusive ownership for shutdown or reopening.
    pub fn into_store(self) -> S {
        self.inner.into_inner().store
    }
}
/// Known resources that must retain their existing owners or live producers.
pub(crate) fn excluded(module: u8, key: u16) -> bool {
    matches!(module, 0 | 0x40 | 0xff)
        || key == 0xf000
        || key == u16::MAX
        || matches!(
            (module, key),
            (0x20, 1 | 3)
                | (0x22 | 0x21 | 0x38, 2)
                | (0x2a, 1..=3)
                | (2, 1..=4 | 6 | 7)
                | (4, 11)
                | (0x25, 0xf0)
                | (7, 2 | 3)
                | (8, 1)
        )
}
