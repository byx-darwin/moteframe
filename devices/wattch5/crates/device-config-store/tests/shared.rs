//! Shared owner and adapter regression tests against test-only fault storage.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use core::cell::Cell;
use device_config_store::{ConfigStore, Defaults, Error};
#[cfg(feature = "heart-rate")]
use device_configuration::HeartPolicy;
#[cfg(all(feature = "heart-rate", feature = "sport"))]
use device_configuration::Zones;
use device_configuration::{HealthCapability, ProductCapabilities, Range};
#[cfg(feature = "heart-rate")]
use device_services::{PropertyAccess, PropertyBackend, PropertyFailure, PropertyOperation};
use device_storage::persistence::{Error as StorageError, Snapshot, SnapshotStore};

// The real durable implementation is exercised by parent integration tests.
// This bounded fixture injects commit failures without hardware operations.
#[derive(Debug, Default)]
struct Store {
    bytes: Vec<u8>,
    revision: u64,
    fail: Cell<Option<StorageError>>,
}
impl SnapshotStore for Store {
    fn load(&self, out: &mut [u8]) -> Result<Option<Snapshot>, StorageError> {
        if self.revision == 0 {
            return Ok(None);
        }
        if out.len() < self.bytes.len() {
            return Err(StorageError::Capacity);
        }
        out[..self.bytes.len()].copy_from_slice(&self.bytes);
        Ok(Some(Snapshot {
            revision: self.revision,
            length: self.bytes.len(),
        }))
    }
    fn commit(&mut self, expected: u64, bytes: &[u8]) -> Result<Snapshot, StorageError> {
        if let Some(error) = self.fail.take() {
            return Err(error);
        }
        if expected != self.revision {
            return Err(StorageError::Conflict);
        }
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(StorageError::RevisionExhausted)?;
        self.bytes = bytes.to_vec();
        Ok(Snapshot {
            revision: self.revision,
            length: bytes.len(),
        })
    }
    fn recover(&mut self) -> Result<(), StorageError> {
        Ok(())
    }
}
static HEALTH: [HealthCapability; 1] = [HealthCapability {
    module: 0x20,
    interval_s: Range { min: 5, max: 600 },
    temperature_modes: 0,
}];
fn caps() -> ProductCapabilities<'static> {
    ProductCapabilities {
        health: Some(&HEALTH),
        rmssd_allowed: true,
        ..ProductCapabilities::NONE
    }
}
#[cfg(feature = "heart-rate")]
fn policy() -> HeartPolicy {
    HeartPolicy {
        enabled: true,
        mode: 1,
        interval_s: 30,
        rmssd_enabled: false,
    }
}
#[cfg(all(feature = "heart-rate", feature = "sport"))]
fn zones(sport_type: u16) -> Zones {
    Zones {
        sport_type,
        zone_type: 1,
        resting: 60,
        maximum: 190,
        thresholds: [100, 120, 140, 160, 180],
    }
}
#[cfg(feature = "heart-rate")]
#[derive(Debug)]
struct Access(bool);
#[cfg(feature = "heart-rate")]
impl PropertyAccess for Access {
    fn allowed(&self, _: u8, _: PropertyOperation, _: Option<u16>) -> bool {
        self.0
    }
}

