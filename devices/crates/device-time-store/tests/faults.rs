//! Whole-byte medium interruption and strict durable recovery contracts.
mod common;
use common::{Access, Nor, Source, Store, must, store};
use core::cell::Cell;
use device_storage::persistence::{Error as StorageError, Snapshot, SnapshotStore};
use device_time_store::{Error, TimeStore};
use std::rc::Rc;

#[derive(Clone, Copy, Debug)]
enum Mode {
    Good,
    Reject(StorageError),
    BadReceipt,
    FailAfterCommit,
    BadRevision,
    RewriteLoad,
}
#[derive(Debug)]
struct Controlled {
    inner: Store,
    mode: Rc<Cell<Mode>>,
}
impl SnapshotStore for Controlled {
    fn load(&self, out: &mut [u8]) -> Result<Option<Snapshot>, StorageError> {
        let snapshot = self.inner.load(out)?;
        if matches!(self.mode.get(), Mode::RewriteLoad) && snapshot.is_some() {
            out[40] ^= 1;
        }
        Ok(snapshot)
    }
    fn commit(&mut self, expected: u64, bytes: &[u8]) -> Result<Snapshot, StorageError> {
        if let Mode::Reject(error) = self.mode.get() {
            return Err(error);
        }
        let mut snapshot = self.inner.commit(expected, bytes)?;
        match self.mode.get() {
            Mode::BadReceipt => snapshot.length = 63,
            Mode::BadRevision => snapshot.revision = 0,
            Mode::FailAfterCommit => return Err(StorageError::Io),
            Mode::Good | Mode::Reject(_) | Mode::RewriteLoad => {}
        }
        Ok(snapshot)
    }
    fn recover(&mut self) -> Result<(), StorageError> {
        self.inner.recover()
    }
}

#[test]
fn test_should_preserve_confirmed_anchor_on_known_precheck_failure_and_remember_actual_floor() {
    for error in [
        StorageError::Capacity,
        StorageError::Conflict,
        StorageError::RevisionExhausted,
    ] {
        let source = Source::new(100);
        let mode = Rc::new(Cell::new(Mode::Good));
        let owner = must(TimeStore::open(
            Controlled {
                inner: store(Nor::blank()),
                mode: mode.clone(),
            },
            source.clone(),
            0,
        ));
        let access = Access::all();
        must(owner.synchronize(10, 1, &access));
        mode.set(Mode::Reject(error));
        source.at(200);
        assert_eq!(
            owner.synchronize(99, 9, &access),
            Err(Error::Storage(error))
        );
        assert_eq!(must(owner.revision()), 1);
        let mapping = must(owner.read(&access));
        assert_eq!((mapping.utc_ms, mapping.valid), (10, true));
        source.at(199);
        assert_eq!(owner.read(&access), Err(Error::NonMonotonic));
    }
}

#[test]
fn test_should_poison_uncertain_or_malformed_receipts_and_recover_actual_new_evidence() {
    for bad in [Mode::BadReceipt, Mode::BadRevision, Mode::FailAfterCommit] {
        let source = Source::new(100);
        let mode = Rc::new(Cell::new(Mode::Good));
        let owner = must(TimeStore::open(
            Controlled {
                inner: store(Nor::blank()),
                mode: mode.clone(),
            },
            source.clone(),
            0,
        ));
        let access = Access::all();
        must(owner.synchronize(10, 1, &access));
        mode.set(bad);
        source.at(200);
        assert!(owner.synchronize(20, 2, &access).is_err());
        assert_eq!(
            owner.read(&access),
            Err(Error::Storage(StorageError::NeedsRecovery))
        );
        assert_eq!(
            owner.revision(),
            Err(Error::Storage(StorageError::NeedsRecovery))
        );
        mode.set(Mode::Good);
        must(owner.recover_and_reload());
        assert_eq!(must(owner.revision()), 2);
        assert_eq!(must(owner.read(&access)).utc_ms, 20);
    }
}

#[test]
fn test_should_reject_different_evidence_at_the_same_confirmed_revision() {
    let mode = Rc::new(Cell::new(Mode::Good));
    let owner = must(TimeStore::open(
        Controlled {
            inner: store(Nor::blank()),
            mode: mode.clone(),
        },
        Source::new(0),
        0,
    ));
    must(owner.synchronize(10, 0, &Access::all()));
    mode.set(Mode::RewriteLoad);
    assert_eq!(owner.recover_and_reload(), Err(Error::Integrity));
    assert_eq!(
        owner.revision(),
        Err(Error::Storage(StorageError::NeedsRecovery))
    );
}

#[test]
fn test_should_refuse_revision_wrap_before_touching_storage_but_remember_sample() {
    struct Exhausted;
    impl SnapshotStore for Exhausted {
        fn load(&self, out: &mut [u8]) -> Result<Option<Snapshot>, StorageError> {
            out[..64].copy_from_slice(&image(u64::MAX));
            Ok(Some(Snapshot {
                revision: u64::MAX,
                length: 64,
            }))
        }
        fn commit(&mut self, _expected: u64, _bytes: &[u8]) -> Result<Snapshot, StorageError> {
            panic!("exhausted revision reached commit")
        }
        fn recover(&mut self) -> Result<(), StorageError> {
            Ok(())
        }
    }
    let source = Source::new(200);
    let owner = must(TimeStore::open(Exhausted, source.clone(), 0));
    assert_eq!(
        owner.synchronize(10, 0, &Access::all()),
        Err(Error::Storage(StorageError::RevisionExhausted))
    );
    assert_eq!(must(owner.revision()), u64::MAX);
    source.at(199);
    assert_eq!(owner.read(&Access::all()), Err(Error::NonMonotonic));
}

