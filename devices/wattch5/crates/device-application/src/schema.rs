use device_configuration::{ProductCapabilities, SchemaProvider};
use device_services::{
    PropertyAccess, PropertyBackend, PropertyDescriptor, PropertyFailure, PropertyOperation,
    PropertyValue, RevisionChange,
};
use device_storage::{ObjectSchema, StoreError};
/// Immutable product schema facts for a serialized connection; never peer supplied.
pub trait SchemaContext {
    /// Actual per-module declarations, including F000 limits and packaged dictionaries.
    fn capabilities(&self, module: u8) -> ProductCapabilities<'_>;
    /// Concrete content schemas; missing implementations must reject.
    fn external(&self) -> &dyn SchemaProvider;
}
/// Complete-object validation adapter for the existing atomic object engine.
pub struct SchemaObjects<'a, C: SchemaContext> {
    context: &'a C,
}
impl<C: SchemaContext> core::fmt::Debug for SchemaObjects<'_, C> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SchemaObjects").finish_non_exhaustive()
    }
}
impl<'a, C: SchemaContext> SchemaObjects<'a, C> {
    /// Bind an immutable, trusted schema declaration.
    pub const fn new(context: &'a C) -> Self {
        Self { context }
    }
}
impl<C: SchemaContext> ObjectSchema for SchemaObjects<'_, C> {
    fn validate(&self, module: u8, kind: u16, body: &[u8]) -> Result<(), StoreError> {
        device_configuration::validate_object_with_provider(
            module,
            kind,
            body,
            &self.context.capabilities(module),
            self.context.external(),
        )
        .map(|_| ())
        .map_err(|_| StoreError::Schema)
    }
}
/// Typed validation around a real property backend; no successful persistence is simulated.
pub struct SchemaProperties<'a, B: PropertyBackend, C: SchemaContext> {
    module: u8,
    backend: &'a mut B,
    context: &'a C,
}
impl<B: PropertyBackend, C: SchemaContext> core::fmt::Debug for SchemaProperties<'_, B, C> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SchemaProperties")
            .field("module", &self.module)
            .finish_non_exhaustive()
    }
}
impl<'a, B: PropertyBackend, C: SchemaContext> SchemaProperties<'a, B, C> {
    /// Bind a live store. Its catalogue and CAS/durability promises remain mandatory.
    pub const fn new(module: u8, backend: &'a mut B, context: &'a C) -> Self {
        Self {
            module,
            backend,
            context,
        }
    }
}
impl<B: PropertyBackend, C: SchemaContext> PropertyBackend for SchemaProperties<'_, B, C> {
    fn catalogue(&self) -> &[PropertyDescriptor] {
        self.backend.catalogue()
    }
    fn read(&self, key: u16, out: &mut [u8]) -> Result<PropertyValue, PropertyFailure> {
        let checked = (|| {
            let snapshot = self.backend.read(key, out)?;
            if snapshot.revision == 0 || snapshot.length > out.len() {
                return Err(PropertyFailure::InternalFailure);
            }
            self.validate_value(key, &out[..snapshot.length])
                .map_err(|_| PropertyFailure::IntegrityFailure)?;
            Ok(snapshot)
        })();
        if checked.is_err() {
            out.fill(0);
        }
        checked
    }
    fn validate_value(&self, key: u16, value: &[u8]) -> Result<(), PropertyFailure> {
        device_configuration::validate_property_value_with_provider(
            self.module,
            key,
            value,
            &self.context.capabilities(self.module),
            self.context.external(),
        )
        .map_err(schema_failure)?;
        self.backend.validate_value(key, value)
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
        if !access.allowed(module, PropertyOperation::Set, None)
            || !access.allowed(module, PropertyOperation::Set, Some(key))
        {
            return Err(PropertyFailure::AccessDenied);
        }
        device_configuration::validate_property_with_provider(
            module,
            key,
            value,
            &self.context.capabilities(module),
            self.context.external(),
        )
        .map_err(schema_failure)?;
        self.backend.validate_value(key, value)?;
        self.backend
            .commit_atomic(module, key, expected, value, access)
    }
}

fn schema_failure(error: device_configuration::Error) -> PropertyFailure {
    match error {
        device_configuration::Error::Unknown => PropertyFailure::NotFound,
        device_configuration::Error::Readonly => PropertyFailure::AccessDenied,
        _ => PropertyFailure::InvalidValue,
    }
}