#[test]
fn test_should_require_explicit_initialization_and_reject_reinitialization() {
    let owner = ConfigStore::open(Store::default(), caps()).unwrap();
    assert_eq!(owner.revision(), Err(Error::Uninitialized));
    #[cfg(not(feature = "heart-rate"))]
    let defaults = Defaults {
        heart: None,
        zones: &[],
    };
    #[cfg(feature = "heart-rate")]
    let defaults = Defaults {
        heart: Some(policy()),
        zones: &[],
    };
    owner.initialize(defaults).unwrap();
    assert_eq!(owner.revision(), Ok(1));
    assert_eq!(owner.initialize(defaults), Err(Error::AlreadyInitialized));
}
#[test]
fn test_should_expose_only_selected_routes() {
    let owner = ConfigStore::open(Store::default(), caps()).unwrap();
    assert_eq!(
        owner.adapter(0x20, None).is_ok(),
        cfg!(feature = "heart-rate")
    );
    assert_eq!(
        owner.adapter(0x22, Some(0)).is_ok(),
        cfg!(all(feature = "heart-rate", feature = "sport"))
    );
    assert_eq!(
        owner.adapter(0x2a, None).is_ok(),
        cfg!(all(feature = "heart-rate", feature = "wellbeing"))
    );
}
#[cfg(all(feature = "heart-rate", feature = "wellbeing"))]
#[test]
fn test_should_share_rmssd_policy_revision_and_reject_stale_cross_adapter_cas() {
    let owner = ConfigStore::open(Store::default(), caps()).unwrap();
    owner
        .initialize(Defaults {
            heart: Some(policy()),
            zones: &[],
        })
        .unwrap();
    let mut hr = owner.adapter(0x20, None).unwrap();
    let mut rmssd = owner.adapter(0x2a, None).unwrap();
    assert_eq!(
        rmssd
            .commit_atomic(0x2a, 1, 1, &[1], &Access(true))
            .unwrap()
            .current,
        2
    );
    let mut bytes = [0; 7];
    assert_eq!(hr.read(1, &mut bytes).unwrap().revision, 2);
    assert_eq!(bytes, [1, 1, 30, 0, 0, 0, 1]);
    assert_eq!(
        hr.commit_atomic(0x20, 1, 1, &[1, 1, 60, 0, 0, 0, 0], &Access(true)),
        Err(PropertyFailure::Conflict)
    );
    assert_eq!(
        rmssd.commit_atomic(0x2a, 1, 2, &[0], &Access(false)),
        Err(PropertyFailure::AccessDenied)
    );
    assert_eq!(rmssd.read(1, &mut bytes).unwrap().revision, 2);
}
#[cfg(all(feature = "heart-rate", feature = "sport"))]
#[test]
fn test_should_share_zone_alias_and_freeze_sport_snapshot() {
    let owner = ConfigStore::open(Store::default(), caps()).unwrap();
    owner
        .initialize(Defaults {
            heart: Some(policy()),
            zones: &[zones(2), zones(3)],
        })
        .unwrap();
    let frozen = owner.capture_sport(88, 2).unwrap();
    let hr = owner.adapter(0x20, Some(2)).unwrap();
    let mut sport = owner.adapter(0x22, Some(2)).unwrap();
    let replacement = [
        2, 0, 1, 60, 0, 190, 0, 105, 0, 125, 0, 145, 0, 165, 0, 185, 0,
    ];
    sport
        .commit_atomic(0x22, 2, 1, &replacement, &Access(true))
        .unwrap();
    let mut out = [0; 17];
    assert_eq!(hr.read(3, &mut out).unwrap().revision, 2);
    assert_eq!(out, replacement);
    assert_eq!(frozen.context_id, 88);
    assert_eq!(frozen.thresholds, [100, 120, 140, 160, 180]);
    assert_eq!(frozen.configuration_revision, 1);
    assert_eq!(
        owner.capture_sport(89, 2).unwrap().thresholds,
        [105, 125, 145, 165, 185]
    );
    assert_eq!(
        sport.commit_atomic(
            0x22,
            2,
            2,
            &[
                3, 0, 1, 60, 0, 190, 0, 105, 0, 125, 0, 145, 0, 165, 0, 185, 0
            ],
            &Access(true)
        ),
        Err(PropertyFailure::InvalidValue)
    );
}
#[cfg(feature = "heart-rate")]
#[test]
fn test_should_refuse_corrupt_schema_and_publish_only_successful_storage() {
    let store = Store {
        bytes: b"stale".to_vec(),
        revision: 1,
        fail: Cell::new(None),
    };
    assert!(matches!(
        ConfigStore::open(store, caps()),
        Err(Error::Integrity)
    ));
    let store = Store {
        fail: Cell::new(Some(StorageError::Io)),
        ..Store::default()
    };
    let owner = ConfigStore::open(store, caps()).unwrap();
    assert_eq!(
        owner.initialize(Defaults {
            heart: Some(policy()),
            zones: &[]
        }),
        Err(Error::Storage(StorageError::Io))
    );
    assert_eq!(
        owner.revision(),
        Err(Error::Storage(StorageError::NeedsRecovery))
    );
    owner.recover_and_reload().unwrap();
    assert_eq!(owner.revision(), Err(Error::Uninitialized));
}

