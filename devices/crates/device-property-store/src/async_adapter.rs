//! Confirmed reads over the boot-owned incremental engine.
use crate::{AsyncPropertyStore, PropertySchema, adapter::property_error, image};
use device_services::{
    PropertyAccess, PropertyBackend, PropertyDescriptor, PropertyFailure, PropertyValue,
    RevisionChange,
};
use device_storage::incremental::AsyncMedium;
/// Borrowed confirmed-read adapter; synchronous writes always fail closed.
pub struct AsyncPropertyAdapter<
    'owner,
    'catalogue,
    M: AsyncMedium,
    C: PropertySchema,
    const N: usize,
> {
    pub(crate) owner: &'owner AsyncPropertyStore<'catalogue, M, C, N>,
    pub(crate) module: u8,
    pub(crate) properties: &'catalogue [PropertyDescriptor],
}
impl<M: AsyncMedium, C: PropertySchema, const N: usize> core::fmt::Debug
    for AsyncPropertyAdapter<'_, '_, M, C, N>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("AsyncPropertyAdapter")
            .field("module", &self.module)
            .finish_non_exhaustive()
    }
}
impl<M: AsyncMedium, C: PropertySchema, const N: usize> PropertyBackend
    for AsyncPropertyAdapter<'_, '_, M, C, N>
{
    fn catalogue(&self) -> &[PropertyDescriptor] {
        self.properties
    }
    fn read(&self, key: u16, out: &mut [u8]) -> Result<PropertyValue, PropertyFailure> {
        let result = (|| {
            let d = self
                .owner
                .descriptor(self.module, key)
                .map_err(property_error)?;
            let mut inner = self
                .owner
                .inner
                .try_borrow_mut()
                .map_err(|_| PropertyFailure::Busy)?;
            inner.ready().map_err(property_error)?;
            self.owner
                .inspect_image(&mut inner, |snapshot, bytes| {
                    let _ = snapshot.ok_or(PropertyFailure::NotFound)?;
                    let entry = image::find(bytes, self.module, key).map_err(property_error)?;
                    let value = image::value(bytes, &entry);
                    if value.len() > usize::from(d.value_max) {
                        return Err(PropertyFailure::IntegrityFailure);
                    }
                    if out.len() < value.len() {
                        return Err(PropertyFailure::LimitExceeded);
                    }
                    out[..value.len()].copy_from_slice(value);
                    Ok(PropertyValue {
                        revision: entry.revision,
                        length: value.len(),
                    })
                })
                .map_err(property_error)?
        })();
        if result.is_err() {
            out.fill(0);
        }
        result
    }
    fn validate_value(&self, key: u16, value: &[u8]) -> Result<(), PropertyFailure> {
        let d = self
            .owner
            .descriptor(self.module, key)
            .map_err(property_error)?;
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
        _module: u8,
        _key: u16,
        _expected: u32,
        _value: &[u8],
        _access: &dyn PropertyAccess,
    ) -> Result<RevisionChange, PropertyFailure> {
        Err(PropertyFailure::StorageFailure)
    }
}
