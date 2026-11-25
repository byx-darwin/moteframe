//! Existing wire service -> real property adapter -> real `AtomicSnapshot`.
#![allow(
    clippy::expect_used,
    reason = "Test fixture construction and assertions deliberately panic on failure."
)]
mod support;
use device_property_store::{DefaultValue, Error, ModuleCatalogue, PropertySchema, PropertyStore};
use device_protocol::{Frame, Header, Kind};
use device_services::{
    PropertyAccess, PropertyBackend, PropertyDescriptor, PropertyDispatchError, PropertyOperation,
    PropertyService, Status,
};
use device_storage::persistence::{AtomicSnapshot, Layout};
use std::cell::Cell;
const D: PropertyDescriptor = PropertyDescriptor {
    key: 1,
    access: 11,
    schema: 1,
    value_max: 1,
};
const C: &[ModuleCatalogue<'_>] = &[ModuleCatalogue {
    module: 4,
    properties: &[D],
}];
#[derive(Debug)]
struct Schema;
impl PropertySchema for Schema {
    fn descriptor(&self, m: u8, k: u16) -> Result<PropertyDescriptor, Error> {
        if (m, k) == (4, 1) {
            Ok(D)
        } else {
            Err(Error::Unavailable)
        }
    }
    fn validate(&self, m: u8, k: u16, b: &[u8]) -> Result<(), Error> {
        self.descriptor(m, k)?;
        if b == [0] || b == [1] {
            Ok(())
        } else {
            Err(Error::InvalidValue)
        }
    }
}
#[derive(Debug)]
struct Access {
    calls: Cell<usize>,
    revoke_at: usize,
}
impl Access {
    fn allow() -> Self {
        Self {
            calls: Cell::new(0),
            revoke_at: usize::MAX,
        }
    }
}
impl PropertyAccess for Access {
    fn allowed(&self, _: u8, _: PropertyOperation, _: Option<u16>) -> bool {
        let next = self.calls.get() + 1;
        self.calls.set(next);
        next < self.revoke_at
    }
}
fn frame(opcode: u8, payload: &[u8]) -> Frame<'_> {
    Frame {
        header: Header {
            kind: Kind::Request,
            module: 4,
            opcode,
            message_id: 1,
            offset: 0,
            total_length: u16::try_from(payload.len()).expect("bounded"),
        },
        payload,
    }
}
fn set(expected: u32, value: u8) -> [u8; 9] {
    let r = expected.to_le_bytes();
    [1, 0, r[0], r[1], r[2], r[3], 1, 0, value]
}
fn owner(
    memory: support::Memory,
) -> PropertyStore<'static, AtomicSnapshot<support::Memory, 256>, Schema, 256> {
    let snapshot = AtomicSnapshot::open(
        memory,
        Layout {
            start: 0,
            bank_bytes: 1024,
        },
    )
    .expect("snapshot");
    let owner = PropertyStore::open(snapshot, C, Schema).expect("owner");
    owner
        .initialize(&[DefaultValue {
            module: 4,
            key: 1,
            value: &[0],
        }])
        .expect("defaults");
    owner
}
#[test]
fn test_should_preserve_exact_get_set_describe_and_error_wire_responses() {
    let memory = support::Memory::new();
    let owner = owner(memory.clone());
    let mut adapter = owner.adapter(4).expect("module");
    let mut service = PropertyService::new(4, &mut adapter, 128).expect("service");
    let mut out = [0xa5; 128];
    let n = service
        .dispatch(frame(0x10, &[1, 0]), &Access::allow(), &mut out)
        .expect("GET");
    assert_eq!(&out[..n], &[0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0, 0]);
    assert_eq!(out[n], 0xa5);
    let n = service
        .dispatch(frame(0x12, &[0, 0, 16]), &Access::allow(), &mut out)
        .expect("DESCRIBE");
    assert_eq!(&out[..n], &[0, 0, 0, 0, 255, 255, 1, 1, 0, 11, 1, 0, 1, 0]);
    let before = memory.mutations();
    for (payload, status) in [
        (set(1, 2), Status::InvalidArgument),
        (set(9, 1), Status::Conflict),
    ] {
        let n = service
            .dispatch(frame(0x11, &payload), &Access::allow(), &mut out)
            .expect("rejection");
        assert_eq!(n, 4);
        assert_eq!(&out[..2], &(status as u16).to_le_bytes());
        assert_eq!(memory.mutations(), before);
    }
    let n = service
        .dispatch(frame(0x11, &set(1, 1)), &Access::allow(), &mut out)
        .expect("SET");
    assert_eq!(&out[..n], &[0, 0, 0, 0, 2, 0, 0, 0]);
    assert!(memory.mutations() > before);
    let n = service
        .dispatch(frame(0x11, &set(2, 0)), &Access::allow(), &mut out[..7])
        .expect("short success capacity");
    assert_eq!(n, 4);
    assert_eq!(&out[..2], &(Status::LimitExceeded as u16).to_le_bytes());
    assert_eq!(owner.revision(), Ok(2));
}
#[test]
fn test_should_apply_live_scope_at_commit_and_discard_revoked_responses() {
    for revoke_at in [1, 5, 7] {
        let memory = support::Memory::new();
        let owner = owner(memory.clone());
        let before = memory.mutations();
        let mut adapter = owner.adapter(4).expect("module");
        let mut service = PropertyService::new(4, &mut adapter, 128).expect("service");
        let mut out = [0xa5; 128];
        let access = Access {
            calls: Cell::new(0),
            revoke_at,
        };
        let result = service.dispatch(frame(0x11, &set(1, 1)), &access, &mut out);
        if revoke_at == 1 {
            let n = result.expect("initial denial");
            assert_eq!(n, 4);
            assert_eq!(&out[..2], &(Status::AccessDenied as u16).to_le_bytes());
        } else {
            assert_eq!(result, Err(PropertyDispatchError::AccessRevoked));
            assert_eq!(out, [0; 128]);
        }
        if revoke_at == 7 {
            assert_eq!(owner.revision(), Ok(2));
            assert!(memory.mutations() > before);
        } else {
            assert_eq!(owner.revision(), Ok(1));
            assert_eq!(memory.mutations(), before);
        }
    }
}
#[test]
fn test_should_return_busy_for_recursive_commit_boundary_and_reject_denied_direct_commit() {
    struct Reentrant<'a>(&'a dyn Fn());
    impl PropertyAccess for Reentrant<'_> {
        fn allowed(&self, _: u8, _: PropertyOperation, _: Option<u16>) -> bool {
            (self.0)();
            true
        }
    }
    let owner = owner(support::Memory::new());
    let callback = || {
        assert_eq!(owner.revision(), Err(Error::Busy));
        let mut out = [0xa5];
        assert_eq!(
            owner.adapter(4).expect("module").read(1, &mut out),
            Err(device_services::PropertyFailure::Busy)
        );
        assert_eq!(out, [0]);
    };
    owner
        .adapter(4)
        .expect("module")
        .commit_atomic(4, 1, 1, &[1], &Reentrant(&callback))
        .expect("outer commit");
    assert_eq!(owner.revision(), Ok(2));
    let denied = Access {
        calls: Cell::new(0),
        revoke_at: 1,
    };
    assert_eq!(
        owner
            .adapter(4)
            .expect("module")
            .commit_atomic(4, 1, 2, &[0], &denied),
        Err(device_services::PropertyFailure::AccessDenied)
    );
    assert_eq!(owner.revision(), Ok(2));
}
#[test]
fn test_should_return_storage_failure_without_a_success_revision_on_uncertain_medium() {
    let memory = support::Memory::new();
    let owner = owner(memory.clone());
    let mut adapter = owner.adapter(4).expect("module");
    let mut service = PropertyService::new(4, &mut adapter, 128).expect("service");
    memory.arm_sync(2);
    let mut out = [0xa5; 128];
    let n = service
        .dispatch(frame(0x11, &set(1, 1)), &Access::allow(), &mut out)
        .expect("storage status");
    assert_eq!(n, 4);
    assert_eq!(&out[..2], &(Status::StorageFailure as u16).to_le_bytes());
    assert_eq!(
        owner.revision(),
        Err(Error::Storage(
            device_storage::persistence::Error::NeedsRecovery
        ))
    );
    memory.disarm();
    owner
        .recover_and_reload()
        .expect("recover uncertain commit");
    assert_eq!(owner.revision(), Ok(2));
}
