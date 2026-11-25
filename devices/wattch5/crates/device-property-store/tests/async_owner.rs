//! Real incremental snapshot ownership with independently driven pending work.
#![allow(
    clippy::expect_used,
    reason = "Test fixtures and assertions panic on failure."
)]
mod async_support;
mod support;
use async_support::{CATALOGUE, DEFAULTS, Nor, Owner, Schema, engine, initialized, settle};
use core::task::Poll;
use device_property_store::{AsyncPropertyStore, Error, MutationError, PropertyStore};
use device_services::{PropertyAccess, PropertyBackend, PropertyFailure, PropertyOperation};
use device_storage::persistence::{AtomicSnapshot, Error as StorageError};

#[derive(Debug)]
struct Allow;
impl PropertyAccess for Allow {
    fn allowed(&self, _: u8, _: PropertyOperation, _: Option<u16>) -> bool {
        true
    }
}
#[test]
fn test_should_require_explicit_defaults_and_confirm_only_after_detached_background_commit() {
    let nor = Nor::new();
    let owner = AsyncPropertyStore::<_, _, 128>::open(engine(nor.clone()), CATALOGUE, Schema)
        .expect("owner");
    assert_eq!(owner.revision(), Err(Error::Uninitialized));
    let before = nor.actions();
    assert_eq!(owner.begin_initialize(&[]), Err(Error::InvalidValue));
    let token = owner.begin_initialize(DEFAULTS).expect("explicit defaults");
    assert_eq!(nor.actions(), before);
    assert_eq!(owner.result(token), Ok(Poll::Pending));
    assert_eq!(owner.take_result(token), Err(MutationError::NotReady));
    owner.detach(token).expect("detach delivery");
    assert_eq!(owner.begin_initialize(DEFAULTS), Err(Error::Busy));
    for _ in 0..2000 {
        owner.step().expect("background");
    }
    assert_eq!(owner.revision(), Ok(1));
    assert_eq!(owner.result(token), Err(MutationError::InvalidToken));
    let token = owner
        .begin_set(4, 1, 1, &[1], &Allow)
        .expect("mutation after detach");
    assert_eq!(
        settle(&owner, token)
            .expect("durable")
            .change
            .expect("change")
            .current,
        2
    );
    assert_eq!(
        owner
            .take_result(token)
            .expect("token")
            .expect("result")
            .snapshot
            .revision,
        2
    );
    assert_eq!(owner.take_result(token), Err(MutationError::InvalidToken));
}
#[test]
fn test_should_serialize_cas_and_keep_old_confirmed_reads_and_revisions_during_pending() {
    let nor = Nor::new();
    let owner = initialized(nor.clone());
    let before = nor.actions();
    let token = owner
        .begin_set(4, 1, 1, &[1, 1, 1], &Allow)
        .expect("accept");
    assert_eq!(nor.actions(), before);
    assert_eq!(
        owner.begin_set(4, 1, 1, &[0], &Allow),
        Err(PropertyFailure::Busy)
    );
    let mut bytes = [9; 8];
    for _ in 0..10 {
        assert_eq!(
            owner
                .adapter(4)
                .expect("adapter")
                .read(1, &mut bytes)
                .expect("old")
                .revision,
            1
        );
        assert_eq!(bytes[0], 0);
        assert_eq!(owner.revision(), Ok(1));
        let before = nor.actions();
        owner.step().expect("one slice");
        assert!(nor.actions() - before <= 1);
    }
    assert_eq!(
        settle(&owner, token)
            .expect("durable")
            .change
            .expect("change")
            .previous,
        1
    );
    assert_eq!(owner.revision(), Ok(2));
    assert_eq!(
        owner.begin_set(4, 1, 2, &[0], &Allow),
        Err(PropertyFailure::Busy)
    );
    owner.take_result(token).expect("token").expect("result");
    assert_eq!(
        owner.begin_set(4, 1, 1, &[0], &Allow),
        Err(PropertyFailure::Conflict)
    );
    let token2 = owner.begin_set(4, 1, 0, &[0], &Allow).expect("blind");
    assert_ne!(token, token2);
    assert_eq!(owner.result(token), Err(MutationError::InvalidToken));
    settle(&owner, token2).expect("shorter replacement");
    owner.take_result(token2).expect("token").expect("result");
    assert_eq!(
        owner
            .adapter(4)
            .expect("adapter")
            .read(1, &mut bytes)
            .expect("new")
            .length,
        1
    );
    let legacy =
        AtomicSnapshot::<_, 128>::open(owner.into_engine().into_medium(), async_support::LAYOUT)
            .expect("legacy engine");
    let legacy = PropertyStore::<_, _, 128>::open(legacy, CATALOGUE, Schema).expect("legacy owner");
    assert_eq!(legacy.revision(), Ok(3));
}
#[test]
fn test_should_reject_invalid_denied_and_capacity_before_admission_and_never_fake_sync_success() {
    struct Deny;
    impl PropertyAccess for Deny {
        fn allowed(&self, _: u8, _: PropertyOperation, _: Option<u16>) -> bool {
            false
        }
    }
    let nor = Nor::new();
    let owner = initialized(nor.clone());
    let before = nor.actions();
    assert_eq!(
        owner.begin_set(4, 1, 1, &[2], &Allow),
        Err(PropertyFailure::InvalidValue)
    );
    assert_eq!(
        owner.begin_set(4, 1, 1, &[1], &Deny),
        Err(PropertyFailure::AccessDenied)
    );
    assert_eq!(
        owner.begin_set(4, 2, 1, &[1], &Allow),
        Err(PropertyFailure::NotFound)
    );
    assert_eq!(
        owner
            .adapter(4)
            .expect("adapter")
            .commit_atomic(4, 1, 1, &[1], &Allow),
        Err(PropertyFailure::StorageFailure)
    );
    assert_eq!(nor.actions(), before);
    assert_eq!(owner.revision(), Ok(1));
}
#[test]
fn test_should_poison_uncertain_final_sync_and_recover_new_image_without_acknowledging_old_token() {
    let nor = Nor::new();
    let owner = initialized(nor.clone());
    nor.memory.arm_sync(2);
    let token = owner.begin_set(4, 1, 1, &[1], &Allow).expect("accept");
    assert_eq!(settle(&owner, token), Err(PropertyFailure::StorageFailure));
    assert_eq!(
        owner.revision(),
        Err(Error::Storage(StorageError::NeedsRecovery))
    );
    assert_eq!(
        owner.adapter(4).expect("adapter").read(1, &mut [0; 8]),
        Err(PropertyFailure::StorageFailure)
    );
    owner
        .take_result(token)
        .expect("token")
        .expect_err("uncertain");
    nor.memory.disarm();
    owner.begin_recover().expect("recover admission");
    for _ in 0..2000 {
        if owner.poll_recover().is_ready() {
            break;
        }
    }
    assert_eq!(owner.poll_recover(), Poll::Ready(Ok(())));
    assert_eq!(owner.revision(), Ok(2));
    assert_eq!(owner.result(token), Err(MutationError::InvalidToken));
}
#[test]
fn test_should_detach_terminal_and_pending_tokens_without_borrowing_inner_state() {
    let owner: Owner = initialized(Nor::new());
    let token = owner.begin_set(4, 1, 1, &[1], &Allow).expect("accept");
    settle(&owner, token).expect("durable");
    owner.detach(token).expect("detach terminal");
    let next = owner
        .begin_set(4, 1, 2, &[0], &Allow)
        .expect("terminal released on next admission");
    assert_eq!(owner.detach(token), Err(MutationError::InvalidToken));
    owner.detach(next).expect("detach pending");
    for _ in 0..2000 {
        owner.step().expect("background");
    }
    assert_eq!(owner.revision(), Ok(3));
}
#[test]
fn test_should_refuse_blank_recovery_below_confirmed_floor_and_remain_poisoned() {
    use device_storage::persistence::Medium;
    let nor = Nor::new();
    let owner = initialized(nor.clone());
    let mut disk = nor.memory.clone();
    disk.erase(0, 2048).expect("model loses confirmed banks");
    owner.begin_recover().expect("recovery");
    let mut result = Poll::Pending;
    for _ in 0..2000 {
        result = owner.poll_recover();
        if result.is_ready() {
            break;
        }
    }
    assert_eq!(result, Poll::Ready(Err(Error::Integrity)));
    assert_eq!(
        owner.revision(),
        Err(Error::Storage(StorageError::NeedsRecovery))
    );
    assert_eq!(
        owner.begin_initialize(DEFAULTS),
        Err(Error::Storage(StorageError::NeedsRecovery))
    );
}
#[test]
fn test_should_refuse_to_adopt_an_engine_with_a_commit_owned_by_another_lifecycle() {
    let owner = initialized(Nor::new());
    let mut engine = owner.into_engine();
    let mut bytes = [0; 128];
    let snapshot = engine
        .load_committed(&mut bytes)
        .expect("load")
        .expect("initialized");
    engine
        .begin_commit(snapshot.revision, &bytes[..snapshot.length])
        .expect("foreign pending commit");
    assert!(matches!(
        AsyncPropertyStore::<_, _, 128>::open(engine, CATALOGUE, Schema),
        Err(Error::Busy)
    ));
}
#[test]
fn test_should_poison_invalid_completion_or_failed_readback_and_require_explicit_recovery() {
    for readback in [false, true] {
        let nor = Nor::new();
        let owner = initialized(nor.clone());
        if readback {
            nor.fail_final_readback();
        } else {
            nor.malformed_completion();
        }
        let token = owner.begin_set(4, 1, 1, &[1], &Allow).expect("accept");
        assert_eq!(settle(&owner, token), Err(PropertyFailure::StorageFailure));
        assert_eq!(
            owner.revision(),
            Err(Error::Storage(StorageError::NeedsRecovery))
        );
        owner
            .take_result(token)
            .expect("token")
            .expect_err("failure");
        owner.begin_recover().expect("recover");
        for _ in 0..2000 {
            if owner.poll_recover().is_ready() {
                break;
            }
        }
        assert_eq!(owner.poll_recover(), Poll::Ready(Ok(())));
        assert_eq!(owner.revision(), Ok(if readback { 2 } else { 1 }));
    }
}

