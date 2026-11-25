//! Real property dispatch through the shared owner and two-bank engine, with test-only media.
#![cfg(all(feature = "heart-rate", feature = "sport", feature = "wellbeing"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use core::cell::{Cell, RefCell};
use device_config_store::{ConfigStore, Defaults, Error as ConfigError};
use device_configuration::{HealthCapability, HeartPolicy, ProductCapabilities, Range, Zones};
use device_services::{
    PropertyAccess, PropertyBackend, PropertyFailure, PropertyOperation, PropertyService,
};
use device_storage::persistence::{AtomicSnapshot, Error, Geometry, Layout, Medium};
use std::rc::Rc;

#[derive(Clone, Debug)]
struct TestMedium {
    bytes: Rc<RefCell<Vec<u8>>>,
    fail_sync: Rc<Cell<Option<u32>>>,
    syncs: Rc<Cell<u32>>,
}
impl TestMedium {
    fn blank() -> Self {
        Self {
            bytes: Rc::new(RefCell::new(vec![0xff; 8192])),
            fail_sync: Rc::new(Cell::new(None)),
            syncs: Rc::new(Cell::new(0)),
        }
    }
}
impl Medium for TestMedium {
    fn geometry(&self) -> Geometry {
        Geometry {
            capacity: 8192,
            erase_size: 256,
            write_size: 8,
        }
    }
    fn read(&mut self, offset: u32, out: &mut [u8]) -> Result<(), Error> {
        let offset = usize::try_from(offset).map_err(|_| Error::Io)?;
        let bytes = self.bytes.borrow();
        let end = offset.checked_add(out.len()).ok_or(Error::Io)?;
        out.copy_from_slice(bytes.get(offset..end).ok_or(Error::Io)?);
        Ok(())
    }
    fn erase(&mut self, offset: u32, length: u32) -> Result<(), Error> {
        if !offset.is_multiple_of(256) || !length.is_multiple_of(256) {
            return Err(Error::Io);
        }
        let start = usize::try_from(offset).map_err(|_| Error::Io)?;
        let end =
            usize::try_from(offset.checked_add(length).ok_or(Error::Io)?).map_err(|_| Error::Io)?;
        self.bytes
            .borrow_mut()
            .get_mut(start..end)
            .ok_or(Error::Io)?
            .fill(0xff);
        Ok(())
    }
    fn program(&mut self, offset: u32, input: &[u8]) -> Result<(), Error> {
        if !offset.is_multiple_of(8) || !input.len().is_multiple_of(8) {
            return Err(Error::Io);
        }
        let start = usize::try_from(offset).map_err(|_| Error::Io)?;
        let mut bytes = self.bytes.borrow_mut();
        let end = start.checked_add(input.len()).ok_or(Error::Io)?;
        let target = bytes.get_mut(start..end).ok_or(Error::Io)?;
        for (old, new) in target.iter_mut().zip(input) {
            if *old & *new != *new {
                return Err(Error::Io);
            }
            *old &= *new;
        }
        Ok(())
    }
    fn sync(&mut self) -> Result<(), Error> {
        let count = self.syncs.get() + 1;
        self.syncs.set(count);
        if self.fail_sync.get() == Some(count) {
            self.fail_sync.set(None);
            return Err(Error::Io);
        }
        Ok(())
    }
}

#[derive(Debug)]
struct Allow;
impl PropertyAccess for Allow {
    fn allowed(&self, _: u8, _: PropertyOperation, _: Option<u16>) -> bool {
        true
    }
}
static HEALTH: [HealthCapability; 1] = [HealthCapability {
    module: 0x20,
    interval_s: Range { min: 1, max: 600 },
    temperature_modes: 0,
}];
fn caps() -> ProductCapabilities<'static> {
    ProductCapabilities {
        health: Some(&HEALTH),
        rmssd_allowed: true,
        ..ProductCapabilities::NONE
    }
}
fn layout() -> Layout {
    Layout {
        start: 0,
        bank_bytes: 4096,
    }
}
fn heart() -> HeartPolicy {
    HeartPolicy {
        enabled: true,
        mode: 1,
        interval_s: 30,
        rmssd_enabled: false,
    }
}
fn zones() -> Zones {
    Zones {
        sport_type: 2,
        zone_type: 1,
        resting: 60,
        maximum: 190,
        thresholds: [100, 120, 140, 160, 180],
    }
}
fn request(module: u8, opcode: u8, payload: &[u8]) -> device_protocol::Frame<'_> {
    device_protocol::Frame {
        header: device_protocol::Header {
            kind: device_protocol::Kind::Request,
            module,
            opcode,
            message_id: 1,
            offset: 0,
            total_length: u16::try_from(payload.len()).unwrap(),
        },
        payload,
    }
}

