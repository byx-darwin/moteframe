//! Transaction and atomic-backend contract tests. Memory store is test-only, not durable.
use device_storage::{
    Authorization, Begin, Commit, ObjectEngine, ObjectMeta, ObjectSchema, ObjectStore, StoreError,
};
#[derive(Debug)]
struct Auth(bool);
impl Authorization for Auth {
    fn write_allowed(&self, _: u8) -> bool {
        self.0
    }
}
#[derive(Debug)]
struct Schema;
impl ObjectSchema for Schema {
    fn validate(&self, _: u8, kind: u16, body: &[u8]) -> Result<(), StoreError> {
        if kind == 1 && body.len() == 3 && body[0] != 0 {
            Ok(())
        } else {
            Err(StoreError::Schema)
        }
    }
}
#[derive(Debug, Default)]
struct Memory {
    high: u32,
    meta: Option<ObjectMeta>,
    bytes: Vec<u8>,
}
impl ObjectStore for Memory {
    fn reserve(&mut self, b: &Begin, authorization: &dyn Authorization) -> Result<(), StoreError> {
        if !authorization.write_allowed(b.module) {
            return Err(StoreError::Denied);
        }
        if b.object_id <= self.high {
            return Err(StoreError::Conflict);
        }
        self.high = b.object_id;
        Ok(())
    }
    fn stat(&self, _: u8, id: u32) -> Result<ObjectMeta, StoreError> {
        self.meta
            .filter(|m| m.object_id == id)
            .ok_or(StoreError::NotFound)
    }
    fn commit(
        &mut self,
        c: &Commit,
        body: &[u8],
        authorization: &dyn Authorization,
    ) -> Result<ObjectMeta, StoreError> {
        if !authorization.write_allowed(c.begin.module) {
            return Err(StoreError::Denied);
        }
        if c.begin.create {
            if self.meta.is_some() {
                return Err(StoreError::Conflict);
            }
        } else if self.stat(c.begin.module, c.begin.object_id)?.revision
            != c.begin.expected_revision
        {
            return Err(StoreError::Conflict);
        }
        let revision = if c.begin.create {
            1
        } else {
            c.begin.expected_revision + 1
        };
        let meta = ObjectMeta {
            object_id: c.begin.object_id,
            kind: c.begin.kind,
            revision,
            total: u32::try_from(body.len()).map_err(|_| StoreError::Capacity)?,
            crc: c.crc,
            birth_token: if c.begin.create {
                c.begin.creation_token
            } else {
                self.stat(c.begin.module, c.begin.object_id)?.birth_token
            },
        };
        self.meta = Some(meta);
        self.bytes = body.to_vec();
        Ok(meta)
    }
}
fn begin(id: u32) -> Begin {
    Begin {
        module: 5,
        kind: 1,
        object_id: id,
        expected_revision: 0,
        total: 3,
        create: true,
        creation_token: [1; 16],
    }
}
#[test]
fn test_should_consume_reserved_ids_even_on_abort_and_reject_bad_creation() {
    let mut store = Memory::default();
    let mut e = ObjectEngine::<16>::new();
    let auth = Auth(true);
    let tx = e
        .begin(begin(7), &mut store, &auth)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(store.high, 7);
    e.abort(tx).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        e.begin(begin(6), &mut store, &auth),
        Err(StoreError::Conflict)
    );
    assert_eq!(store.high, 7);
    let mut b = begin(8);
    b.creation_token = [0; 16];
    assert_eq!(e.begin(b, &mut store, &auth), Err(StoreError::Invalid));
    assert_eq!(store.high, 7);
    assert_eq!(
        e.begin(begin(8), &mut store, &Auth(false)),
        Err(StoreError::Denied)
    );
    assert_eq!(store.high, 7);
}
#[test]
fn test_should_commit_only_complete_valid_authorized_crc_checked_object() {
    let mut store = Memory::default();
    let mut e = ObjectEngine::<16>::new();
    let auth = Auth(true);
    let tx = e
        .begin(begin(1), &mut store, &auth)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(e.write(tx, 1, &[9]), Err(StoreError::Sequence));
    assert_eq!(e.write(tx, 0, &[1, 2]), Ok(2));
    assert_eq!(
        e.commit(tx, 0, &mut store, &Schema, &auth),
        Err(StoreError::Incomplete)
    );
    assert!(store.meta.is_none());
    e.write(tx, 2, &[3]).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        e.commit(tx, 0, &mut store, &Schema, &auth),
        Err(StoreError::Integrity)
    );
    let crc = device_protocol::object_crc(5, 1, &[1, 2, 3]).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        e.commit(tx, crc, &mut store, &Schema, &auth)
            .map(|m| m.revision),
        Ok(1)
    );
    assert_eq!(store.bytes, [1, 2, 3]);
    assert_eq!(
        e.commit(tx, crc, &mut store, &Schema, &auth),
        Err(StoreError::InvalidState)
    );
}
#[test]
fn test_should_recheck_revision_at_commit_after_concurrent_ui_update() {
    let mut store = Memory::default();
    let mut e = ObjectEngine::<16>::new();
    let auth = Auth(true);
    let tx = e
        .begin(begin(1), &mut store, &auth)
        .unwrap_or_else(|e| panic!("{e:?}"));
    e.write(tx, 0, &[1, 2, 3])
        .unwrap_or_else(|e| panic!("{e:?}"));
    let crc = device_protocol::object_crc(5, 1, &[1, 2, 3]).unwrap_or_else(|e| panic!("{e:?}"));
    e.commit(tx, crc, &mut store, &Schema, &auth)
        .unwrap_or_else(|e| panic!("{e:?}"));
    let mut b = begin(1);
    b.create = false;
    b.expected_revision = 1;
    b.creation_token = [0; 16];
    let tx = e
        .begin(b, &mut store, &auth)
        .unwrap_or_else(|e| panic!("{e:?}"));
    e.write(tx, 0, &[1, 2, 3])
        .unwrap_or_else(|e| panic!("{e:?}"));
    store
        .meta
        .as_mut()
        .unwrap_or_else(|| panic!("missing"))
        .revision = 2;
    assert_eq!(
        e.commit(tx, crc, &mut store, &Schema, &auth),
        Err(StoreError::Conflict)
    );
    assert_eq!(store.meta.map(|m| m.revision), Some(2));
}
#[derive(Debug)]
struct Revokable(core::cell::Cell<bool>);
impl Authorization for Revokable {
    fn write_allowed(&self, _: u8) -> bool {
        self.0.get()
    }
}
struct RevokingSchema<'a>(&'a Revokable);
impl ObjectSchema for RevokingSchema<'_> {
    fn validate(&self, _: u8, _: u16, _: &[u8]) -> Result<(), StoreError> {
        self.0.0.set(false);
        Ok(())
    }
}
#[test]
fn test_should_recheck_authorization_after_schema_and_destroy_revoked_staging() {
    let auth = Revokable(core::cell::Cell::new(true));
    let mut store = Memory::default();
    let mut e = ObjectEngine::<16>::new();
    let tx = e
        .begin(begin(1), &mut store, &auth)
        .unwrap_or_else(|e| panic!("{e:?}"));
    e.write(tx, 0, &[1, 2, 3])
        .unwrap_or_else(|e| panic!("{e:?}"));
    let crc = device_protocol::object_crc(5, 1, &[1, 2, 3]).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        e.commit(tx, crc, &mut store, &RevokingSchema(&auth), &auth),
        Err(StoreError::Denied)
    );
    assert!(store.meta.is_none());
    auth.0.set(true);
    assert_eq!(
        e.commit(tx, crc, &mut store, &Schema, &auth),
        Err(StoreError::InvalidState)
    );
}
#[test]
fn test_should_end_staging_after_backend_commit_conflict() {
    let mut store = Memory::default();
    let mut e = ObjectEngine::<16>::new();
    let auth = Auth(true);
    let tx = e
        .begin(begin(1), &mut store, &auth)
        .unwrap_or_else(|e| panic!("{e:?}"));
    e.write(tx, 0, &[1, 2, 3])
        .unwrap_or_else(|e| panic!("{e:?}"));
    store.meta = Some(ObjectMeta {
        object_id: 1,
        kind: 1,
        revision: 7,
        total: 3,
        crc: 0,
        birth_token: [2; 16],
    });
    let crc = device_protocol::object_crc(5, 1, &[1, 2, 3]).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        e.commit(tx, crc, &mut store, &Schema, &auth),
        Err(StoreError::Conflict)
    );
    assert!(e.begin(begin(2), &mut store, &auth).is_ok());
}
#[test]
fn test_should_not_resurrect_staging_after_permission_returns() {
    let mut store = Memory::default();
    let mut e = ObjectEngine::<16>::new();
    let auth = Auth(true);
    let tx = e
        .begin(begin(1), &mut store, &auth)
        .unwrap_or_else(|e| panic!("{e:?}"));
    e.write(tx, 0, &[1, 2, 3])
        .unwrap_or_else(|e| panic!("{e:?}"));
    let crc = device_protocol::object_crc(5, 1, &[1, 2, 3]).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        e.commit(tx, crc, &mut store, &Schema, &Auth(false)),
        Err(StoreError::Denied)
    );
    assert_eq!(
        e.commit(tx, crc, &mut store, &Schema, &auth),
        Err(StoreError::InvalidState)
    );
}
struct OneShotAuthorization(core::cell::Cell<bool>);
impl Authorization for OneShotAuthorization {
    fn write_allowed(&self, _: u8) -> bool {
        self.0.replace(false)
    }
}
#[test]
fn test_should_recheck_scope_inside_durable_high_water_reservation() {
    let auth = OneShotAuthorization(core::cell::Cell::new(true));
    let mut store = Memory::default();
    let mut engine = ObjectEngine::<16>::new();
    assert_eq!(
        engine.begin(begin(99), &mut store, &auth),
        Err(StoreError::Denied)
    );
    assert_eq!(store.high, 0);
}
