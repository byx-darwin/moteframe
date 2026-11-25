//! Contract, decoder and whole-byte power interruption tests.
#![allow(
    clippy::unwrap_used,
    reason = "Tests stop immediately when fixture construction or an asserted success fails."
)]
use device_object_service::{
    Birth, DirectoryEntry, ObjectAccess, ObjectOperation, ObjectRepository,
};
use device_object_store::DurableObjects;
use device_protocol::object_crc;
use device_storage::persistence::{AtomicSnapshot, Error, Geometry, Layout, Medium};
use device_storage::{Authorization, Begin, Commit, ObjectSchema, ObjectStore, StoreError};

#[derive(Clone, Debug)]
struct Media {
    bytes: Vec<u8>,
    operation: usize,
    fail: Option<(usize, usize)>,
}
impl Media {
    fn blank() -> Self {
        Self {
            bytes: vec![255; 8192],
            operation: 0,
            fail: None,
        }
    }
    fn cut(&mut self, count: usize) -> usize {
        let n = self.operation;
        self.operation += 1;
        if let Some((at, prefix)) = self.fail
            && n == at
        {
            return prefix.min(count);
        }
        count
    }
    fn result(&self) -> Result<(), Error> {
        if self.fail.is_some_and(|(at, _)| at + 1 == self.operation) {
            Err(Error::Io)
        } else {
            Ok(())
        }
    }
}
impl Medium for Media {
    fn geometry(&self) -> Geometry {
        Geometry {
            capacity: 8192,
            erase_size: 4096,
            write_size: 1,
        }
    }
    fn read(&mut self, at: u32, out: &mut [u8]) -> Result<(), Error> {
        out.copy_from_slice(&self.bytes[at as usize..at as usize + out.len()]);
        Ok(())
    }
    fn erase(&mut self, at: u32, n: u32) -> Result<(), Error> {
        let count = self.cut(n as usize);
        self.bytes[at as usize..at as usize + count].fill(255);
        self.result()
    }
    fn program(&mut self, at: u32, body: &[u8]) -> Result<(), Error> {
        let count = self.cut(body.len());
        for (d, b) in self.bytes[at as usize..].iter_mut().zip(&body[..count]) {
            *d &= b;
        }
        self.result()
    }
    fn sync(&mut self) -> Result<(), Error> {
        self.cut(0);
        self.result()
    }
}
#[derive(Debug, Clone, Copy)]
struct Schema;
impl ObjectSchema for Schema {
    fn validate(&self, m: u8, k: u16, b: &[u8]) -> Result<(), StoreError> {
        if [1, 2].contains(&m) && k == 7 && b.first() == Some(&42) {
            Ok(())
        } else {
            Err(StoreError::Schema)
        }
    }
}
struct Access;
impl Authorization for Access {
    fn write_allowed(&self, _: u8) -> bool {
        true
    }
}
impl ObjectAccess for Access {
    fn allowed(&self, _: u8, _: ObjectOperation, _: Option<u32>) -> bool {
        true
    }
    fn dangerous_allowed(&self, _: u8) -> bool {
        true
    }
}
type Repo = DurableObjects<AtomicSnapshot<Media, 2048>, Schema, 2, 8, 64, 2048>;
fn open(m: Media) -> Repo {
    Repo::open(
        AtomicSnapshot::open(
            m,
            Layout {
                start: 0,
                bank_bytes: 4096,
            },
        )
        .unwrap(),
        Schema,
    )
    .unwrap()
}
fn fresh() -> Repo {
    let mut r = open(Media::blank());
    r.initialize(&[1, 2]).unwrap();
    r
}
fn begin(m: u8, id: u32, token: u8) -> Begin {
    Begin {
        module: m,
        kind: 7,
        object_id: id,
        expected_revision: 0,
        total: 2,
        create: true,
        creation_token: [token; 16],
    }
}
fn publish(r: &mut Repo, b: Begin, body: &[u8]) -> Result<device_storage::ObjectMeta, StoreError> {
    r.commit(
        &Commit {
            begin: b,
            crc: object_crc(b.module, b.kind, body).unwrap(),
        },
        body,
        &Access,
    )
}