#[cfg(feature = "heart-rate")]
#[test]
fn test_should_keep_reads_available_at_exhausted_property_revision() {
    let owner = ConfigStore::open(Store::default(), caps()).unwrap();
    owner
        .initialize(Defaults {
            heart: Some(policy()),
            zones: &[],
        })
        .unwrap();
    let mut store = owner.into_store();
    store.bytes[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
    let owner = ConfigStore::open(store, caps()).unwrap();
    let mut hr = owner.adapter(0x20, None).unwrap();
    let mut bytes = [0; 7];
    assert_eq!(hr.read(1, &mut bytes).unwrap().revision, u32::MAX);
    // GET validation must remain available when no further revision can commit.
    assert_eq!(hr.validate_value(1, &bytes), Ok(()));
    assert_eq!(
        hr.commit_atomic(0x20, 1, 0, &[1, 1, 60, 0, 0, 0, 0], &Access(true)),
        Err(PropertyFailure::RevisionExhausted)
    );
    assert_eq!(owner.revision(), Ok(1));
}

#[cfg(feature = "heart-rate")]
#[test]
fn test_should_recheck_live_authorization_while_recursive_reads_are_busy() {
    struct Reentrant<'a> {
        owner: &'a ConfigStore<'static, Store>,
        calls: Cell<u8>,
        busy: Cell<bool>,
    }
    impl PropertyAccess for Reentrant<'_> {
        fn allowed(&self, _: u8, _: PropertyOperation, key: Option<u16>) -> bool {
            self.busy.set(self.owner.revision() == Err(Error::Busy));
            self.calls.set(self.calls.get() + 1);
            key.is_none()
        }
    }

    let owner = ConfigStore::open(Store::default(), caps()).unwrap();
    owner
        .initialize(Defaults {
            heart: Some(policy()),
            zones: &[],
        })
        .unwrap();
    let access = Reentrant {
        owner: &owner,
        calls: Cell::new(0),
        busy: Cell::new(false),
    };
    let mut hr = owner.adapter(0x20, None).unwrap();
    assert_eq!(
        hr.commit_atomic(0x20, 1, 1, &[1, 1, 60, 0, 0, 0, 0], &access),
        Err(PropertyFailure::AccessDenied)
    );
    assert!(access.busy.get());
    assert_eq!(access.calls.get(), 2);
    let mut bytes = [0; 7];
    assert_eq!(hr.read(1, &mut bytes).unwrap().revision, 1);
    assert_eq!(bytes, [1, 1, 30, 0, 0, 0, 0]);
}

#[cfg(feature = "heart-rate")]
#[test]
fn test_should_validate_actual_product_bounds_and_disabled_rmssd_dependency() {
    let owner = ConfigStore::open(Store::default(), caps()).unwrap();
    owner
        .initialize(Defaults {
            heart: Some(policy()),
            zones: &[],
        })
        .unwrap();
    let mut hr = owner.adapter(0x20, None).unwrap();
    for invalid in [
        &[1, 1, 4, 0, 0, 0, 0][..],
        &[0, 1, 30, 0, 0, 0, 1],
        &[1, 0, 30, 0, 0, 0, 0],
    ] {
        assert_eq!(
            hr.commit_atomic(0x20, 1, 0, invalid, &Access(true)),
            Err(PropertyFailure::InvalidValue)
        );
    }
    let missing = ProductCapabilities {
        rmssd_allowed: false,
        ..caps()
    };
    let owner = ConfigStore::open(Store::default(), missing).unwrap();
    owner
        .initialize(Defaults {
            heart: Some(policy()),
            zones: &[],
        })
        .unwrap();
    assert_eq!(
        owner.adapter(0x20, None).unwrap().commit_atomic(
            0x20,
            1,
            0,
            &[1, 1, 30, 0, 0, 0, 1],
            &Access(true)
        ),
        Err(PropertyFailure::InvalidValue)
    );
    assert!(matches!(
        ConfigStore::open(Store::default(), ProductCapabilities::NONE),
        Err(Error::Unavailable)
    ));
}

#[cfg(feature = "heart-rate")]
#[test]
fn test_should_reopen_committed_bytes_and_reject_short_output_without_mutation() {
    let owner = ConfigStore::open(Store::default(), caps()).unwrap();
    owner
        .initialize(Defaults {
            heart: Some(policy()),
            zones: &[],
        })
        .unwrap();
    let mut hr = owner.adapter(0x20, None).unwrap();
    hr.commit_atomic(0x20, 1, 0, &[1, 1, 60, 0, 0, 0, 0], &Access(true))
        .unwrap();
    let owner = ConfigStore::open(owner.into_store(), caps()).unwrap();
    let hr = owner.adapter(0x20, None).unwrap();
    let mut short = [0xaa; 6];
    assert_eq!(hr.read(1, &mut short), Err(PropertyFailure::LimitExceeded));
    assert_eq!(short, [0xaa; 6]);
    let mut out = [0; 7];
    assert_eq!(hr.read(1, &mut out).unwrap().revision, 2);
    assert_eq!(out, [1, 1, 60, 0, 0, 0, 0]);
    assert_eq!(owner.revision(), Ok(2));
}

#[cfg(all(feature = "heart-rate", feature = "wellbeing"))]
#[test]
fn test_should_reject_rmssd_alias_when_hr_is_disabled_or_backend_absent() {
    for backend in [false, true] {
        let owner = ConfigStore::open(
            Store::default(),
            ProductCapabilities {
                rmssd_allowed: backend,
                ..caps()
            },
        )
        .unwrap();
        let policy = HeartPolicy {
            enabled: false,
            ..policy()
        };
        owner
            .initialize(Defaults {
                heart: Some(policy),
                zones: &[],
            })
            .unwrap();
        let mut rmssd = owner.adapter(0x2a, None).unwrap();
        assert_eq!(
            rmssd.commit_atomic(0x2a, 1, 0, &[1], &Access(true)),
            Err(PropertyFailure::InvalidValue)
        );
        assert_eq!(owner.revision(), Ok(1));
    }
}