#[test]
fn test_should_validate_current_schema_on_every_read_and_poison_until_checked_recovery() {
    use device_property_store::PropertySchema;
    use device_services::PropertyDescriptor;
    use std::{cell::Cell, rc::Rc};
    struct CurrentSchema(Rc<Cell<bool>>);
    impl PropertySchema for CurrentSchema {
        fn descriptor(&self, m: u8, k: u16) -> Result<PropertyDescriptor, Error> {
            Schema.descriptor(m, k)
        }
        fn validate(&self, m: u8, k: u16, value: &[u8]) -> Result<(), Error> {
            if self.0.get() {
                Err(Error::InvalidValue)
            } else {
                Schema.validate(m, k, value)
            }
        }
    }
    let invalid = Rc::new(Cell::new(false));
    let owner = AsyncPropertyStore::<_, _, 128>::open(
        engine(Nor::new()),
        CATALOGUE,
        CurrentSchema(invalid.clone()),
    )
    .expect("owner");
    let token = owner.begin_initialize(DEFAULTS).expect("initialize");
    settle(&owner, token).expect("durable");
    owner.take_result(token).expect("token").expect("result");
    let mut out = [9; 8];
    assert_eq!(
        owner
            .adapter(4)
            .expect("adapter")
            .read(1, &mut out)
            .expect("read")
            .revision,
        1
    );
    invalid.set(true);
    assert_eq!(
        owner.adapter(4).expect("adapter").read(1, &mut out),
        Err(PropertyFailure::IntegrityFailure)
    );
    assert_eq!(out, [0; 8]);
    invalid.set(false);
    assert_eq!(
        owner.adapter(4).expect("adapter").read(1, &mut out),
        Err(PropertyFailure::StorageFailure)
    );
    owner.begin_recover().expect("recover");
    for _ in 0..2000 {
        if owner.poll_recover().is_ready() {
            break;
        }
    }
    assert_eq!(owner.poll_recover(), Poll::Ready(Ok(())));
    assert_eq!(
        owner
            .adapter(4)
            .expect("adapter")
            .read(1, &mut out)
            .expect("recovered")
            .revision,
        1
    );
    let token = owner.begin_set(4, 1, 1, &[1], &Allow).expect("accept");
    invalid.set(true);
    assert_eq!(
        settle(&owner, token),
        Err(PropertyFailure::IntegrityFailure)
    );
    owner
        .take_result(token)
        .expect("token")
        .expect_err("failure");
    owner.begin_recover().expect("recover");
    for _ in 0..2000 {
        if owner.poll_recover().is_ready() {
            break;
        }
    }
    assert_eq!(owner.poll_recover(), Poll::Ready(Err(Error::Integrity)));
    assert_eq!(
        owner.revision(),
        Err(Error::Storage(StorageError::NeedsRecovery))
    );
    invalid.set(false);
    owner.begin_recover().expect("retry");
    for _ in 0..2000 {
        if owner.poll_recover().is_ready() {
            break;
        }
    }
    assert_eq!(owner.poll_recover(), Poll::Ready(Ok(())));
    assert_eq!(owner.revision(), Ok(2));
}