// Break caught: losing durable Begin reservations or reusing namespace tokens.
#[test]
fn test_should_keep_reservation_and_skipped_ids_across_reopen() {
    let mut r = fresh();
    r.reserve(&begin(1, 5, 9), &Access).unwrap();
    let mut r = open(r.into_store().into_medium());
    assert_eq!(r.id_state(1, 3, &Access).unwrap().state, 2);
    assert_eq!(r.id_state(1, 5, &Access).unwrap().birth, Birth::default());
    assert_eq!(
        r.reserve(&begin(2, 1, 9), &Access),
        Err(StoreError::Conflict)
    );
    let mut wrong = begin(1, 5, 9);
    wrong.total = 3;
    assert_eq!(
        publish(&mut r, wrong, &[42, 0, 1]),
        Err(StoreError::Conflict)
    );
    publish(&mut r, begin(1, 5, 9), &[42, 3]).unwrap();
    assert_eq!(r.id_state(1, 5, &Access).unwrap().birth.total, 2);
}
// Break caught: replacing birth evidence on edit, missing read/delete CAS, or recycling clear IDs.
#[test]
fn test_should_preserve_birth_through_edit_delete_clear() {
    let mut r = fresh();
    let b = begin(1, 2, 1);
    r.reserve(&b, &Access).unwrap();
    publish(&mut r, b, &[42, 1]).unwrap();
    let birth = r.id_state(1, 2, &Access).unwrap().birth;
    let edit = Begin {
        create: false,
        creation_token: [0; 16],
        expected_revision: 1,
        total: 3,
        ..b
    };
    assert_eq!(publish(&mut r, edit, &[42, 2, 3]).unwrap().revision, 2);
    assert_eq!(r.id_state(1, 2, &Access).unwrap().birth, birth);
    let mut out = [0; 5];
    assert_eq!(
        r.read_range(1, 2, 1, 0, &mut out, &Access),
        Err(StoreError::Conflict)
    );
    assert_eq!(
        r.read_range(1, 2, 2, 1, &mut out, &Access).unwrap().length,
        2
    );
    assert_eq!(&out[..2], &[2, 3]);
    assert_eq!(r.delete_atomic(1, 2, 1, &Access), Err(StoreError::Conflict));
    r.delete_atomic(1, 2, 2, &Access).unwrap();
    assert_eq!(r.id_state(1, 2, &Access).unwrap().birth, Birth::default());
    let b = begin(1, 3, 2);
    r.reserve(&b, &Access).unwrap();
    publish(&mut r, b, &[42, 4]).unwrap();
    let mut entries = [DirectoryEntry::default(); 2];
    let rev = r.enumerate(1, 0, &mut entries, &Access).unwrap().revision;
    r.delete_atomic(1, u32::MAX, rev, &Access).unwrap();
    let mut r = open(r.into_store().into_medium());
    assert_eq!(r.id_state(1, 3, &Access).unwrap().state, 2);
    assert_eq!(
        r.reserve(&begin(1, 3, 3), &Access),
        Err(StoreError::Conflict)
    );
    assert_eq!(
        r.reserve(&begin(2, 1, 1), &Access),
        Err(StoreError::Conflict)
    );
}
// Break caught: trusting engine-only validation or publishing unauthorized bytes.
#[test]
fn test_should_validate_crc_schema_and_full_length() {
    let mut r = fresh();
    let b = begin(1, 1, 1);
    r.reserve(&b, &Access).unwrap();
    assert_eq!(publish(&mut r, b, &[42]), Err(StoreError::Incomplete));
    assert_eq!(publish(&mut r, b, &[0, 1]), Err(StoreError::Schema));
    assert_eq!(
        r.commit(&Commit { begin: b, crc: 0 }, &[42, 1], &Access),
        Err(StoreError::Integrity)
    );
    assert_eq!(r.stat(1, 1), Err(StoreError::NotFound));
}
struct Denied {
    op: ObjectOperation,
    id: Option<u32>,
    dangerous: bool,
}
impl ObjectAccess for Denied {
    fn allowed(&self, _: u8, op: ObjectOperation, id: Option<u32>) -> bool {
        !(op == self.op && id == self.id)
    }
    fn dangerous_allowed(&self, _: u8) -> bool {
        self.dangerous
    }
}
struct NoWrite;
impl Authorization for NoWrite {
    fn write_allowed(&self, _: u8) -> bool {
        false
    }
}
// Break caught: leaking partial directory metadata or accepting missing Clear scope.
#[test]
fn test_should_authorize_complete_pages_and_clear_independently() {
    let mut r = fresh();
    for id in 1..=3 {
        let b = begin(1, id, u8::try_from(id).unwrap());
        r.reserve(&b, &Access).unwrap();
        publish(&mut r, b, &[42, 1]).unwrap();
    }
    let mut out = [DirectoryEntry {
        id: 99,
        kind: 99,
        total: 99,
    }; 1];
    let denied = Denied {
        op: ObjectOperation::Enumerate,
        id: Some(2),
        dangerous: true,
    };
    assert_eq!(
        r.enumerate(1, 0, &mut out, &denied),
        Err(StoreError::Denied)
    );
    assert_eq!(out[0].id, 99);
    let rev = r.enumerate(1, 0, &mut out, &Access).unwrap().revision;
    for denied in [
        Denied {
            op: ObjectOperation::Clear,
            id: None,
            dangerous: true,
        },
        Denied {
            op: ObjectOperation::Read,
            id: None,
            dangerous: false,
        },
    ] {
        assert_eq!(
            r.delete_atomic(1, u32::MAX, rev, &denied),
            Err(StoreError::Denied)
        );
    }
    assert!(r.stat(1, 1).is_ok());
    assert_eq!(
        r.reserve(&begin(2, 1, 8), &NoWrite),
        Err(StoreError::Denied)
    );
    assert_eq!(r.id_state(2, 1, &Access).unwrap().state, 0);
}
// Break caught: reclaiming aborted reservations or ledger tokens at the capacity boundary.
#[test]
fn test_should_exhaust_lifetime_capacity_without_recycling() {
    let mut r = fresh();
    for id in 1..=8 {
        r.reserve(&begin(1, id, u8::try_from(id).unwrap()), &Access)
            .unwrap();
    }
    assert_eq!(
        r.reserve(&begin(1, 9, 9), &Access),
        Err(StoreError::Capacity)
    );
    let mut out = [];
    let revision = r.enumerate(1, 0, &mut out, &Access).unwrap().revision;
    r.delete_atomic(1, u32::MAX, revision, &Access).unwrap();
    assert_eq!(
        r.reserve(&begin(2, 1, 9), &Access),
        Err(StoreError::Capacity)
    );
    assert_eq!(r.id_state(1, 8, &Access).unwrap().high_water, 8);
}
// Break caught: exposing uncommitted RAM after any uncertain logical operation.
#[test]
fn test_should_recover_old_or_complete_new_edit_after_every_torn_operation() {
    let mut base = fresh();
    let b = begin(1, 1, 1);
    base.reserve(&b, &Access).unwrap();
    publish(&mut base, b, &[42, 1]).unwrap();
    let base = base.into_store().into_medium();
    let edit = Begin {
        create: false,
        creation_token: [0; 16],
        expected_revision: 1,
        ..b
    };
    let mut baseline = open(base.clone());
    publish(&mut baseline, edit, &[42, 2]).unwrap();
    let limit = baseline.into_store().into_medium().operation - base.operation;
    for cut in 1..=limit {
        for prefix in [0, 1, 2048] {
            let mut media = base.clone();
            media.operation = 0;
            media.fail = Some((cut, prefix));
            let mut r = open(media);
            let result = publish(&mut r, edit, &[42, 2]);
            if result.is_err() {
                assert_eq!(r.stat(1, 1), Err(StoreError::Backend));
            }
            let mut media = r.into_store().into_medium();
            media.fail = None;
            let r = open(media);
            let current = r.stat(1, 1).unwrap();
            assert!([1, 2].contains(&current.revision));
            let mut body = [0; 2];
            r.read_range(1, 1, current.revision, 0, &mut body, &Access)
                .unwrap();
            assert_eq!(
                body,
                if current.revision == 1 {
                    [42, 1]
                } else {
                    [42, 2]
                }
            );
            assert_eq!(r.id_state(1, 1, &Access).unwrap().birth.revision, 1);
        }
    }
}

