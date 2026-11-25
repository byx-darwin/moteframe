//! Real snapshot fault, malformed image and recovery tests.
use device_history_service::{HistoryAccess, HistoryFailure, HistoryOperation, RecordStore};
use device_history_store::{AppendRecord, DomainConfig, HistoryClock, HistorySchema, HistoryStore};
use device_protocol::HistoryFilter;
use device_storage::{
    StoredRecord,
    persistence::{AtomicSnapshot, Error, Geometry, Layout, Medium, SnapshotStore},
};
use std::{cell::RefCell, rc::Rc};
#[derive(Debug)]
struct State {
    bytes: Vec<u8>,
    sync_fail: Option<usize>,
    syncs: usize,
    program_budget: Option<usize>,
}
#[derive(Clone, Debug)]
struct Media(Rc<RefCell<State>>);
impl Medium for Media {
    fn geometry(&self) -> Geometry {
        Geometry {
            capacity: 8192,
            erase_size: 4096,
            write_size: 1,
        }
    }
    fn read(&mut self, offset: u32, out: &mut [u8]) -> Result<(), Error> {
        let at = offset as usize;
        out.copy_from_slice(&self.0.borrow().bytes[at..at + out.len()]);
        Ok(())
    }
    fn erase(&mut self, offset: u32, length: u32) -> Result<(), Error> {
        self.0.borrow_mut().bytes[offset as usize..(offset + length) as usize].fill(255);
        Ok(())
    }
    fn program(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Error> {
        let mut state = self.0.borrow_mut();
        for (i, b) in bytes.iter().enumerate() {
            if state.program_budget == Some(0) {
                return Err(Error::Io);
            }
            if let Some(left) = state.program_budget.as_mut() {
                *left -= 1;
            }
            state.bytes[offset as usize + i] &= *b;
        }
        Ok(())
    }
    fn sync(&mut self) -> Result<(), Error> {
        let mut s = self.0.borrow_mut();
        s.syncs += 1;
        if s.sync_fail == Some(s.syncs) {
            Err(Error::Io)
        } else {
            Ok(())
        }
    }
}
#[derive(Debug)]
struct Schema;
impl HistorySchema for Schema {
    fn validate(&self, r: StoredRecord<'_>, parent: u64) -> Result<(), HistoryFailure> {
        if r.schema == 1 && r.payload == [1, 11] && parent == 1 {
            Ok(())
        } else {
            Err(HistoryFailure::Invalid)
        }
    }
}
#[derive(Debug)]
struct Clock;
impl HistoryClock for Clock {
    fn now_ms(&self) -> Result<u64, HistoryFailure> {
        Ok(100)
    }
}
struct Access;
impl HistoryAccess for Access {
    fn module_allowed(&self) -> bool {
        true
    }
    fn allowed(&self, _: [u8; 16], _: Option<u8>, _: HistoryOperation) -> bool {
        true
    }
}
type Store = HistoryStore<AtomicSnapshot<Media, 2048>, Schema, Clock, 2, 8, 16, 2048>;
fn snap(m: Media) -> AtomicSnapshot<Media, 2048> {
    AtomicSnapshot::open(
        m,
        Layout {
            start: 0,
            bank_bytes: 4096,
        },
    )
    .unwrap_or_else(|error| panic!("{error:?}"))
}
fn fresh() -> (Store, Media) {
    let m = Media(Rc::new(RefCell::new(State {
        bytes: vec![255; 8192],
        sync_fail: None,
        syncs: 0,
        program_budget: None,
    })));
    let s = Store::initialize(
        snap(m.clone()),
        Schema,
        Clock,
        [7; 16],
        &[DomainConfig {
            domain: 1,
            epoch: 3,
        }],
        10,
    )
    .unwrap_or_else(|error| panic!("{error:?}"));
    (s, m)
}
fn record() -> AppendRecord<'static> {
    AppendRecord {
        domain: 1,
        record_type: 2,
        schema: 1,
        utc_ms: 100,
        flags: 1,
        parent_session: 1,
        payload: &[1, 11],
    }
}
fn filter() -> HistoryFilter {
    HistoryFilter {
        domain: 1,
        filter_flags: 0,
        start_utc_ms: 0,
        end_utc_ms: 0,
        record_type: 0,
        parent_session: 0,
    }
}
fn arm(m: &Media, sync: Option<usize>, budget: Option<usize>) {
    let mut state = m.0.borrow_mut();
    state.syncs = 0;
    state.sync_fail = sync;
    state.program_budget = budget;
}