#[test]
fn test_should_hold_checked_owner_borrow_during_inspected_schema_and_final_authority_callbacks() {
    use device_property_store::PropertySchema;
    use device_services::PropertyDescriptor;
    use std::{cell::RefCell, rc::Rc};
    type Hook = Rc<RefCell<Option<Box<dyn Fn()>>>>;
    struct CheckedSchema(Hook);
    impl PropertySchema for CheckedSchema {
        fn descriptor(&self, m: u8, k: u16) -> Result<PropertyDescriptor, Error> {
            Schema.descriptor(m, k)
        }
        fn validate(&self, m: u8, k: u16, value: &[u8]) -> Result<(), Error> {
            if let Some(hook) = self.0.borrow().as_ref() {
                hook();
            }
            Schema.validate(m, k, value)
        }
    }
    struct RevokeOnKey;
    impl PropertyAccess for RevokeOnKey {
        fn allowed(&self, _: u8, _: PropertyOperation, key: Option<u16>) -> bool {
            key.is_none()
        }
    }
    let hook: Hook = Rc::new(RefCell::new(None));
    let owner = &*Box::leak(Box::new(
        AsyncPropertyStore::<_, _, 128>::open(
            engine(Nor::new()),
            CATALOGUE,
            CheckedSchema(hook.clone()),
        )
        .expect("owner"),
    ));
    let token = owner.begin_initialize(DEFAULTS).expect("initialize");
    settle(owner, token).expect("durable");
    owner.take_result(token).expect("token").expect("result");
    *hook.borrow_mut() = Some(Box::new(move || {
        let mut out = [9; 8];
        assert_eq!(
            owner.adapter(4).expect("adapter").read(1, &mut out),
            Err(PropertyFailure::Busy)
        );
        assert_eq!(out, [0; 8]);
        assert_eq!(owner.revision(), Err(Error::Busy));
        assert_eq!(
            owner.begin_set(4, 1, 1, &[1], &Allow),
            Err(PropertyFailure::Busy)
        );
    }));
    let mut out = [9; 8];
    assert_eq!(
        owner
            .adapter(4)
            .expect("adapter")
            .read(1, &mut out)
            .expect("read")
            .revision,
        1
    );
    // Final authority changes after full old-image validation must still deny
    // admission, leaving both the revision and physical medium untouched.
    assert_eq!(
        owner.begin_set(4, 1, 1, &[1], &RevokeOnKey),
        Err(PropertyFailure::AccessDenied)
    );
    let token = owner.begin_set(4, 1, 1, &[1], &Allow).expect("accept");
    settle(owner, token).expect("receipt validation reentrancy");
    owner.take_result(token).expect("token").expect("result");
    owner.begin_recover().expect("recover");
    for _ in 0..2000 {
        if owner.poll_recover().is_ready() {
            break;
        }
    }
    assert_eq!(owner.poll_recover(), Poll::Ready(Ok(())));
    *hook.borrow_mut() = None;
}