#[derive(Debug)]
struct Image {
    bytes: Vec<u8>,
    revision: u64,
}
impl device_storage::persistence::SnapshotStore for Image {
    fn load(&self, out: &mut [u8]) -> Result<Option<device_storage::persistence::Snapshot>, Error> {
        out[..self.bytes.len()].copy_from_slice(&self.bytes);
        Ok(Some(device_storage::persistence::Snapshot {
            revision: self.revision,
            length: self.bytes.len(),
        }))
    }
    fn commit(&mut self, _: u64, _: &[u8]) -> Result<device_storage::persistence::Snapshot, Error> {
        Err(Error::Io)
    }
    fn recover(&mut self) -> Result<(), Error> {
        Ok(())
    }
}
fn image() -> Image {
    use device_storage::persistence::SnapshotStore;
    let mut r = fresh();
    let b = begin(1, 1, 1);
    r.reserve(&b, &Access).unwrap();
    publish(&mut r, b, &[42, 1]).unwrap();
    let mut bytes = vec![0; 2048];
    let snapshot = r.into_store().load(&mut bytes).unwrap().unwrap();
    bytes.truncate(snapshot.length);
    Image {
        bytes,
        revision: snapshot.revision,
    }
}
fn reseal(image: &mut Image) {
    let split = image.bytes.len() - 4;
    let mut crc = device_protocol::Crc32c::new();
    crc.update(&image.bytes[..split]);
    image.bytes[split..].copy_from_slice(&crc.finish().to_le_bytes());
}
// Break caught: accepting an image whose high water has no durable identity evidence.
#[test]
fn test_should_reject_inflated_highwater_even_with_valid_image_checksum() {
    let mut image = image();
    image.bytes[37..41].copy_from_slice(&2u32.to_le_bytes());
    reseal(&mut image);
    assert!(DurableObjects::<_, _, 2, 8, 64, 2048>::open(image, Schema).is_err());
}
// Break caught: returning reserve success before the token and skipped IDs are durable.
#[test]
fn test_should_recover_whole_reservations_after_torn_publication() {
    let base = fresh().into_store().into_medium();
    let b = begin(1, 5, 9);
    let mut baseline = open(base.clone());
    baseline.reserve(&b, &Access).unwrap();
    let limit = baseline.into_store().into_medium().operation - base.operation;
    for cut in 1..=limit {
        let mut media = base.clone();
        media.operation = 0;
        media.fail = Some((cut, 0));
        let mut r = open(media);
        let result = r.reserve(&b, &Access);
        if result.is_err() {
            assert_eq!(r.id_state(1, 5, &Access), Err(StoreError::Backend));
        }
        let mut media = r.into_store().into_medium();
        media.fail = None;
        let mut r = open(media);
        let state = r.id_state(1, 5, &Access).unwrap();
        assert!([0, 5].contains(&state.high_water));
        if state.high_water == 5 {
            assert_eq!(state.state, 2);
            assert_eq!(
                r.reserve(&begin(2, 1, 9), &Access),
                Err(StoreError::Conflict)
            );
            assert_eq!(
                r.reserve(&begin(1, 4, 8), &Access),
                Err(StoreError::Conflict)
            );
            publish(&mut r, b, &[42, 7]).unwrap();
        } else {
            assert_eq!(state.state, 0);
            r.reserve(&begin(2, 1, 9), &Access).unwrap();
        }
    }
}
// Break caught: accepting invalid format, body CRC, schema or mismatched snapshot revision.
#[test]
fn test_should_reject_crc_schema_and_snapshot_identity_corruption() {
    for case in 0..5 {
        let mut image = image();
        match case {
            0 => image.bytes[0] ^= 1,
            1 => image.revision += 1,
            2 => {
                let i = image.bytes.len() - 5;
                image.bytes[i] ^= 1;
            }
            3 => {
                let i = image.bytes.len() - 6;
                image.bytes[i] = 0;
                reseal(&mut image);
            }
            _ => {
                image.bytes.push(0);
            }
        }
        assert!(
            DurableObjects::<_, _, 2, 8, 64, 2048>::open(image, Schema).is_err(),
            "case {case}"
        );
    }
}
#[derive(Debug)]
struct Outcomes {
    inner: AtomicSnapshot<Media, 2048>,
    mode: std::rc::Rc<std::cell::Cell<u8>>,
}
impl device_storage::persistence::SnapshotStore for Outcomes {
    fn load(&self, out: &mut [u8]) -> Result<Option<device_storage::persistence::Snapshot>, Error> {
        self.inner.load(out)
    }
    fn commit(&mut self, r: u64, b: &[u8]) -> Result<device_storage::persistence::Snapshot, Error> {
        match self.mode.get() {
            2 => return Err(Error::Capacity),
            3 => return Err(Error::Conflict),
            4 => return Err(Error::RevisionExhausted),
            _ => {}
        }
        let mut snapshot = self.inner.commit(r, b)?;
        if self.mode.get() == 1 {
            snapshot.length += 1;
        }
        Ok(snapshot)
    }
    fn recover(&mut self) -> Result<(), Error> {
        self.inner.recover()
    }
}
// Break caught: treating unexpected successful backend metadata as rollback or exposing stale RAM.
#[test]
fn test_should_poison_unexpected_outcomes_and_reload_complete_new_version() {
    let base = fresh().into_store();
    let mode = std::rc::Rc::new(std::cell::Cell::new(0));
    let backend = Outcomes {
        inner: base,
        mode: mode.clone(),
    };
    let mut r = DurableObjects::<_, _, 2, 8, 64, 2048>::open(backend, Schema).unwrap();
    for value in [2, 3, 4] {
        mode.set(value);
        assert!(r.reserve(&begin(1, 1, 1), &Access).is_err());
        assert_eq!(r.id_state(1, 1, &Access).unwrap().state, 0);
    }
    mode.set(1);
    assert_eq!(
        r.reserve(&begin(1, 1, 1), &Access),
        Err(StoreError::Backend)
    );
    assert_eq!(r.stat(1, 1), Err(StoreError::Backend));
    mode.set(0);
    r.recover().unwrap();
    assert_eq!(r.id_state(1, 1, &Access).unwrap().state, 2);
}
// Break caught: wrapping object, collection or snapshot revisions to zero.
#[test]
fn test_should_reject_revision_exhaustion_without_poisoning_known_state() {
    let mut data = image();
    data.revision = u64::MAX;
    data.bytes[20..28].copy_from_slice(&u64::MAX.to_le_bytes());
    reseal(&mut data);
    // With two u32 collection counters and eight ledger entries, u64::MAX
    // cannot describe any valid v1 image. Reject it as corrupt.
    assert!(DurableObjects::<_, _, 2, 8, 64, 2048>::open(data, Schema).is_err());
    let mut data = image();
    data.revision = u64::from(u32::MAX) + 1;
    data.bytes[20..28].copy_from_slice(&data.revision.to_le_bytes());
    data.bytes[41..45].copy_from_slice(&u32::MAX.to_le_bytes());
    data.bytes[117..121].copy_from_slice(&(u32::MAX - 1).to_le_bytes());
    reseal(&mut data);
    let mut r = DurableObjects::<_, _, 2, 8, 64, 2048>::open(data, Schema).unwrap();
    let b = Begin {
        create: false,
        creation_token: [0; 16],
        expected_revision: u32::MAX - 1,
        ..begin(1, 1, 1)
    };
    assert_eq!(
        r.commit(
            &Commit {
                begin: b,
                crc: object_crc(1, 7, &[42, 9]).unwrap()
            },
            &[42, 9],
            &Access
        ),
        Err(StoreError::Capacity)
    );
    assert_eq!(
        r.delete_atomic(1, 1, u32::MAX - 1, &Access),
        Err(StoreError::Capacity)
    );
    assert_eq!(r.stat(1, 1).unwrap().revision, u32::MAX - 1);
}
// Break caught: autoformatting blank storage or allowing a second initialization.
#[test]
fn test_should_require_explicit_initialization_on_confirmed_empty_storage() {
    let mut r = open(Media::blank());
    assert_eq!(r.stat(1, 1), Err(StoreError::InvalidState));
    assert!(r.initialize(&[2, 1]).is_err());
    r.initialize(&[1, 2]).unwrap();
    assert!(r.initialize(&[1, 2]).is_err());
    let r = open(r.into_store().into_medium());
    assert_eq!(r.id_state(1, 1, &Access).unwrap().state, 0);
}
#[derive(Debug)]
struct Reject;
impl ObjectSchema for Reject {
    fn validate(&self, _: u8, _: u16, _: &[u8]) -> Result<(), StoreError> {
        Err(StoreError::Schema)
    }
}
// Break caught: reopening under a permissive fallback rather than the actual injected schema.
#[test]
fn test_should_revalidate_actual_schema_on_reopen() {
    assert!(DurableObjects::<_, _, 2, 8, 64, 2048>::open(image(), Reject).is_err());
}
struct Revoking(std::cell::Cell<u8>);
impl Authorization for Revoking {
    fn write_allowed(&self, _: u8) -> bool {
        let old = self.0.get();
        self.0.set(old + 1);
        old == 0
    }
}
// Break caught: trusting the initial auth callback after schema/body validation and state assembly.
#[test]
fn test_should_recheck_live_authorization_before_durable_mutation() {
    let mut r = fresh();
    assert_eq!(
        r.reserve(&begin(1, 1, 1), &Revoking(std::cell::Cell::new(0))),
        Err(StoreError::Denied)
    );
    assert_eq!(r.id_state(1, 1, &Access).unwrap().state, 0);
    let b = begin(1, 1, 1);
    r.reserve(&b, &Access).unwrap();
    assert_eq!(
        r.commit(
            &Commit {
                begin: b,
                crc: object_crc(1, 7, &[42, 1]).unwrap()
            },
            &[42, 1],
            &Revoking(std::cell::Cell::new(0))
        ),
        Err(StoreError::Denied)
    );
    assert_eq!(r.stat(1, 1), Err(StoreError::NotFound));
}
// Break caught: truncating complete-body capacity or fabricating empty non-EOF pages.
#[test]
fn test_should_bound_ranges_and_zero_capacity_pages() {
    let mut r = fresh();
    let b = begin(1, 1, 1);
    r.reserve(&b, &Access).unwrap();
    publish(&mut r, b, &[42, 1]).unwrap();
    assert_eq!(
        r.enumerate(1, 0, &mut [], &Access),
        Err(StoreError::Capacity)
    );
    let mut out = [9; 1];
    assert_eq!(
        r.read_range(1, 1, 1, 0, &mut out, &Access).unwrap().length,
        1
    );
    assert_eq!(out, [42]);
    assert_eq!(
        r.read_range(1, 1, 1, 3, &mut out, &Access),
        Err(StoreError::Invalid)
    );
    let denied = Denied {
        op: ObjectOperation::Read,
        id: Some(1),
        dangerous: true,
    };
    assert_eq!(
        r.read_range(1, 1, 1, 0, &mut out, &denied),
        Err(StoreError::Denied)
    );
    assert_eq!(out, [42]);
}
// Break caught: undoing durable ID/token consumption on engine abort or disconnect.
#[test]
fn test_should_keep_aborted_and_disconnected_engine_ids_consumed() {
    let mut r = fresh();
    let mut engine = device_storage::ObjectEngine::<64>::new();
    let tx = engine.begin(begin(1, 5, 1), &mut r, &Access).unwrap();
    engine.write(tx, 0, &[42]).unwrap();
    engine.abort(tx).unwrap();
    let tx = engine.begin(begin(1, 6, 2), &mut r, &Access).unwrap();
    engine.write(tx, 0, &[42]).unwrap();
    engine.disconnect();
    let mut r = open(r.into_store().into_medium());
    for id in 1..=6 {
        assert_eq!(r.id_state(1, id, &Access).unwrap().state, 2);
    }
    assert_eq!(
        r.reserve(&begin(2, 1, 1), &Access),
        Err(StoreError::Conflict)
    );
}
// Break caught: partial publication of collection deletion or clear tombstones.
#[test]
fn test_should_recover_complete_delete_and_clear_after_all_sync_failures() {
    for clear in [false, true] {
        let mut r = fresh();
        for id in 1..=2 {
            let b = begin(1, id, u8::try_from(id).unwrap());
            r.reserve(&b, &Access).unwrap();
            publish(&mut r, b, &[42, 1]).unwrap();
        }
        let mut entries = [DirectoryEntry::default(); 2];
        let revision = r.enumerate(1, 0, &mut entries, &Access).unwrap().revision;
        let base = r.into_store().into_medium();
        let mut baseline = open(base.clone());
        baseline
            .delete_atomic(
                1,
                if clear { u32::MAX } else { 1 },
                if clear { revision } else { 1 },
                &Access,
            )
            .unwrap();
        let limit = baseline.into_store().into_medium().operation - base.operation;
        for cut in 1..=limit {
            let mut media = base.clone();
            media.operation = 0;
            media.fail = Some((cut, 1));
            let mut r = open(media);
            let _ = r.delete_atomic(
                1,
                if clear { u32::MAX } else { 1 },
                if clear { revision } else { 1 },
                &Access,
            );
            let mut media = r.into_store().into_medium();
            media.fail = None;
            let mut r = open(media);
            let first = r.id_state(1, 1, &Access).unwrap().state;
            let second = r.id_state(1, 2, &Access).unwrap().state;
            assert!([1, 2].contains(&first));
            assert_eq!(second, if clear { first } else { 1 });
            assert_eq!(
                r.reserve(&begin(2, 1, 1), &Access),
                Err(StoreError::Conflict)
            );
        }
    }
}

