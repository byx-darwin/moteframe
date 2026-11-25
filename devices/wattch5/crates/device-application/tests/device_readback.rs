//! Complete GET bodies use the same strict candidate battery/reset semantics as ordinary events.
#![cfg(feature = "device")]
use device_application::{SchemaContext, SchemaProperties};
use device_configuration::{ProductCapabilities, SchemaProvider};
use device_protocol::{Frame, Header, Kind};
use device_services::{
    PropertyAccess, PropertyBackend, PropertyDescriptor, PropertyFailure, PropertyOperation,
    PropertyService, PropertyValue, RevisionChange, Status,
};
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
struct Access;
impl PropertyAccess for Access {
    fn allowed(&self, _: u8, _: PropertyOperation, _: Option<u16>) -> bool {
        true
    }
}
struct Store<'a> {
    body: &'a [u8],
}
const DIRECTORY: [PropertyDescriptor; 2] = [
    PropertyDescriptor {
        key: 1,
        access: 1,
        schema: 1,
        value_max: 8,
    },
    PropertyDescriptor {
        key: 6,
        access: 1,
        schema: 1,
        value_max: 15,
    },
];
impl PropertyBackend for Store<'_> {
    fn catalogue(&self) -> &[PropertyDescriptor] {
        &DIRECTORY
    }
    fn read(&self, _: u16, out: &mut [u8]) -> Result<PropertyValue, PropertyFailure> {
        out[..self.body.len()].copy_from_slice(self.body);
        Ok(PropertyValue {
            revision: 7,
            length: self.body.len(),
        })
    }
    fn validate_value(&self, _: u16, _: &[u8]) -> Result<(), PropertyFailure> {
        Ok(())
    }
    fn commit_atomic(
        &mut self,
        _: u8,
        _: u16,
        _: u32,
        _: &[u8],
        _: &dyn PropertyAccess,
    ) -> Result<RevisionChange, PropertyFailure> {
        Err(PropertyFailure::AccessDenied)
    }
}
fn read(key: u16, body: &[u8]) -> Vec<u8> {
    let mut raw = Store { body };
    let context = Context;
    let mut typed = SchemaProperties::new(2, &mut raw, &context);
    let mut service = PropertyService::new(2, &mut typed, 128).unwrap_or_else(|e| panic!("{e:?}"));
    let key_bytes = key.to_le_bytes();
    let frame = Frame {
        header: Header {
            kind: Kind::Request,
            module: 2,
            opcode: 0x10,
            message_id: 1,
            offset: 0,
            total_length: 2,
        },
        payload: &key_bytes,
    };
    let mut out = [0xa5; 128];
    let n = service
        .dispatch(frame, &Access, &mut out)
        .unwrap_or_else(|e| panic!("{e:?}"));
    out[..n].to_vec()
}
#[test]
fn test_should_reject_invalid_battery_actual_get_body() {
    let actual = read(1, &[80, 1, 0, 5, 0, 0, 0, 15]);
    assert_eq!(actual.len(), 4);
    assert_eq!(
        &actual[..2],
        &(Status::IntegrityFailure as u16).to_le_bytes()
    );
    let bytes = [80, 1, 0, 0, 0, 0, 0, 15];
    let actual = read(1, &bytes);
    assert_eq!(actual.len(), 20);
    assert_eq!(&actual[12..], &bytes);
}
#[test]
fn test_should_read_actual_fault_reset_and_reject_false_known_unknown() {
    let mut bytes = [0; 15];
    bytes[0] = 6;
    bytes[14] = 1;
    let actual = read(6, &bytes);
    assert_eq!(actual.len(), 27);
    assert_eq!(&actual[12..], &bytes);
    bytes[0] = 0;
    let actual = read(6, &bytes);
    assert_eq!(actual.len(), 4);
    assert_eq!(
        &actual[..2],
        &(Status::IntegrityFailure as u16).to_le_bytes()
    );
}

#[test]
fn test_should_not_read_valid_elapsed_usage_when_charging_state_is_unknown() {
    let actual = read(1, &[80, 0, 0, 5, 0, 0, 0, 9]);
    assert_eq!(actual.len(), 4);
    assert_eq!(
        &actual[..2],
        &(Status::IntegrityFailure as u16).to_le_bytes()
    );
}
