//! Typed schema, live authorization and real-store adapter regressions.
#![cfg(all(feature = "settings", feature = "profile"))]
use core::cell::Cell;
use device_application::{SchemaContext, SchemaObjects, SchemaProperties};
use device_configuration::{ProductCapabilities, SchemaProvider};
use device_services::{
    PropertyAccess, PropertyBackend, PropertyDescriptor, PropertyFailure, PropertyOperation,
    PropertyValue, RevisionChange,
};
use device_storage::{ObjectSchema, StoreError};

#[derive(Debug)]
struct Context;
impl SchemaProvider for Context {}
impl SchemaContext for Context {
    fn capabilities(&self, _: u8) -> ProductCapabilities<'_> {
        ProductCapabilities::NONE
    }
    fn external(&self) -> &dyn SchemaProvider {
        self
    }
}
#[derive(Debug)]
struct Access(Cell<bool>);
impl PropertyAccess for Access {
    fn allowed(&self, _: u8, _: PropertyOperation, _: Option<u16>) -> bool {
        self.0.get()
    }
}
#[derive(Debug)]
struct Store {
    value: u8,
    revision: u32,
    commits: u32,
}
const CATALOGUE: [PropertyDescriptor; 1] = [PropertyDescriptor {
    key: 1,
    access: 3,
    schema: 1,
    value_max: 1,
}];
impl PropertyBackend for Store {
    fn catalogue(&self) -> &[PropertyDescriptor] {
        &CATALOGUE
    }
    fn read(&self, _: u16, out: &mut [u8]) -> Result<PropertyValue, PropertyFailure> {
        let first = out.first_mut().ok_or(PropertyFailure::LimitExceeded)?;
        *first = self.value;
        Ok(PropertyValue {
            revision: self.revision,
            length: 1,
        })
    }
    fn validate_value(&self, _: u16, _: &[u8]) -> Result<(), PropertyFailure> {
        Ok(())
    }
    fn commit_atomic(
        &mut self,
        module: u8,
        key: u16,
        expected: u32,
        value: &[u8],
        access: &dyn PropertyAccess,
    ) -> Result<RevisionChange, PropertyFailure> {
        if !access.allowed(module, PropertyOperation::Set, Some(key)) {
            return Err(PropertyFailure::AccessDenied);
        }
        if expected != 0 && expected != self.revision {
            return Err(PropertyFailure::Conflict);
        }
        let next = self
            .revision
            .checked_add(1)
            .ok_or(PropertyFailure::RevisionExhausted)?;
        let byte = *value.first().ok_or(PropertyFailure::InvalidValue)?;
        let change = RevisionChange {
            previous: self.revision,
            current: next,
        };
        self.value = byte;
        self.revision = next;
        self.commits += 1;
        Ok(change)
    }
}
#[test]
fn test_should_reject_invalid_typed_write_before_reaching_a_permissive_store() {
    let context = Context;
    let mut raw = Store {
        value: 0,
        revision: 1,
        commits: 0,
    };
    let mut store = SchemaProperties::new(4, &mut raw, &context);
    assert_eq!(
        store.commit_atomic(4, 1, 1, &[2], &Access(Cell::new(true))),
        Err(PropertyFailure::InvalidValue)
    );
    assert_eq!((raw.value, raw.revision, raw.commits), (0, 1, 0));
}
#[test]
fn test_should_validate_read_only_readback_and_preserve_live_cas_boundary() {
    let context = Context;
    let mut raw = Store {
        value: 1,
        revision: 7,
        commits: 0,
    };
    let mut out = [0; 8];
    let mut store = SchemaProperties::new(4, &mut raw, &context);
    assert_eq!(
        store.read(1, &mut out),
        Ok(PropertyValue {
            revision: 7,
            length: 1
        })
    );
    assert_eq!(out[0], 1);
    assert_eq!(
        store.commit_atomic(3, 1, 7, &[0], &Access(Cell::new(true))),
        Err(PropertyFailure::InvalidValue)
    );
    assert_eq!(
        store.commit_atomic(4, 1, 6, &[0], &Access(Cell::new(true))),
        Err(PropertyFailure::Conflict)
    );
    assert_eq!(
        store.commit_atomic(4, 1, 7, &[0], &Access(Cell::new(false))),
        Err(PropertyFailure::AccessDenied)
    );
    assert_eq!(
        store.commit_atomic(4, 1, 7, &[0], &Access(Cell::new(true))),
        Ok(RevisionChange {
            previous: 7,
            current: 8
        })
    );
    assert_eq!((raw.value, raw.revision, raw.commits), (0, 8, 1));
}
#[test]
fn test_should_reject_invalid_stored_value_without_exposing_output() {
    let context = Context;
    let mut raw = Store {
        value: 255,
        revision: 1,
        commits: 0,
    };
    let mut out = [0xa5; 8];
    let store = SchemaProperties::new(4, &mut raw, &context);
    assert_eq!(
        store.read(1, &mut out),
        Err(PropertyFailure::IntegrityFailure)
    );
    assert_eq!(out, [0; 8]);
}
#[test]
fn test_should_require_selected_object_dictionary_and_reject_unknown_routes() {
    let context = Context;
    let schema = SchemaObjects::new(&context);
    // Frozen window object requires the product's actual ID/parent dictionary.
    assert_eq!(
        schema.validate(4, 1, &[1, 0, 255, 255, 1, 0, 0]),
        Err(StoreError::Schema)
    );
    assert_eq!(schema.validate(0x40, 1, &[0]), Err(StoreError::Schema));
}
