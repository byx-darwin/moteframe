//! Generic ownership tests use explicit trusted declarations and real snapshot storage.
#![allow(
    clippy::expect_used,
    reason = "Test fixture construction and assertions deliberately panic on failure."
)]
mod support;
use device_property_store::{DefaultValue, Error, ModuleCatalogue, PropertySchema, PropertyStore};
use device_services::{PropertyAccess, PropertyBackend, PropertyDescriptor, PropertyOperation};
use device_storage::persistence::{AtomicSnapshot, Layout};
use support::Memory;
const BOOL: PropertyDescriptor = PropertyDescriptor {
    key: 1,
    access: 11,
    schema: 1,
    value_max: 1,
};
const CATALOGUE: &[ModuleCatalogue<'_>] = &[
    ModuleCatalogue {
        module: 4,
        properties: &[BOOL],
    },
    ModuleCatalogue {
        module: 0x55,
        properties: &[BOOL],
    },
];
const DEFAULTS: [DefaultValue<'static>; 2] = [
    DefaultValue {
        module: 4,
        key: 1,
        value: &[0],
    },
    DefaultValue {
        module: 0x55,
        key: 1,
        value: &[1],
    },
];
#[derive(Debug)]
struct Schemas;
impl PropertySchema for Schemas {
    fn descriptor(&self, module: u8, key: u16) -> Result<PropertyDescriptor, Error> {
        if [4, 0x55].contains(&module) && key == 1 {
            Ok(BOOL)
        } else {
            Err(Error::Unavailable)
        }
    }
    fn validate(&self, module: u8, key: u16, value: &[u8]) -> Result<(), Error> {
        self.descriptor(module, key)?;
        if value == [0] || value == [1] {
            Ok(())
        } else {
            Err(Error::InvalidValue)
        }
    }
}
#[derive(Debug)]
struct Allow;
impl PropertyAccess for Allow {
    fn allowed(&self, _: u8, _: PropertyOperation, _: Option<u16>) -> bool {
        true
    }
}
fn store(memory: Memory) -> AtomicSnapshot<Memory, 256> {
    AtomicSnapshot::open(
        memory,
        Layout {
            start: 0,
            bank_bytes: 1024,
        },
    )
    .expect("valid medium")
}
#[test]
fn test_should_require_explicit_complete_defaults_and_reopen_durable_cas() {
    let memory = Memory::new();
    let owner =
        PropertyStore::<_, _, 256>::open(store(memory.clone()), CATALOGUE, Schemas).expect("open");
    assert_eq!(owner.revision(), Err(Error::Uninitialized));
    assert_eq!(memory.mutations(), 0);
    assert_eq!(
        owner.initialize(&[DefaultValue {
            module: 4,
            key: 1,
            value: &[0]
        }]),
        Err(Error::InvalidValue)
    );
    owner
        .initialize(&[
            DefaultValue {
                module: 4,
                key: 1,
                value: &[0],
            },
            DefaultValue {
                module: 0x55,
                key: 1,
                value: &[1],
            },
        ])
        .expect("explicit initialize");
    let mut first = owner.adapter(4).expect("module");
    assert_eq!(
        first
            .commit_atomic(4, 1, 1, &[1], &Allow)
            .expect("commit")
            .current,
        2
    );
    assert_eq!(
        first.commit_atomic(4, 1, 1, &[0], &Allow),
        Err(device_services::PropertyFailure::Conflict)
    );
    assert_eq!(
        first
            .commit_atomic(4, 1, 0, &[0], &Allow)
            .expect("blind commit")
            .current,
        3
    );
    drop(owner.into_store());
    let reopened =
        PropertyStore::<_, _, 256>::open(store(memory), CATALOGUE, Schemas).expect("reopen");
    assert_eq!(reopened.revision(), Ok(3));
    let mut out = [9];
    assert_eq!(
        reopened
            .adapter(4)
            .expect("module")
            .read(1, &mut out)
            .expect("read")
            .revision,
        3
    );
    assert_eq!(out, [0]);
    assert_eq!(
        reopened
            .adapter(0x55)
            .expect("future module")
            .read(1, &mut out)
            .expect("read")
            .revision,
        1
    );
    assert_eq!(out, [1]);
}
#[test]
fn test_should_poison_all_handles_after_stored_schema_validation_failure() {
    use std::{cell::Cell, rc::Rc};
    #[derive(Debug)]
    struct MutableSchema(Rc<Cell<bool>>);
    impl PropertySchema for MutableSchema {
        fn descriptor(&self, m: u8, k: u16) -> Result<PropertyDescriptor, Error> {
            Schemas.descriptor(m, k)
        }
        fn validate(&self, m: u8, k: u16, b: &[u8]) -> Result<(), Error> {
            if self.0.get() {
                Err(Error::InvalidValue)
            } else {
                Schemas.validate(m, k, b)
            }
        }
    }
    let reject = Rc::new(Cell::new(false));
    let owner = PropertyStore::<_, _, 256>::open(
        store(Memory::new()),
        CATALOGUE,
        MutableSchema(reject.clone()),
    )
    .expect("open");
    initialize(&owner);
    reject.set(true);
    let mut out = [0xa5; 8];
    assert_eq!(
        owner.adapter(4).expect("module").read(1, &mut out),
        Err(device_services::PropertyFailure::IntegrityFailure)
    );
    assert_eq!(out, [0; 8]);
    assert_eq!(
        owner.revision(),
        Err(Error::Storage(
            device_storage::persistence::Error::NeedsRecovery
        ))
    );
    assert_eq!(
        owner.adapter(0x55).expect("other module").read(1, &mut out),
        Err(device_services::PropertyFailure::StorageFailure)
    );
    assert_eq!(owner.recover_and_reload(), Err(Error::Integrity));
    reject.set(false);
    owner
        .recover_and_reload()
        .expect("full validation restored");
    assert_eq!(owner.revision(), Ok(1));
}
fn initialize<S: device_storage::persistence::SnapshotStore, C: PropertySchema, const N: usize>(
    owner: &PropertyStore<'_, S, C, N>,
) {
    owner
        .initialize(&[
            DefaultValue {
                module: 4,
                key: 1,
                value: &[0],
            },
            DefaultValue {
                module: 0x55,
                key: 1,
                value: &[1],
            },
        ])
        .expect("defaults");
}
#[test]
fn test_should_recover_only_whole_values_after_all_sync_program_and_erase_cuts() {
    use device_services::PropertyFailure;
    use device_storage::persistence::Error as StorageError;
    // 54-byte property image +32-byte snapshot header +retired/new markers.
    for stage in 0..=2 {
        let initialization = stage == 0;
        for fault in 0..(3 + 89 + 1025) {
            let memory = Memory::new();
            let owner = PropertyStore::<_, _, 256>::open(store(memory.clone()), CATALOGUE, Schemas)
                .expect("open");
            if !initialization {
                initialize(&owner);
            }
            if stage == 2 {
                owner
                    .adapter(4)
                    .expect("module")
                    .commit_atomic(4, 1, 1, &[1], &Allow)
                    .expect("seed stale target");
            }
            match fault {
                0..=2 => memory.arm_sync(fault),
                3..=91 => memory.arm_program(fault - 3),
                _ => memory.arm_erase(fault - 92),
            }
            let result = if initialization {
                owner.initialize(&DEFAULTS).map_err(|_| ())
            } else {
                owner
                    .adapter(4)
                    .expect("module")
                    .commit_atomic(
                        4,
                        1,
                        if stage == 2 { 2 } else { 1 },
                        if stage == 2 { &[0] } else { &[1] },
                        &Allow,
                    )
                    .map(|_| ())
                    .map_err(|_| ())
            };
            assert!(
                result.is_err(),
                "fault {fault},initialization {initialization}"
            );
            assert_eq!(
                owner.revision(),
                Err(Error::Storage(StorageError::NeedsRecovery))
            );
            let mut out = [0xa5];
            assert_eq!(
                owner.adapter(0x55).expect("module").read(1, &mut out),
                Err(PropertyFailure::StorageFailure)
            );
            assert_eq!(out, [0]);
            memory.disarm();
            owner.recover_and_reload().expect("explicit recover");
            if initialization && owner.revision() == Err(Error::Uninitialized) {
                drop(owner.into_store());
                assert_eq!(
                    PropertyStore::<_, _, 256>::open(store(memory), CATALOGUE, Schemas)
                        .expect("restart blank")
                        .revision(),
                    Err(Error::Uninitialized)
                );
                continue;
            }
            let before = owner.revision().expect("committed old/new");
            let p = owner
                .adapter(4)
                .expect("module")
                .read(1, &mut out)
                .expect("whole value");
            if initialization {
                assert_eq!((before, p.revision, out[0]), (1, 1, 0));
            } else if stage == 1 {
                assert!([(1, 1, 0), (2, 2, 1)].contains(&(before, p.revision, out[0])));
            } else {
                assert!([(2, 2, 1), (3, 3, 0)].contains(&(before, p.revision, out[0])));
            }
            owner
                .adapter(0x55)
                .expect("other module")
                .read(1, &mut out)
                .expect("other value");
            assert_eq!(out, [1]);
            drop(owner.into_store());
            assert_eq!(
                PropertyStore::<_, _, 256>::open(store(memory), CATALOGUE, Schemas)
                    .expect("restart")
                    .revision(),
                Ok(before)
            );
        }
    }
}
#[test]
fn test_should_preserve_known_precheck_errors_and_poison_uncertain_or_bad_acknowledgements() {
    use device_services::PropertyFailure as P;
    use device_storage::persistence::Error as E;
    for (failure, expected) in [
        (E::Capacity, P::LimitExceeded),
        (E::Conflict, P::Conflict),
        (E::RevisionExhausted, P::RevisionExhausted),
        (E::Io, P::StorageFailure),
        (E::Corrupt, P::IntegrityFailure),
        (E::NeedsRecovery, P::StorageFailure),
    ] {
        let medium = support::Controlled::default();
        let owner =
            PropertyStore::<_, _, 256>::open(medium.clone(), CATALOGUE, Schemas).expect("open");
        initialize(&owner);
        medium.0.borrow_mut().commit_error = Some(failure);
        assert_eq!(
            owner
                .adapter(4)
                .expect("module")
                .commit_atomic(4, 1, 1, &[1], &Allow),
            Err(expected)
        );
        if [E::Capacity, E::Conflict, E::RevisionExhausted].contains(&failure) {
            assert_eq!(owner.revision(), Ok(1));
        } else {
            assert_eq!(owner.revision(), Err(Error::Storage(E::NeedsRecovery)));
        }
        medium.0.borrow_mut().commit_error = None;
        owner.recover_and_reload().expect("recover");
        assert_eq!(owner.revision(), Ok(1));
    }
    for bad_length in [false, true] {
        let medium = support::Controlled::default();
        let owner =
            PropertyStore::<_, _, 256>::open(medium.clone(), CATALOGUE, Schemas).expect("open");
        initialize(&owner);
        if bad_length {
            medium.0.borrow_mut().ack_length = Some(999);
        } else {
            medium.0.borrow_mut().ack_revision = Some(99);
        }
        assert_eq!(
            owner
                .adapter(4)
                .expect("module")
                .commit_atomic(4, 1, 1, &[1], &Allow),
            Err(P::StorageFailure)
        );
        assert_eq!(owner.revision(), Err(Error::Storage(E::NeedsRecovery)));
        owner
            .recover_and_reload()
            .expect("new whole committed value");
        assert_eq!(owner.revision(), Ok(2));
    }
}
#[test]
fn test_should_reject_malformed_images_schema_drift_and_recovery_below_confirmed_floor() {
    use device_storage::persistence::SnapshotStore;
    let original = support::Controlled::default();
    let owner =
        PropertyStore::<_, _, 256>::open(original.clone(), CATALOGUE, Schemas).expect("open");
    initialize(&owner);
    let image = original.0.borrow().bytes.clone();
    for mutation in 0..12 {
        let mut bytes = image.clone();
        match mutation {
            0 => bytes[0] ^= 1,
            1 => bytes[8] = 0,
            2 => bytes[16] = 1,
            3 => bytes[18] = 1,
            4 => bytes[24] = 5,
            5 => bytes[25] = 3,
            6 => bytes[26] = 2,
            7 => bytes[28] = 2,
            8 => bytes[30] = 2,
            9 => bytes[32..36].fill(0),
            10 => {
                bytes.pop();
            }
            _ => bytes.push(0),
        }
        let malformed = support::Controlled::default();
        {
            let mut m = malformed.0.borrow_mut();
            m.revision = 1;
            m.bytes = bytes;
        }
        assert!(
            matches!(
                PropertyStore::<_, _, 256>::open(malformed, CATALOGUE, Schemas),
                Err(Error::Integrity)
            ),
            "mutation {mutation}"
        );
    }
    let mut adapter = owner.adapter(4).expect("module");
    adapter
        .commit_atomic(4, 1, 1, &[1], &Allow)
        .expect("second snapshot");
    {
        let mut c = original.0.borrow_mut();
        c.bytes = image;
        c.revision = 1;
    }
    assert_eq!(owner.recover_and_reload(), Err(Error::Integrity));
    original.0.borrow_mut().revision = 0;
    assert_eq!(owner.recover_and_reload(), Err(Error::Integrity));
    assert_eq!(
        owner.revision(),
        Err(Error::Storage(
            device_storage::persistence::Error::NeedsRecovery
        ))
    );
    // A fresh lifecycle has no authenticated floor; this is documented separately.
    drop(owner.into_store());
    assert!(original.load(&mut [0; 256]).expect("blank").is_none());
}
#[test]
fn test_should_reject_invalid_catalogues_excluded_direct_calls_and_short_reads() {
    for (module, key) in [
        (0x20, 1),
        (0x20, 3),
        (0x22, 2),
        (0x2a, 1),
        (7, 2),
        (7, 3),
        (0x2a, 3),
        (2, 1),
        (4, 11),
        (0x21, 2),
        (4, 0xf000),
        (0x40, 1),
    ] {
        let d = PropertyDescriptor { key, ..BOOL };
        let m = [ModuleCatalogue {
            module,
            properties: &[d],
        }];
        assert!(matches!(
            PropertyStore::<_, _, 256>::open(store(Memory::new()), &m, Schemas),
            Err(Error::InvalidCatalogue)
        ));
    }
    for access in [0, 1, 3, 7, 15, 0xff] {
        let d = PropertyDescriptor { access, ..BOOL };
        let m = [ModuleCatalogue {
            module: 4,
            properties: &[d],
        }];
        assert!(matches!(
            PropertyStore::<_, _, 256>::open(store(Memory::new()), &m, Schemas),
            Err(Error::InvalidCatalogue)
        ));
    }
    let owner =
        PropertyStore::<_, _, 256>::open(store(Memory::new()), CATALOGUE, Schemas).expect("open");
    initialize(&owner);
    assert_eq!(
        owner.adapter(4).expect("module").read(1, &mut []),
        Err(device_services::PropertyFailure::LimitExceeded)
    );
    assert_eq!(
        owner
            .adapter(4)
            .expect("module")
            .commit_atomic(0x55, 1, 1, &[1], &Allow),
        Err(device_services::PropertyFailure::InvalidValue)
    );
    for key in [3, 11, 0xf000, u16::MAX] {
        assert_eq!(
            owner
                .adapter(4)
                .expect("module")
                .commit_atomic(4, key, 1, &[1], &Allow),
            Err(device_services::PropertyFailure::NotFound)
        );
    }
    assert_eq!(owner.revision(), Ok(1));
}
#[test]
fn test_should_block_failed_recovery_reads_and_detect_real_payload_corruption() {
    let memory = Memory::new();
    let owner =
        PropertyStore::<_, _, 256>::open(store(memory.clone()), CATALOGUE, Schemas).expect("open");
    initialize(&owner);
    memory.fail_reads(true);
    assert_eq!(
        owner.recover_and_reload(),
        Err(Error::Storage(device_storage::persistence::Error::Io))
    );
    assert_eq!(
        owner.revision(),
        Err(Error::Storage(
            device_storage::persistence::Error::NeedsRecovery
        ))
    );
    memory.disarm();
    owner.recover_and_reload().expect("recovery restored");
    memory.corrupt(32 + 38);
    assert_eq!(
        owner.recover_and_reload(),
        Err(Error::Storage(device_storage::persistence::Error::Corrupt))
    );
}
#[test]
fn test_should_refuse_property_or_snapshot_revision_wrap_and_invalid_load_metadata() {
    let medium = support::Controlled::default();
    let owner = PropertyStore::<_, _, 256>::open(medium.clone(), CATALOGUE, Schemas).expect("open");
    initialize(&owner);
    drop(owner.into_store());
    {
        let mut c = medium.0.borrow_mut();
        c.revision = u64::MAX;
        c.bytes[8..16].copy_from_slice(&u64::MAX.to_le_bytes());
    }
    assert!(matches!(
        PropertyStore::<_, _, 256>::open(medium.clone(), CATALOGUE, Schemas),
        Err(Error::Integrity)
    ));
    {
        let mut c = medium.0.borrow_mut();
        c.revision = u64::from(u32::MAX);
        c.bytes[8..16].copy_from_slice(&u64::from(u32::MAX).to_le_bytes());
        c.bytes[32..36].copy_from_slice(&u32::MAX.to_le_bytes());
    }
    let owner = PropertyStore::<_, _, 256>::open(medium.clone(), CATALOGUE, Schemas)
        .expect("matching aggregate at exhausted property revision");
    assert_eq!(
        owner
            .adapter(4)
            .expect("module")
            .commit_atomic(4, 1, u32::MAX, &[1], &Allow),
        Err(device_services::PropertyFailure::RevisionExhausted)
    );
    drop(owner.into_store());
    medium.0.borrow_mut().load_length = Some(257);
    assert!(matches!(
        PropertyStore::<_, _, 256>::open(medium, CATALOGUE, Schemas),
        Err(Error::Integrity)
    ));
}
#[test]
fn test_should_reject_image_capacity_before_any_mutation_and_keep_confirmed_state() {
    #[derive(Debug)]
    struct Variable;
    impl PropertySchema for Variable {
        fn descriptor(&self, m: u8, k: u16) -> Result<PropertyDescriptor, Error> {
            Schemas
                .descriptor(m, k)
                .map(|d| PropertyDescriptor { value_max: 4, ..d })
        }
        fn validate(&self, m: u8, k: u16, b: &[u8]) -> Result<(), Error> {
            self.descriptor(m, k)?;
            if b.len() <= 4 && b.iter().all(|n| *n <= 1) {
                Ok(())
            } else {
                Err(Error::InvalidValue)
            }
        }
    }
    let properties = [PropertyDescriptor {
        value_max: 4,
        ..BOOL
    }];
    let catalogue = [ModuleCatalogue {
        module: 4,
        properties: &properties,
    }];
    let memory = Memory::new();
    let owner = PropertyStore::<_, _, 40>::open(store(memory.clone()), &catalogue, Variable)
        .expect("bounded owner");
    owner
        .initialize(&[DefaultValue {
            module: 4,
            key: 1,
            value: &[0],
        }])
        .expect("fits39bytes");
    let before = memory.mutations();
    assert_eq!(
        owner
            .adapter(4)
            .expect("module")
            .commit_atomic(4, 1, 1, &[1; 4], &Allow),
        Err(device_services::PropertyFailure::LimitExceeded)
    );
    assert_eq!(owner.revision(), Ok(1));
    assert_eq!(memory.mutations(), before);
    let mut out = [9];
    owner
        .adapter(4)
        .expect("module")
        .read(1, &mut out)
        .expect("old value");
    assert_eq!(out, [0]);
    let owner = PropertyStore::<_, _, 38>::open(store(Memory::new()), &catalogue, Variable)
        .expect("minimum record overhead fits");
    assert_eq!(
        owner.initialize(&[DefaultValue {
            module: 4,
            key: 1,
            value: &[0]
        }]),
        Err(Error::Storage(device_storage::persistence::Error::Capacity))
    );
    assert_eq!(owner.revision(), Err(Error::Uninitialized));
}
#[test]
fn test_should_reject_unsorted_duplicate_and_schema_incompatible_catalogues() {
    for properties in [
        vec![BOOL, BOOL],
        vec![PropertyDescriptor { schema: 2, ..BOOL }],
        vec![PropertyDescriptor {
            value_max: 2,
            ..BOOL
        }],
        vec![PropertyDescriptor {
            value_max: 513,
            ..BOOL
        }],
    ] {
        let catalogue = [ModuleCatalogue {
            module: 4,
            properties: &properties,
        }];
        assert!(matches!(
            PropertyStore::<_, _, 256>::open(store(Memory::new()), &catalogue, Schemas),
            Err(Error::InvalidCatalogue)
        ));
    }
    let duplicate = [CATALOGUE[0], CATALOGUE[0]];
    let unsorted = [CATALOGUE[1], CATALOGUE[0]];
    for catalogue in [&duplicate[..], &unsorted[..]] {
        assert!(matches!(
            PropertyStore::<_, _, 256>::open(store(Memory::new()), catalogue, Schemas),
            Err(Error::InvalidCatalogue)
        ));
    }
}
#[test]
fn test_should_return_busy_when_actual_schema_callback_reenters_owner() {
    use std::{cell::RefCell, rc::Rc};
    type Hook = Rc<RefCell<Option<Box<dyn Fn()>>>>;
    struct Hooked(Hook);
    impl core::fmt::Debug for Hooked {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            f.write_str("Hooked")
        }
    }
    impl PropertySchema for Hooked {
        fn descriptor(&self, m: u8, k: u16) -> Result<PropertyDescriptor, Error> {
            Schemas.descriptor(m, k)
        }
        fn validate(&self, m: u8, k: u16, b: &[u8]) -> Result<(), Error> {
            if let Some(callback) = self.0.borrow().as_ref() {
                callback();
            }
            Schemas.validate(m, k, b)
        }
    }
    let hook: Hook = Rc::new(RefCell::new(None));
    let owner = Rc::new(
        PropertyStore::<_, _, 256>::open(store(Memory::new()), CATALOGUE, Hooked(hook.clone()))
            .expect("open"),
    );
    initialize(owner.as_ref());
    let weak = Rc::downgrade(&owner);
    *hook.borrow_mut() = Some(Box::new(move || {
        let owner = weak.upgrade().expect("owner");
        assert_eq!(owner.revision(), Err(Error::Busy));
        assert_eq!(
            owner.adapter(4).expect("module").validate_value(1, &[1]),
            Err(device_services::PropertyFailure::Busy)
        );
    }));
    owner
        .adapter(4)
        .expect("module")
        .validate_value(1, &[1])
        .expect("outer prevalidation");
    assert_eq!(owner.revision(), Ok(1));
    owner
        .adapter(4)
        .expect("module")
        .commit_atomic(4, 1, 1, &[1], &Allow)
        .expect("outer commit");
    let mut out = [9];
    owner
        .adapter(4)
        .expect("module")
        .read(1, &mut out)
        .expect("outer read");
    assert_eq!(out, [1]);
}
fn crc_valid_counter_image(revision: u64, counters: [u32; 2]) -> Memory {
    use device_storage::persistence::SnapshotStore;
    let memory = Memory::new();
    let owner =
        PropertyStore::<_, _, 256>::open(store(memory.clone()), CATALOGUE, Schemas).expect("open");
    initialize(&owner);
    let mut snapshot = owner.into_store();
    let mut bytes = [0; 256];
    let metadata = snapshot
        .load(&mut bytes)
        .expect("load")
        .expect("initialized");
    bytes[8..16].copy_from_slice(&revision.to_le_bytes());
    bytes[32..36].copy_from_slice(&counters[0].to_le_bytes());
    bytes[47..51].copy_from_slice(&counters[1].to_le_bytes());
    for expected in 1..revision {
        snapshot
            .commit(expected, &bytes[..metadata.length])
            .expect("seal actual snapshot CRC");
    }
    drop(snapshot);
    memory
}
#[test]
fn test_should_reject_counter_aggregate_under_snapshot_revision() {
    let memory = crc_valid_counter_image(2, [1, 1]);
    assert!(
        matches!(
            PropertyStore::<_, _, 256>::open(store(memory), CATALOGUE, Schemas),
            Err(Error::Integrity)
        ),
        "accepted image2 with property revisions[1,1]"
    );
}
#[test]
fn test_should_reject_counter_aggregate_over_snapshot_revision() {
    let memory = crc_valid_counter_image(2, [3, 1]);
    assert!(
        matches!(
            PropertyStore::<_, _, 256>::open(store(memory), CATALOGUE, Schemas),
            Err(Error::Integrity)
        ),
        "accepted image2 with property revisions[3,1]"
    );
}
#[test]
fn test_should_accept_matching_counter_distribution_and_poison_bad_reload() {
    let memory = crc_valid_counter_image(3, [2, 2]);
    let owner = PropertyStore::<_, _, 256>::open(store(memory), CATALOGUE, Schemas)
        .expect("matching aggregate");
    assert_eq!(owner.revision(), Ok(3));
    for module in [4, 0x55] {
        let mut out = [9];
        assert_eq!(
            owner
                .adapter(module)
                .expect("module")
                .read(1, &mut out)
                .expect("read")
                .revision,
            2
        );
    }
    let medium = support::Controlled::default();
    let owner = PropertyStore::<_, _, 256>::open(medium.clone(), CATALOGUE, Schemas).expect("open");
    initialize(&owner);
    {
        let mut c = medium.0.borrow_mut();
        c.revision = 2;
        c.bytes[8..16].copy_from_slice(&2u64.to_le_bytes());
    }
    assert_eq!(owner.recover_and_reload(), Err(Error::Integrity));
    assert_eq!(
        owner.revision(),
        Err(Error::Storage(
            device_storage::persistence::Error::NeedsRecovery
        ))
    );
}