// Break caught: accepting a globally impossible number of durable v1 operations.
#[test]
fn test_should_reject_snapshot_counter_inconsistent_with_reservations_and_collections() {
    let mut data = image();
    data.revision = 2;
    data.bytes[20..28].copy_from_slice(&2u64.to_le_bytes());
    reseal(&mut data);
    assert!(DurableObjects::<_, _, 2, 8, 64, 2048>::open(data, Schema).is_err());
}

// Break caught: counting only the largest live revision, or omitting creation/removal evidence.
#[test]
fn test_should_reject_impossible_collection_aggregate_with_exact_global_counter() {
    use device_storage::persistence::SnapshotStore;
    let mut accepted = Vec::new();
    for removals in 0..=2 {
        let mut r = fresh();
        for id in 1..=2 {
            let b = begin(1, id, u8::try_from(id).unwrap());
            r.reserve(&b, &Access).unwrap();
            publish(&mut r, b, &[42, 1]).unwrap();
        }
        if removals == 1 {
            r.delete_atomic(1, 1, 1, &Access).unwrap();
        } else if removals == 2 {
            r.delete_atomic(1, u32::MAX, 3, &Access).unwrap();
        }
        let mut bytes = vec![0; 2048];
        let snapshot = r.into_store().load(&mut bytes).unwrap().unwrap();
        bytes.truncate(snapshot.length);
        // Two creates need collection3. One or two tombstones need collection4:
        // both commits plus at least one actual delete/Clear operation.
        let invalid_collection: u32 = if removals == 0 { 2 } else { 3 };
        let mut data = Image {
            bytes,
            revision: 1 + 2 + u64::from(invalid_collection - 1),
        };
        data.bytes[20..28].copy_from_slice(&data.revision.to_le_bytes());
        data.bytes[41..45].copy_from_slice(&invalid_collection.to_le_bytes());
        reseal(&mut data);
        if DurableObjects::<_, _, 2, 8, 64, 2048>::open(data, Schema).is_ok() {
            accepted.push(removals);
        }
    }
    assert!(
        accepted.is_empty(),
        "accepted impossible removal cases: {accepted:?}"
    );
}

