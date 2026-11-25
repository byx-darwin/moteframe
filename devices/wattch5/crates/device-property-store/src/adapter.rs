//! Module handles borrow the one checked durable owner.
use crate::{Error, PropertySchema, PropertyStore, image};
use device_services::{
    PropertyAccess, PropertyBackend, PropertyDescriptor, PropertyFailure, PropertyOperation,
    PropertyValue, RevisionChange,
};
use device_storage::persistence::{Error as StorageError, SnapshotStore};
/// Module-bound property backend over one exclusively owned snapshot store.
pub struct PropertyAdapter<'owner, 'catalogue, S: SnapshotStore, C: PropertySchema, const N: usize>
{
    pub(crate) owner: &'owner PropertyStore<'catalogue, S, C, N>,
    pub(crate) module: u8,
    pub(crate) properties: &'catalogue [PropertyDescriptor],
}
impl<S: SnapshotStore, C: PropertySchema, const N: usize> core::fmt::Debug
    for PropertyAdapter<'_, '_, S, C, N>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PropertyAdapter")
            .field("module", &self.module)
            .finish_non_exhaustive()
    }
}
impl<S: SnapshotStore, C: PropertySchema, const N: usize> PropertyAdapter<'_, '_, S, C, N> {
    fn descriptor(&self, key: u16) -> Result<PropertyDescriptor, PropertyFailure> {
        self.properties
            .iter()
            .find(|d| d.key == key)
            .copied()
            .ok_or(PropertyFailure::NotFound)
    }
}
impl<S: SnapshotStore, C: PropertySchema, const N: usize> PropertyBackend
    for PropertyAdapter<'_, '_, S, C, N>
{
    fn catalogue(&self) -> &[PropertyDescriptor] {
        self.properties
    }
    fn read(&self, key: u16, out: &mut [u8]) -> Result<PropertyValue, PropertyFailure> {
        let result = (|| {
            let d = self.descriptor(key)?;
            let mut inner = self
                .owner
                .inner
                .try_borrow_mut()
                .map_err(|_| PropertyFailure::Busy)?;
            inner.ready().map_err(property_error)?;
            let bytes = &inner.bytes[..inner.length];
            let e = image::find(bytes, self.module, key).map_err(property_error)?;
            let value = image::value(bytes, &e);
            if value.len() > usize::from(d.value_max) {
                return Err(PropertyFailure::IntegrityFailure);
            }
            if self
                .owner
                .schemas
                .validate(self.module, key, value)
                .is_err()
            {
                inner.poisoned = true;
                return Err(PropertyFailure::IntegrityFailure);
            }
            if out.len() < value.len() {
                return Err(PropertyFailure::LimitExceeded);
            }
            out[..value.len()].copy_from_slice(value);
            Ok(PropertyValue {
                revision: e.revision,
                length: value.len(),
            })
        })();
        if result.is_err() {
            out.fill(0);
        }
        result
    }
    fn validate_value(&self, key: u16, value: &[u8]) -> Result<(), PropertyFailure> {
        let d = self.descriptor(key)?;
        let inner = self
            .owner
            .inner
            .try_borrow_mut()
            .map_err(|_| PropertyFailure::Busy)?;
        inner.ready().map_err(property_error)?;
        if value.len() > usize::from(d.value_max) {
            return Err(PropertyFailure::InvalidValue);
        }
        self.owner
            .schemas
            .validate(self.module, key, value)
            .map_err(property_error)
    }
    fn commit_atomic(
        &mut self,
        module: u8,
        key: u16,
        expected: u32,
        value: &[u8],
        access: &dyn PropertyAccess,
    ) -> Result<RevisionChange, PropertyFailure> {
        if module != self.module {
            return Err(PropertyFailure::InvalidValue);
        }
        let d = self.descriptor(key)?;
        let mut inner = self
            .owner
            .inner
            .try_borrow_mut()
            .map_err(|_| PropertyFailure::Busy)?;
        inner.ready().map_err(property_error)?;
        if value.len() > usize::from(d.value_max) {
            return Err(PropertyFailure::InvalidValue);
        }
        self.owner
            .schemas
            .validate(module, key, value)
            .map_err(property_error)?;
        let old = &inner.bytes[..inner.length];
        let entry = image::find(old, module, key).map_err(property_error)?;
        if expected != 0 && expected != entry.revision {
            return Err(PropertyFailure::Conflict);
        }
        let current = entry
            .revision
            .checked_add(1)
            .ok_or(PropertyFailure::RevisionExhausted)?;
        let mut bytes = [0; N];
        let length =
            image::replace(old, entry, &mut bytes, value, current).map_err(property_error)?;
        if !access.allowed(module, PropertyOperation::Set, None)
            || !access.allowed(module, PropertyOperation::Set, Some(key))
        {
            return Err(PropertyFailure::AccessDenied);
        }
        image::persist(&mut inner, bytes, length).map_err(property_error)?;
        Ok(RevisionChange {
            previous: entry.revision,
            current,
        })
    }
}
pub(crate) fn property_error(error: Error) -> PropertyFailure {
    match error {
        Error::Busy => PropertyFailure::Busy,
        Error::Uninitialized | Error::Unavailable => PropertyFailure::NotFound,
        Error::AlreadyInitialized | Error::Storage(StorageError::Conflict) => {
            PropertyFailure::Conflict
        }
        Error::InvalidCatalogue | Error::InvalidValue => PropertyFailure::InvalidValue,
        Error::Integrity | Error::Storage(StorageError::Corrupt) => {
            PropertyFailure::IntegrityFailure
        }
        Error::Storage(StorageError::Capacity) => PropertyFailure::LimitExceeded,
        Error::Storage(StorageError::RevisionExhausted) => PropertyFailure::RevisionExhausted,
        Error::Storage(_) => PropertyFailure::StorageFailure,
    }
}