#[test]
fn test_should_persist_real_wire_set_and_recover_cross_alias_revision() {
    let medium = TestMedium::blank();
    let store = AtomicSnapshot::<_, 1024>::open(medium, layout()).unwrap();
    let owner = ConfigStore::open(store, caps()).unwrap();
    assert_eq!(owner.revision(), Err(ConfigError::Uninitialized));
    owner
        .initialize(Defaults {
            heart: Some(heart()),
            zones: &[zones()],
        })
        .unwrap();
    {
        let mut alias = owner.adapter(0x2a, None).unwrap();
        let mut service = PropertyService::new(0x2a, &mut alias, 128).unwrap();
        let mut out = [0xcc; 128];
        let n = service
            .dispatch(
                request(0x2a, 0x11, &[1, 0, 1, 0, 0, 0, 1, 0, 1]),
                &Allow,
                &mut out,
            )
            .unwrap();
        assert_eq!(&out[..n], &[0, 0, 0, 0, 2, 0, 0, 0]);
    }
    let medium = owner.into_store().into_medium();
    let owner = ConfigStore::open(
        AtomicSnapshot::<_, 1024>::open(medium, layout()).unwrap(),
        caps(),
    )
    .unwrap();
    let mut hr = owner.adapter(0x20, None).unwrap();
    let mut service = PropertyService::new(0x20, &mut hr, 128).unwrap();
    let mut out = [0; 128];
    let n = service
        .dispatch(request(0x20, 0x10, &[1, 0]), &Allow, &mut out)
        .unwrap();
    assert_eq!(
        &out[..n],
        &[0, 0, 0, 0, 2, 0, 0, 0, 1, 0, 7, 0, 1, 1, 30, 0, 0, 0, 1]
    );
    let n = service
        .dispatch(
            request(0x20, 0x11, &[1, 0, 1, 0, 0, 0, 7, 0, 1, 1, 60, 0, 0, 0, 0]),
            &Allow,
            &mut out,
        )
        .unwrap();
    assert_eq!(&out[..n], &[11, 0, 0, 0]);
}

#[test]
fn test_should_keep_live_sport_snapshot_while_new_zone_alias_survives_reopen() {
    let owner = ConfigStore::open(
        AtomicSnapshot::<_, 1024>::open(TestMedium::blank(), layout()).unwrap(),
        caps(),
    )
    .unwrap();
    owner
        .initialize(Defaults {
            heart: Some(heart()),
            zones: &[zones()],
        })
        .unwrap();
    let frozen = owner.capture_sport(77, 2).unwrap();
    let replacement = [
        2, 0, 1, 60, 0, 190, 0, 105, 0, 125, 0, 145, 0, 165, 0, 185, 0,
    ];
    owner
        .adapter(0x22, Some(2))
        .unwrap()
        .commit_atomic(0x22, 2, 1, &replacement, &Allow)
        .unwrap();
    assert_eq!(frozen.thresholds, [100, 120, 140, 160, 180]);
    assert_eq!(frozen.configuration_revision, 1);
    let medium = owner.into_store().into_medium();
    let owner = ConfigStore::open(
        AtomicSnapshot::<_, 1024>::open(medium, layout()).unwrap(),
        caps(),
    )
    .unwrap();
    let mut bytes = [0; 17];
    assert_eq!(
        owner
            .adapter(0x20, Some(2))
            .unwrap()
            .read(3, &mut bytes)
            .unwrap()
            .revision,
        2
    );
    assert_eq!(bytes, replacement);
    assert_eq!(
        owner.capture_sport(78, 2).unwrap().thresholds,
        [105, 125, 145, 165, 185]
    );
}

#[test]
fn test_should_never_reply_success_on_failed_barrier_and_recover_one_whole_alias_state() {
    // Mutations are immediately durable in this fixture. A final barrier failure
    // therefore demonstrates the important uncertain-new-commit case too.
    for barrier in 1..=3 {
        let medium = TestMedium::blank();
        let control = medium.clone();
        let owner = ConfigStore::open(
            AtomicSnapshot::<_, 1024>::open(medium, layout()).unwrap(),
            caps(),
        )
        .unwrap();
        owner
            .initialize(Defaults {
                heart: Some(heart()),
                zones: &[zones()],
            })
            .unwrap();
        control.fail_sync.set(Some(control.syncs.get() + barrier));
        {
            let mut alias = owner.adapter(0x2a, None).unwrap();
            let mut service = PropertyService::new(0x2a, &mut alias, 128).unwrap();
            let mut out = [0; 128];
            let n = service
                .dispatch(
                    request(0x2a, 0x11, &[1, 0, 1, 0, 0, 0, 1, 0, 1]),
                    &Allow,
                    &mut out,
                )
                .unwrap();
            assert_eq!(&out[..n], &[9, 0, 0, 0]);
            assert_eq!(
                alias.read(1, &mut out),
                Err(PropertyFailure::StorageFailure)
            );
        }
        owner.recover_and_reload().unwrap();
        let mut hr = [0; 7];
        let mut alias = [0; 1];
        let h = owner.adapter(0x20, None).unwrap().read(1, &mut hr).unwrap();
        let a = owner
            .adapter(0x2a, None)
            .unwrap()
            .read(1, &mut alias)
            .unwrap();
        assert_eq!(h.revision, a.revision);
        assert!(matches!(
            (h.revision, hr, alias),
            (1, [1, 1, 30, 0, 0, 0, 0], [0]) | (2, [1, 1, 30, 0, 0, 0, 1], [1])
        ));
    }
}