// Catches acknowledging/reading uncertain writes, and assuming an error always rolled back.
#[test]
fn test_should_recover_complete_old_or_new_append_at_all_three_commit_barriers() {
    for barrier in 1..=3 {
        let (mut s, m) = fresh();
        s.append(record())
            .unwrap_or_else(|error| panic!("{error:?}"));
        let old = s
            .open(1, filter(), 0, &Access)
            .unwrap_or_else(|error| panic!("{error:?}"));
        arm(&m, Some(barrier), None);
        assert_eq!(s.append(record()), Err(HistoryFailure::Storage));
        assert!(s.directory().is_err());
        assert!(s.read(old, 1, &mut [0; 16], &Access).is_err());
        arm(&m, None, None);
        s.recover().unwrap_or_else(|error| panic!("{error:?}"));
        assert!(s.record_admitted(old, 1, &Access).is_err());
        let count = s
            .count(filter(), &Access)
            .unwrap_or_else(|error| panic!("{error:?}"))
            .1;
        assert!(count == 1 || count == 2);
        assert_eq!(s.append(record()), Ok(u64::from(count) + 1));
    }
}
// Catches returning ACK success before final sync or retaining sent proof after recovery.
#[test]
fn test_should_recover_complete_old_or_new_ack_at_every_barrier() {
    for barrier in 1..=3 {
        let (mut s, m) = fresh();
        s.append(record())
            .unwrap_or_else(|error| panic!("{error:?}"));
        let l = s
            .open(1, filter(), 0, &Access)
            .unwrap_or_else(|error| panic!("{error:?}"));
        s.record_admitted(l, 1, &Access)
            .unwrap_or_else(|error| panic!("{error:?}"));
        arm(&m, Some(barrier), None);
        assert_eq!(
            s.ack_atomic(l, filter(), 3, 1, 1, &Access),
            Err(HistoryFailure::Storage)
        );
        assert!(s.directory().is_err());
        arm(&m, None, None);
        s.recover().unwrap_or_else(|error| panic!("{error:?}"));
        let new = s
            .open(2, filter(), 2, &Access)
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert!(new.committed_id <= 1);
        let retry = s.ack_atomic(new, filter(), 3, 1, 3, &Access);
        if new.committed_id == 1 {
            assert_eq!(retry, Ok(1));
        } else {
            assert_eq!(retry, Err(HistoryFailure::InvalidState));
        }
        drop(s);
        let mut reopened =
            Store::open(snap(m), Schema, Clock, 10).unwrap_or_else(|error| panic!("{error:?}"));
        let recovered = reopened
            .open(3, filter(), 4, &Access)
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(recovered.committed_id, new.committed_id);
    }
}
// Catches treating partially programmed logical payloads as committed records.
#[test]
fn test_should_preserve_old_snapshot_through_each_torn_program_position() {
    for budget in [0, 1, 8, 31, 32, 33, 70, 100, 150] {
        let (mut s, m) = fresh();
        s.append(record())
            .unwrap_or_else(|error| panic!("{error:?}"));
        arm(&m, None, Some(budget));
        let result = s.append(record());
        arm(&m, None, None);
        if result.is_err() {
            assert!(s.directory().is_err());
            s.recover().unwrap_or_else(|error| panic!("{error:?}"));
        }
        let count = s
            .count(filter(), &Access)
            .unwrap_or_else(|error| panic!("{error:?}"))
            .1;
        assert!(count == 1 || count == 2);
        let l = s
            .open(1, filter(), 0, &Access)
            .unwrap_or_else(|error| panic!("{error:?}"));
        let mut body = [0; 16];
        assert_eq!(
            s.read(l, 1, &mut body, &Access)
                .unwrap_or_else(|error| panic!("{error:?}"))
                .length,
            2
        );
        assert_eq!(&body[..2], &[1, 11]);
    }
}
fn replace_image(m: Media, mutate: impl FnOnce(&mut Vec<u8>)) {
    let mut snapshot = snap(m);
    let mut image = [0; 2048];
    let meta = snapshot
        .load(&mut image)
        .unwrap_or_else(|error| panic!("{error:?}"))
        .unwrap_or_else(|| panic!("expected initialized image"));
    let mut bytes = image[..meta.length].to_vec();
    mutate(&mut bytes);
    snapshot
        .commit(meta.revision, &bytes)
        .unwrap_or_else(|error| panic!("{error:?}"));
}
// Catches trusting outer snapshot CRC in place of strict image, WREC and actual schema validation.
#[test]
fn test_should_reject_committed_images_with_invalid_schema_crc_or_canonical_structure() {
    for mutation in 0..5 {
        let (mut s, m) = fresh();
        s.append(record())
            .unwrap_or_else(|error| panic!("{error:?}"));
        drop(s);
        replace_image(m.clone(), |bytes| match mutation {
            0 => bytes[7] = b'2',
            1 => bytes[67 + 37] ^= 1,
            2 => bytes[67 + 27] = 2,
            3 => bytes.push(0),
            _ => bytes[30 + 5] = 2,
        });
        assert!(matches!(
            Store::open(snap(m), Schema, Clock, 10),
            Err(HistoryFailure::StoredIntegrity)
        ));
    }
}
// Catches numerical adjacency being substituted for the actual ordered eligible prefix.
#[test]
fn test_should_admit_actual_order_across_numeric_id_gaps() {
    let (mut s, m) = fresh();
    s.append(record())
        .unwrap_or_else(|error| panic!("{error:?}"));
    s.append(record())
        .unwrap_or_else(|error| panic!("{error:?}"));
    drop(s);
    replace_image(m.clone(), |bytes| {
        bytes[30 + 13..30 + 21].copy_from_slice(&7u64.to_le_bytes());
        bytes[30 + 21..30 + 29].copy_from_slice(&7u64.to_le_bytes());
        let at = 67 + 43;
        bytes[at + 5..at + 13].copy_from_slice(&7u64.to_le_bytes());
        let crc = StoredRecord {
            domain: 1,
            epoch: 3,
            record_id: 7,
            record_type: 2,
            schema: 1,
            utc_ms: 100,
            flags: 1,
            payload: &[1, 11],
        }
        .crc()
        .unwrap_or_else(|error| panic!("{error:?}"));
        bytes[at + 37..at + 41].copy_from_slice(&crc.to_le_bytes());
    });
    let mut s = Store::open(snap(m), Schema, Clock, 10).unwrap_or_else(|error| panic!("{error:?}"));
    let l = s
        .open(1, filter(), 0, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    let p = s
        .plan(l, 0, 16, 1, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(&p.ids[..2], &[1, 7]);
    assert_eq!(s.record_admitted(l, 7, &Access), Ok(()));
    assert_eq!(
        s.ack_atomic(l, filter(), 3, 7, 1, &Access),
        Err(HistoryFailure::InvalidState)
    );
    s.record_admitted(l, 1, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    s.record_admitted(l, 7, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(s.ack_atomic(l, filter(), 3, 7, 2, &Access), Ok(7));
    assert_eq!(s.append(record()), Ok(8));
}

#[derive(Debug)]
struct BadSuccess {
    inner: AtomicSnapshot<Media, 2048>,
    bad: Rc<std::cell::Cell<bool>>,
}
impl SnapshotStore for BadSuccess {
    fn load(&self, out: &mut [u8]) -> Result<Option<device_storage::persistence::Snapshot>, Error> {
        self.inner.load(out)
    }
    fn commit(
        &mut self,
        expected: u64,
        bytes: &[u8],
    ) -> Result<device_storage::persistence::Snapshot, Error> {
        let mut meta = self.inner.commit(expected, bytes)?;
        if self.bad.get() {
            meta.length += 1;
        }
        Ok(meta)
    }
    fn recover(&mut self) -> Result<(), Error> {
        self.inner.recover()
    }
}
// Catches trusting successful injected snapshot metadata without validating its revision and length.
#[test]
fn test_should_poison_after_malformed_success_and_recover_actual_committed_image() {
    let (s, m) = fresh();
    drop(s);
    let bad = Rc::new(std::cell::Cell::new(false));
    let mut s = Wrapped::open(
        BadSuccess {
            inner: snap(m),
            bad: bad.clone(),
        },
        Schema,
        Clock,
        10,
    )
    .unwrap_or_else(|error| panic!("{error:?}"));
    bad.set(true);
    assert_eq!(s.append(record()), Err(HistoryFailure::StoredIntegrity));
    assert!(s.directory().is_err());
    bad.set(false);
    s.recover().unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(s.count(filter(), &Access), Ok((3, 1)));
    assert_eq!(s.append(record()), Ok(2));
}
// Catches wrapping persistent domain identities or changing data when the image cannot fit.
#[test]
fn test_should_reject_exhausted_identity_and_short_image_capacity_without_mutation() {
    let (s, m) = fresh();
    drop(s);
    replace_image(m.clone(), |bytes| {
        bytes[30 + 21..30 + 29].copy_from_slice(&u64::MAX.to_le_bytes());
    });
    let mut s =
        Store::open(snap(m.clone()), Schema, Clock, 10).unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(s.append(record()), Err(HistoryFailure::Capacity));
    assert_eq!(s.count(filter(), &Access), Ok((3, 0)));
    let (s, m) = fresh();
    drop(s);
    let mut s = Short::open(snap(m), Schema, Clock, 10).unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(s.append(record()), Err(HistoryFailure::Capacity));
    assert_eq!(s.count(filter(), &Access), Ok((3, 0)));
}

type Wrapped = HistoryStore<BadSuccess, Schema, Clock, 2, 8, 16, 2048>;
type Short = HistoryStore<AtomicSnapshot<Media, 2048>, Schema, Clock, 2, 8, 16, 80>;

// Catches an idempotent durable ACK retry performing another snapshot write.
#[test]
fn test_should_return_durable_ack_retry_without_a_storage_mutation() {
    let (mut s, m) = fresh();
    s.append(record())
        .unwrap_or_else(|error| panic!("{error:?}"));
    let l = s
        .open(1, filter(), 0, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    s.record_admitted(l, 1, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(s.ack_atomic(l, filter(), 3, 1, 1, &Access), Ok(1));
    let snapshot = s.into_snapshot();
    let mut bytes = [0; 2048];
    let before = snapshot
        .load(&mut bytes)
        .unwrap_or_else(|error| panic!("{error:?}"));
    let mut s =
        Store::open(snapshot, Schema, Clock, 10).unwrap_or_else(|error| panic!("{error:?}"));
    let l = s
        .open(2, filter(), 2, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    arm(&m, Some(1), None);
    assert_eq!(s.ack_atomic(l, filter(), 3, 1, 3, &Access), Ok(1));
    let snapshot = s.into_snapshot();
    assert_eq!(
        snapshot
            .load(&mut bytes)
            .unwrap_or_else(|error| panic!("{error:?}")),
        before
    );
}

// Catches deriving the resume prefix from numeric adjacency instead of durable ACK and actual IDs.
#[test]
fn test_should_resume_above_durable_ack_across_actual_numeric_id_gaps() {
    let (mut s, m) = fresh();
    s.append(record())
        .unwrap_or_else(|error| panic!("{error:?}"));
    s.append(record())
        .unwrap_or_else(|error| panic!("{error:?}"));
    let old = s
        .open(1, filter(), 0, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    s.record_admitted(old, 1, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(s.ack_atomic(old, filter(), 3, 1, 1, &Access), Ok(1));
    drop(s);
    replace_image(m.clone(), |bytes| {
        bytes[30 + 13..30 + 21].copy_from_slice(&7u64.to_le_bytes());
        bytes[30 + 21..30 + 29].copy_from_slice(&7u64.to_le_bytes());
        let at = 67 + 43;
        bytes[at + 5..at + 13].copy_from_slice(&7u64.to_le_bytes());
        let crc = StoredRecord {
            domain: 1,
            epoch: 3,
            record_id: 7,
            record_type: 2,
            schema: 1,
            utc_ms: 100,
            flags: 1,
            payload: &[1, 11],
        }
        .crc()
        .unwrap_or_else(|error| panic!("{error:?}"));
        bytes[at + 37..at + 41].copy_from_slice(&crc.to_le_bytes());
    });
    let mut s = Store::open(snap(m), Schema, Clock, 10).unwrap_or_else(|error| panic!("{error:?}"));
    let fresh = s
        .open(2, filter(), 2, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(fresh.committed_id, 1);
    let plan = s
        .plan(fresh, 1, 16, 3, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!((plan.count, plan.ids[0]), (1, 7));
    assert_eq!(s.record_admitted(fresh, 7, &Access), Ok(()));
    assert_eq!(s.ack_atomic(fresh, filter(), 3, 7, 4, &Access), Ok(7));
}

// Catches losing explicit initialization semantics at any programmed-byte cutoff,
// or reporting success before any of the three durability barriers completes.
#[test]
fn test_should_reopen_complete_or_uninitialized_history_after_interrupted_initialization() {
    for (barrier, budget) in (0..=101)
        .map(|budget| (None, Some(budget)))
        .chain((1..=3).map(|barrier| (Some(barrier), None)))
    {
        let m = Media(Rc::new(RefCell::new(State {
            bytes: vec![255; 8192],
            sync_fail: None,
            syncs: 0,
            program_budget: None,
        })));
        let snapshot = snap(m.clone());
        arm(&m, barrier, budget);
        let initialized = Store::initialize(
            snapshot,
            Schema,
            Clock,
            [7; 16],
            &[DomainConfig {
                domain: 1,
                epoch: 3,
            }],
            10,
        );
        if barrier.is_some() {
            assert!(matches!(initialized, Err(HistoryFailure::Storage)));
        }
        let success = initialized.is_ok();
        drop(initialized);
        arm(&m, None, None);
        match Store::open(snap(m.clone()), Schema, Clock, 10) {
            Ok(mut recovered) => {
                assert_eq!(recovered.count(filter(), &Access), Ok((3, 0)));
                assert_eq!(recovered.append(record()), Ok(1));
                assert!(matches!(
                    Store::initialize(
                        snap(m),
                        Schema,
                        Clock,
                        [8; 16],
                        &[DomainConfig {
                            domain: 1,
                            epoch: 4
                        }],
                        10,
                    ),
                    Err(HistoryFailure::Conflict)
                ));
            }
            Err(error) => {
                assert_eq!(error, HistoryFailure::NotFound);
                assert!(!success, "successful initialization must survive restart");
                let mut initialized = Store::initialize(
                    snap(m),
                    Schema,
                    Clock,
                    [7; 16],
                    &[DomainConfig {
                        domain: 1,
                        epoch: 3,
                    }],
                    10,
                )
                .unwrap_or_else(|error| panic!("{error:?}"));
                assert_eq!(initialized.append(record()), Ok(1));
            }
        }
    }
}

// Catches a torn ACK publishing a partial watermark, losing records or leaving
// admission proof live after uncertainty. A complete one-record image is110
// bytes; with32 header bytes and two marker bytes, all144 cutoffs are covered.
#[test]
fn test_should_recover_ack_at_every_programmed_byte_cutoff_without_record_loss() {
    for budget in 0..=144 {
        let (mut s, m) = fresh();
        assert_eq!(s.append(record()), Ok(1));
        let old = s
            .open(1, filter(), 0, &Access)
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(s.record_admitted(old, 1, &Access), Ok(()));
        arm(&m, None, Some(budget));
        let result = s.ack_atomic(old, filter(), 3, 1, 1, &Access);
        if result.is_err() {
            assert!(s.directory().is_err());
            assert!(s.record_admitted(old, 1, &Access).is_err());
        }
        arm(&m, None, None);
        s.recover().unwrap_or_else(|error| panic!("{error:?}"));
        assert!(s.record_admitted(old, 1, &Access).is_err());
        assert_eq!(s.count(filter(), &Access), Ok((3, 1)));
        let lease = s
            .open(2, filter(), 2, &Access)
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert!(lease.committed_id == 0 || lease.committed_id == 1);
        if result.is_ok() {
            assert_eq!(lease.committed_id, 1);
        }
        let mut body = [0; 16];
        let read = s
            .read(lease, 1, &mut body, &Access)
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(read.length, 2);
        assert_eq!(&body[..2], &[1, 11]);
        assert_eq!(s.record_admitted(lease, 1, &Access), Ok(()));
        assert_eq!(s.ack_atomic(lease, filter(), 3, 1, 3, &Access), Ok(1));
        assert_eq!(s.append(record()), Ok(2));
        let mut reopened = Store::open(s.into_snapshot(), Schema, Clock, 10)
            .unwrap_or_else(|error| panic!("{error:?}"));
        let lease = reopened
            .open(3, filter(), 4, &Access)
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(lease.committed_id, 1);
        assert_eq!(reopened.count(filter(), &Access), Ok((3, 2)));
    }
}

// Catches trusting ACK<=high as evidence of an actual phone claim when this
// format retains all records. Nonzero ACK must name an existing domain record.
#[test]
fn test_should_reject_durable_ack_without_retained_endpoint_even_with_valid_outer_crc() {
    for (count, ack) in [(0, 3u64), (1, 7), (2, 3)] {
        let (mut s, m) = fresh();
        for _ in 0..count {
            s.append(record())
                .unwrap_or_else(|error| panic!("{error:?}"));
        }
        drop(s);
        replace_image(m.clone(), |bytes| {
            // Domain row starts at30: high at51, durable ACK at59.
            bytes[51..59].copy_from_slice(&7u64.to_le_bytes());
            bytes[59..67].copy_from_slice(&ack.to_le_bytes());
            if count == 2 {
                bytes[43..51].copy_from_slice(&7u64.to_le_bytes());
                let at = 67 + 43;
                bytes[at + 5..at + 13].copy_from_slice(&7u64.to_le_bytes());
                let crc = StoredRecord {
                    domain: 1,
                    epoch: 3,
                    record_id: 7,
                    record_type: 2,
                    schema: 1,
                    utc_ms: 100,
                    flags: 1,
                    payload: &[1, 11],
                }
                .crc()
                .unwrap_or_else(|error| panic!("{error:?}"));
                bytes[at + 37..at + 41].copy_from_slice(&crc.to_le_bytes());
            }
        });
        assert!(
            matches!(
                Store::open(snap(m), Schema, Clock, 10),
                Err(HistoryFailure::StoredIntegrity)
            ),
            "accepted ACK {ack} without retained endpoint, record count {count}"
        );
    }
}

// Catches poisoning readable history and losing its lease on a documented
// pre-mutation error from the actual snapshot payload capacity check.
#[test]
fn test_should_preserve_history_and_lease_when_snapshot_capacity_rejects_before_mutation() {
    type Small = HistoryStore<AtomicSnapshot<Media, 120>, Schema, Clock, 2, 8, 16, 2048>;
    let m = Media(Rc::new(RefCell::new(State {
        bytes: vec![255; 8192],
        sync_fail: None,
        syncs: 0,
        program_budget: None,
    })));
    let snapshot = AtomicSnapshot::<_, 120>::open(
        m,
        Layout {
            start: 0,
            bank_bytes: 4096,
        },
    )
    .unwrap_or_else(|error| panic!("{error:?}"));
    let mut s = Small::initialize(
        snapshot,
        Schema,
        Clock,
        [7; 16],
        &[DomainConfig {
            domain: 1,
            epoch: 3,
        }],
        10,
    )
    .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(s.append(record()), Ok(1));
    let lease = s
        .open(1, filter(), 0, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(s.record_admitted(lease, 1, &Access), Ok(()));
    assert_eq!(s.append(record()), Err(HistoryFailure::Capacity));
    assert_eq!(s.count(filter(), &Access), Ok((3, 1)));
    assert!(s.read(lease, 1, &mut [0; 16], &Access).is_ok());
    assert_eq!(s.ack_atomic(lease, filter(), 3, 1, 1, &Access), Ok(1));
}

#[derive(Debug)]
struct RecoveryView {
    inner: AtomicSnapshot<Media, 2048>,
    previous: Vec<u8>,
    previous_meta: device_storage::persistence::Snapshot,
    mode: Rc<std::cell::Cell<u8>>,
}
impl SnapshotStore for RecoveryView {
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
        match self.mode.get() {
            3 => Err(Error::Capacity),
            4 => Err(Error::Conflict),
            5 => Err(Error::RevisionExhausted),
            _ => self.inner.commit(expected, bytes),
        }
    }
    fn recover(&mut self) -> Result<(), Error> {
        self.inner.recover()
    }
}
type Viewed = HistoryStore<RecoveryView, Schema, Clock, 2, 8, 16, 2048>;
fn viewed() -> (Viewed, Rc<std::cell::Cell<u8>>) {
    let (s, _) = fresh();
    let snapshot = s.into_snapshot();
    let mut previous = vec![0; 2048];
    let meta = snapshot
        .load(&mut previous)
        .unwrap_or_else(|error| panic!("{error:?}"))
        .unwrap_or_else(|| panic!("initialized fixture"));
    previous.truncate(meta.length);
    let mode = Rc::new(std::cell::Cell::new(0));
    let s = Viewed::open(
        RecoveryView {
            inner: snapshot,
            previous,
            previous_meta: meta,
            mode: mode.clone(),
        },
        Schema,
        Clock,
        10,
    )
    .unwrap_or_else(|error| panic!("{error:?}"));
    (s, mode)
}

// Defensive boundary: detect injected backend blank/rollback below a confirmed
// revision rather than exposing an older ACK as current. Not a hardware claim.
#[test]
fn test_should_reject_recovered_history_below_last_confirmed_revision() {
    let (mut s, mode) = viewed();
    assert_eq!(s.append(record()), Ok(1));
    let lease = s
        .open(1, filter(), 0, &Access)
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(s.record_admitted(lease, 1, &Access), Ok(()));
    assert_eq!(s.ack_atomic(lease, filter(), 3, 1, 1, &Access), Ok(1));
    for replay in [2, 1] {
        mode.set(replay);
        assert_eq!(s.recover(), Err(HistoryFailure::StoredIntegrity));
        assert!(s.directory().is_err());
        assert!(s.record_admitted(lease, 1, &Access).is_err());
        mode.set(0);
        s.recover().unwrap_or_else(|error| panic!("{error:?}"));
        let current = s
            .open(2 + u32::from(replay), filter(), 2, &Access)
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(current.committed_id, 1);
        s.close(current).unwrap_or_else(|error| panic!("{error:?}"));
    }
}

// Catches losing existing data/proof on any explicitly pre-mutation error;
// uncertain Io/Corrupt/NeedsRecovery remain covered by poisoning regressions.
#[test]
fn test_should_preserve_lease_and_proof_after_all_known_snapshot_prevalidation_errors() {
    for (mode_value, expected) in [
        (3, HistoryFailure::Capacity),
        (4, HistoryFailure::Conflict),
        (5, HistoryFailure::Capacity),
    ] {
        let (mut s, mode) = viewed();
        assert_eq!(s.append(record()), Ok(1));
        let lease = s
            .open(1, filter(), 0, &Access)
            .unwrap_or_else(|error| panic!("{error:?}"));
        assert_eq!(s.record_admitted(lease, 1, &Access), Ok(()));
        mode.set(mode_value);
        assert_eq!(
            s.ack_atomic(lease, filter(), 3, 1, 1, &Access),
            Err(expected)
        );
        assert_eq!(s.count(filter(), &Access), Ok((3, 1)));
        assert!(s.read(lease, 1, &mut [0; 16], &Access).is_ok());
        mode.set(0);
        assert_eq!(s.ack_atomic(lease, filter(), 3, 1, 2, &Access), Ok(1));
    }
}