#[test]
fn test_should_reject_each_malformed_confirmed_image_at_open() {
    use device_storage::persistence::Snapshot;
    for defect in 0..5 {
        let owner = initialized(Nor::new());
        let mut engine = owner.into_engine();
        let mut bytes = [0; 128];
        let snapshot = engine
            .load_committed(&mut bytes)
            .expect("load")
            .expect("image");
        bytes[8..16].copy_from_slice(&2u64.to_le_bytes());
        bytes[32..36].copy_from_slice(&2u32.to_le_bytes());
        match defect {
            0 => bytes[0] = 0,
            1 => bytes[8..16].copy_from_slice(&1u64.to_le_bytes()),
            2 => bytes[28] = 2, // Catalogue schema mismatch.
            3 => bytes[16] = 2, // Incorrect complete entry count.
            _ => bytes[32..36].copy_from_slice(&1u32.to_le_bytes()), // Aggregate revision mismatch.
        }
        let token = engine
            .begin_commit(1, &bytes[..snapshot.length])
            .expect("malformed media commit");
        loop {
            if let Poll::Ready(receipt) = engine.poll_commit(token) {
                assert_eq!(
                    receipt.expect("durable").snapshot,
                    Snapshot {
                        revision: 2,
                        length: snapshot.length
                    }
                );
                break;
            }
        }
        assert!(
            matches!(
                AsyncPropertyStore::<_, _, 128>::open(engine, CATALOGUE, Schema),
                Err(Error::Integrity)
            ),
            "defect {defect}"
        );
    }
}