#[cfg(feature = "heart-rate")]
#[test]
fn test_should_deny_a_live_service_request_at_the_commit_boundary() {
    use device_protocol::{Frame, Header, Kind};
    use device_services::{PropertyDispatchError, PropertyService};
    struct Revoking(Cell<u8>);
    impl PropertyAccess for Revoking {
        fn allowed(&self, _: u8, _: PropertyOperation, _: Option<u16>) -> bool {
            let count = self.0.get() + 1;
            self.0.set(count);
            count < 6
        }
    }
    let owner = ConfigStore::open(Store::default(), caps()).unwrap();
    owner
        .initialize(Defaults {
            heart: Some(policy()),
            zones: &[],
        })
        .unwrap();
    let mut hr = owner.adapter(0x20, None).unwrap();
    let mut service = PropertyService::new(0x20, &mut hr, 128).unwrap();
    let payload = [1, 0, 1, 0, 0, 0, 7, 0, 1, 1, 60, 0, 0, 0, 0];
    let frame = Frame {
        header: Header {
            kind: Kind::Request,
            module: 0x20,
            opcode: 0x11,
            message_id: 1,
            offset: 0,
            total_length: 15,
        },
        payload: &payload,
    };
    let mut out = [0xaa; 32];
    assert_eq!(
        service.dispatch(frame, &Revoking(Cell::new(0)), &mut out),
        Err(PropertyDispatchError::AccessRevoked)
    );
    assert_eq!(out, [0; 32]);
    assert_eq!(owner.revision(), Ok(1));
    let mut bytes = [0; 7];
    assert_eq!(hr.read(1, &mut bytes).unwrap().revision, 1);
    assert_eq!(bytes, [1, 1, 30, 0, 0, 0, 0]);
}

#[cfg(all(feature = "heart-rate", feature = "sport"))]
#[test]
fn test_should_persist_every_bounded_selector_in_deterministic_order() {
    let values: Vec<_> = (0..24).rev().map(zones).collect();
    let owner = ConfigStore::open(Store::default(), caps()).unwrap();
    owner
        .initialize(Defaults {
            heart: Some(policy()),
            zones: &values,
        })
        .unwrap();
    let store = owner.into_store();
    assert_eq!(store.bytes.len(), 523);
    let owner = ConfigStore::open(store, caps()).unwrap();
    for selector in 0..24 {
        let mut out = [0; 17];
        let hr = owner.adapter(0x20, Some(selector)).unwrap();
        assert_eq!(hr.read(3, &mut out).unwrap().revision, 1);
        assert_eq!(u16::from_le_bytes([out[0], out[1]]), selector);
    }
    assert!(matches!(
        owner.adapter(0x22, Some(24)),
        Err(Error::InvalidValue)
    ));
}

#[cfg(feature = "heart-rate")]
#[test]
fn test_should_reject_stale_layout_versions_and_zero_revisions_without_defaults() {
    let owner = ConfigStore::open(Store::default(), caps()).unwrap();
    owner
        .initialize(Defaults {
            heart: Some(policy()),
            zones: &[],
        })
        .unwrap();
    let baseline = owner.into_store().bytes;
    for (index, value) in [(4, 2), (5, 0xff), (7, 1), (8, 0)] {
        let mut bytes = baseline.clone();
        bytes[index] = value;
        let store = Store {
            bytes,
            revision: 1,
            fail: Cell::new(None),
        };
        assert!(matches!(
            ConfigStore::open(store, caps()),
            Err(Error::Integrity)
        ));
    }
}

#[cfg(feature = "heart-rate")]
#[test]
fn test_should_stop_serving_cached_configuration_after_storage_detects_corruption() {
    let owner = ConfigStore::open(Store::default(), caps()).unwrap();
    owner
        .initialize(Defaults {
            heart: Some(policy()),
            zones: &[],
        })
        .unwrap();
    let store = owner.into_store();
    store.fail.set(Some(StorageError::Corrupt));
    let owner = ConfigStore::open(store, caps()).unwrap();
    let mut hr = owner.adapter(0x20, None).unwrap();
    assert_eq!(
        hr.commit_atomic(0x20, 1, 1, &[1, 1, 60, 0, 0, 0, 0], &Access(true)),
        Err(PropertyFailure::IntegrityFailure)
    );
    assert_eq!(
        owner.revision(),
        Err(Error::Storage(StorageError::NeedsRecovery))
    );
    assert_eq!(
        hr.read(1, &mut [0; 7]),
        Err(PropertyFailure::StorageFailure)
    );
    owner.recover_and_reload().unwrap();
    assert_eq!(owner.revision(), Ok(1));
}