#[derive(Debug)]
struct RecoveryView {
    inner: AtomicSnapshot<Media, 2048>,
    previous: Vec<u8>,
    previous_meta: device_storage::persistence::Snapshot,
    mode: std::rc::Rc<std::cell::Cell<u8>>,
}
impl device_storage::persistence::SnapshotStore for RecoveryView {
    fn load(&self, out: &mut [u8]) -> Result<Option<device_storage::persistence::Snapshot>, Error> {
        match self.mode.get() {
            1 => Ok(None),
            2 => {
                out[..self.previous.len()].copy_from_slice(&self.previous);
                Ok(Some(self.previous_meta))
            }
            _ => self.inner.load(out),
        }
    }
    fn commit(
        &mut self,
        expected: u64,
        bytes: &[u8],
    ) -> Result<device_storage::persistence::Snapshot, Error> {
        self.inner.commit(expected, bytes)
    }
    fn recover(&mut self) -> Result<(), Error> {
        self.inner.recover()
    }
}

// Defensive boundary: an injected backend has violated successful durability.
// Known state must stay blocked rather than forgetting identities and reusing them.
#[test]
fn test_should_reject_blank_or_older_recovery_below_last_confirmed_object_revision() {
    use device_storage::persistence::SnapshotStore;
    let snapshot = fresh().into_store();
    let mut previous = vec![0; 2048];
    let previous_meta = snapshot.load(&mut previous).unwrap().unwrap();
    previous.truncate(previous_meta.length);
    let mode = std::rc::Rc::new(std::cell::Cell::new(0));
    let mut r = DurableObjects::<_, _, 2, 8, 64, 2048>::open(
        RecoveryView {
            inner: snapshot,
            previous,
            previous_meta,
            mode: mode.clone(),
        },
        Schema,
    )
    .unwrap();
    let reservation = begin(1, 5, 9);
    r.reserve(&reservation, &Access).unwrap();
    for replay in [1, 2] {
        mode.set(replay);
        assert_eq!(r.recover(), Err(device_object_store::Error::Integrity));
        assert_eq!(r.id_state(1, 5, &Access), Err(StoreError::Backend));
        assert!(r.initialize(&[1, 2]).is_err());
        mode.set(0);
        r.recover().unwrap();
        assert_eq!(r.revision().unwrap(), 2);
        assert_eq!(r.id_state(1, 5, &Access).unwrap().high_water, 5);
        assert_eq!(r.reserve(&reservation, &Access), Err(StoreError::Conflict));
    }
}