fn interrupted(generations: u64, cut: Option<usize>, sync_cut: Option<usize>) {
    let media = Nor::blank();
    let source = Source::new(1_000);
    let access = Access::all();
    let owner = must(TimeStore::open(store(media.clone()), source.clone(), 0));
    for revision in 1..=generations {
        must(owner.synchronize(must(i64::try_from(revision)) * 100, 1, &access));
    }
    media.heal();
    source.at(2_000);
    media.0.borrow_mut().cut = cut;
    media.0.borrow_mut().sync_cut = sync_cut;
    let outcome = owner.synchronize(900, 2, &access);
    if outcome.is_err() {
        assert_eq!(
            owner.read(&access),
            Err(Error::Storage(StorageError::NeedsRecovery))
        );
    }
    let (snapshot, _) = owner.into_parts();
    let reopened_media = snapshot.into_medium();
    reopened_media.heal();
    let snapshot = store(reopened_media);
    let owner = must(TimeStore::open(snapshot, source, 0));
    let evidence = must(owner.evidence());
    if outcome.is_ok() {
        assert_eq!(evidence.map(|item| item.utc_ms), Some(900));
    }
    match evidence {
        Some(item) if item.revision == generations + 1 => assert_eq!(
            (item.utc_ms, item.reading.mono_us, item.uncertainty_ms),
            (900, 2_000, 2)
        ),
        Some(item) => assert_eq!(
            (
                item.revision,
                item.utc_ms,
                item.reading.mono_us,
                item.uncertainty_ms
            ),
            (
                generations,
                must(i64::try_from(generations)) * 100,
                1_000,
                1
            )
        ),
        None => assert_eq!(generations, 0),
    }
}

#[test]
fn test_should_reopen_exact_old_or_new_sync_after_each_byte_cut() {
    for generations in 0..=2 {
        for cut in 0..=360 {
            interrupted(generations, Some(cut), None);
        }
    }
}

#[test]
fn test_should_never_acknowledge_failed_commit_sync_barriers() {
    for generations in 0..=2 {
        for barrier in 0..=3 {
            interrupted(generations, None, Some(barrier));
        }
    }
}

#[test]
fn test_should_reject_in_owner_rollback_and_failed_recovery_reads() {
    let media = Nor::blank();
    let source = Source::new(100);
    let access = Access::all();
    let owner = must(TimeStore::open(store(media.clone()), source, 0));
    must(owner.synchronize(10, 0, &access));
    must(owner.synchronize(20, 0, &access));
    media.0.borrow_mut().read_fail = true;
    assert_eq!(
        owner.recover_and_reload(),
        Err(Error::Storage(StorageError::Io))
    );
    assert_eq!(
        owner.read(&access),
        Err(Error::Storage(StorageError::NeedsRecovery))
    );
    media.heal();
    media.0.borrow_mut().bytes[508..512].fill(0);
    assert_eq!(owner.recover_and_reload(), Err(Error::Integrity));
    assert_eq!(
        owner.read(&access),
        Err(Error::Storage(StorageError::NeedsRecovery))
    );
}

fn image(revision: u64) -> [u8; 64] {
    let mut bytes = [0; 64];
    bytes[..8].copy_from_slice(b"DTIME001");
    bytes[8..16].copy_from_slice(&revision.to_le_bytes());
    bytes[16..24].copy_from_slice(&11_u64.to_le_bytes());
    bytes[24..32].copy_from_slice(&22_u64.to_le_bytes());
    bytes
}
#[test]
fn test_should_reject_incompatible_image_identity_reserved_revision_and_exact_length() {
    let mut bad_images = Vec::new();
    let mut bytes = image(1);
    bytes[0] = 0;
    bad_images.push(bytes.to_vec());
    let mut bytes = image(1);
    bytes[56] = 1;
    bad_images.push(bytes.to_vec());
    let mut bytes = image(1);
    bytes[16..24].fill(0);
    bad_images.push(bytes.to_vec());
    let mut bytes = image(1);
    bytes[24..32].fill(0);
    bad_images.push(bytes.to_vec());
    bad_images.push(image(0).to_vec());
    bad_images.push(image(2).to_vec());
    bad_images.push(image(1)[..63].to_vec());
    for bytes in bad_images {
        let mut snapshot = store(Nor::blank());
        must(snapshot.commit(0, &bytes));
        assert!(matches!(
            TimeStore::open(snapshot, Source::new(0), 0),
            Err(Error::Integrity)
        ));
    }
}

#[test]
fn test_should_detect_outer_payload_corruption_without_fabricating_utc() {
    let media = Nor::blank();
    let owner = must(TimeStore::open(store(media.clone()), Source::new(0), 0));
    must(owner.synchronize(10, 0, &Access::all()));
    media.0.borrow_mut().bytes[72] ^= 1;
    assert_eq!(
        owner.recover_and_reload(),
        Err(Error::Storage(StorageError::Corrupt))
    );
}
