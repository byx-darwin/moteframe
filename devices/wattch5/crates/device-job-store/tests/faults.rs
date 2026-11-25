//! Real two-bank byte tears, sync failures and semantic image recovery.
mod support;
use device_job_store::JobStore;
use device_maintenance_service::{
    DeviceJobLedger, DeviceJobState as State, DeviceJobTerminal, Error, ExecutionClaim,
};
use device_storage::persistence::{Error as StorageError, Snapshot, SnapshotStore};
use std::{cell::Cell, rc::Rc};
use support::*;

#[derive(Clone, Copy, Debug)]
enum Step {
    Initialize,
    Claim,
    Start,
    Finish,
    Boot,
}
fn terminal() -> DeviceJobTerminal {
    DeviceJobTerminal {
        state: State::Succeeded,
        result: 0,
        reason: 0,
    }
}
fn perform(ledger: &mut Ledger, boot: &Boot, step: Step) -> Result<(), Error> {
    let access = Access::new(7);
    match step {
        Step::Initialize => ledger.initialize(),
        Step::Claim => ledger.claim_atomic(request(1, 1), &access).map(|_| ()),
        Step::Start => {
            let record = ledger.query([1; 16], &access)?;
            ledger.begin_execution_atomic(record, &access).map(|_| ())
        }
        Step::Finish => {
            let record = ledger.query([1; 16], &access)?;
            ledger.finish_atomic(record, terminal()).map(|_| ())
        }
        Step::Boot => {
            boot.0.set(Ok(2));
            ledger.recover_and_reload()
        }
    }
}
fn baseline(step: Step, reused: bool) -> Nor {
    let media = Nor::blank();
    let mut ledger = open(media.clone(), Boot::new(), Policy::new(), Isolation::new());
    if !matches!(step, Step::Initialize) {
        must(ledger.initialize());
    }
    if reused {
        let access = Access::new(7);
        let record = must(ledger.claim_atomic(request(9, 2), &access)).record;
        let ExecutionClaim::Acquired(record) = must(ledger.begin_execution_atomic(record, &access))
        else {
            panic!("missing claim")
        };
        must(ledger.finish_atomic(record, terminal()));
    }
    if matches!(step, Step::Start | Step::Finish | Step::Boot) {
        must(ledger.claim_atomic(request(1, 1), &Access::new(7)));
    }
    if matches!(step, Step::Finish | Step::Boot) {
        let record = must(ledger.query([1; 16], &Access::new(7)));
        must(ledger.begin_execution_atomic(record, &Access::new(7)));
    }
    media.heal();
    media
}
fn exercise(base: &Nor, step: Step, cut: Option<usize>, sync_cut: Option<usize>) {
    let media = base.duplicate();
    let boot = Boot::new();
    let mut ledger = open(media.clone(), boot.clone(), Policy::new(), Isolation::new());
    media.heal();
    media.0.borrow_mut().cut = cut;
    media.0.borrow_mut().sync_cut = sync_cut;
    let outcome = perform(&mut ledger, &boot, step);
    if outcome.is_err() {
        assert!(
            ledger.revision().is_err(),
            "uncertainty was exposed: {step:?}/{cut:?}/{sync_cut:?}"
        );
    }
    let (s, b, p, i) = ledger.into_parts();
    let media = s.into_medium();
    media.heal();
    let mut ledger = must(JobStore::<_, _, _, _, 32, 4096>::open(
        snapshot(media),
        b,
        p,
        i,
    ));
    if matches!(step, Step::Initialize) {
        if ledger.revision().is_err() {
            must(ledger.initialize());
        }
        assert_eq!(must(ledger.retained()), 0);
        return;
    }
    let access = Access::new(7);
    match step {
        Step::Claim => match ledger.query([1; 16], &access) {
            Ok(record) => {
                assert_eq!(record.state, State::Accepted);
                assert!(must(ledger.claim_atomic(request(1, 1), &access)).replayed);
            }
            Err(Error::NotFound) => {
                assert!(outcome.is_err());
            }
            other => panic!("unexpected claim recovery {other:?}"),
        },
        Step::Start => {
            let record = must(ledger.query([1; 16], &access));
            assert!(matches!(record.state, State::Accepted | State::Running));
            if outcome.is_ok() {
                assert_eq!(record.state, State::Running);
            }
        }
        Step::Finish => {
            let record = must(ledger.query([1; 16], &access));
            assert!(matches!(record.state, State::Running | State::Succeeded));
            if outcome.is_ok() {
                assert_eq!(record.state, State::Succeeded);
            }
        }
        Step::Boot => {
            let record = must(ledger.query([1; 16], &access));
            assert_eq!(
                (record.state, record.result, record.reason),
                (State::Interrupted, 4, 0xff06)
            );
            assert!(must(ledger.claim_atomic(request(1, 1), &access)).replayed);
        }
        Step::Initialize => {}
    }
    if base.0.borrow().bytes.iter().any(|&b| b != 255) && must(ledger.retained()) > 1 {
        assert_eq!(must(ledger.query([9; 16], &access)).state, State::Succeeded);
    }
}
#[test]
fn test_should_recover_every_byte_cut_and_all_sync_barriers_without_inventing_effects() {
    for step in [
        Step::Initialize,
        Step::Claim,
        Step::Start,
        Step::Finish,
        Step::Boot,
    ] {
        for reused in [false, true] {
            if reused && matches!(step, Step::Initialize) {
                continue;
            }
            let base = baseline(step, reused);
            let probe = base.duplicate();
            let boot = Boot::new();
            let mut ledger = open(probe.clone(), boot.clone(), Policy::new(), Isolation::new());
            probe.heal();
            must(perform(&mut ledger, &boot, step));
            let effects = probe.0.borrow().effects;
            let syncs = probe.0.borrow().syncs;
            for cut in 0..=effects {
                exercise(&base, step, Some(cut), None);
            }
            for sync in 0..syncs {
                exercise(&base, step, None, Some(sync));
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Mode {
    Good,
    Reject(StorageError),
    AfterCommit,
    BadLength,
    BadRevision,
    Blank,
    Rewrite,
    LoadError,
    RecoverError,
}
#[derive(Debug)]
struct Controlled {
    store: Store,
    mode: Rc<Cell<Mode>>,
}
impl SnapshotStore for Controlled {
    fn load(&self, out: &mut [u8]) -> Result<Option<Snapshot>, StorageError> {
        match self.mode.get() {
            Mode::Blank => return Ok(None),
            Mode::LoadError => return Err(StorageError::Io),
            _ => {}
        }
        let result = self.store.load(out)?;
        if matches!(self.mode.get(), Mode::Rewrite) && result.is_some() {
            out[64] ^= 1;
        }
        Ok(result)
    }
    fn commit(&mut self, expected: u64, bytes: &[u8]) -> Result<Snapshot, StorageError> {
        if let Mode::Reject(e) = self.mode.get() {
            return Err(e);
        }
        let mut s = self.store.commit(expected, bytes)?;
        match self.mode.get() {
            Mode::AfterCommit => return Err(StorageError::Io),
            Mode::BadLength => s.length -= 1,
            Mode::BadRevision => s.revision = 0,
            _ => {}
        }
        Ok(s)
    }
    fn recover(&mut self) -> Result<(), StorageError> {
        if matches!(self.mode.get(), Mode::RecoverError) {
            return Err(StorageError::Io);
        }
        self.store.recover()
    }
}
#[test]
fn test_should_preserve_known_prechecks_and_poison_unknown_commit_receipts() {
    for mode in [
        Mode::Reject(StorageError::Capacity),
        Mode::Reject(StorageError::Conflict),
        Mode::Reject(StorageError::RevisionExhausted),
        Mode::AfterCommit,
        Mode::BadLength,
        Mode::BadRevision,
    ] {
        let control = Rc::new(Cell::new(Mode::Good));
        let mut ledger = must(JobStore::<_, _, _, _, 32, 4096>::open(
            Controlled {
                store: snapshot(Nor::blank()),
                mode: control.clone(),
            },
            Boot::new(),
            Policy::new(),
            Isolation::new(),
        ));
        must(ledger.initialize());
        let access = Access::new(7);
        must(ledger.claim_atomic(request(1, 1), &access));
        let revision = must(ledger.revision());
        control.set(mode);
        assert!(ledger.claim_atomic(request(2, 2), &access).is_err());
        if matches!(mode, Mode::Reject(_)) {
            assert_eq!(must(ledger.revision()), revision);
            assert_eq!(must(ledger.retained()), 1);
        } else {
            assert_eq!(ledger.revision(), Err(Error::Storage));
        }
        control.set(Mode::Good);
        must(ledger.recover_and_reload());
        if matches!(mode, Mode::Reject(_)) {
            assert_eq!(must(ledger.retained()), 1);
        } else {
            assert_eq!(must(ledger.retained()), 2);
            assert!(must(ledger.claim_atomic(request(2, 2), &access)).replayed);
        }
    }
}
#[test]
fn test_should_preserve_confirmed_history_and_poison_load_or_recovery_faults() {
    for mode in [
        Mode::Blank,
        Mode::Rewrite,
        Mode::LoadError,
        Mode::RecoverError,
    ] {
        let control = Rc::new(Cell::new(Mode::Good));
        let mut ledger = must(JobStore::<_, _, _, _, 32, 4096>::open(
            Controlled {
                store: snapshot(Nor::blank()),
                mode: control.clone(),
            },
            Boot::new(),
            Policy::new(),
            Isolation::new(),
        ));
        must(ledger.initialize());
        must(ledger.claim_atomic(request(1, 1), &Access::new(7)));
        control.set(mode);
        assert!(ledger.recover_and_reload().is_err());
        assert_eq!(ledger.revision(), Err(Error::Storage));
        control.set(Mode::Good);
        must(ledger.recover_and_reload());
        assert_eq!(must(ledger.retained()), 1);
    }
}
#[test]
fn test_should_reject_semantic_corruption_even_with_recomputed_outer_crc() {
    let mut ledger = initialized();
    must(ledger.claim_atomic(request(1, 1), &Access::new(7)));
    let (store, _, _, _) = ledger.into_parts();
    let mut original = [0; 4096];
    let metadata = must(store.load(&mut original)).unwrap_or_else(|| panic!("missing image"));
    for offset in [
        0, 8, 10, 12, 14, 16, 24, 32, 40, 48, 52, 54, 96, 100, 102, 104, 105, 106, 108, 110, 112,
        114, 116, 124, 125, 128,
    ] {
        let mut bad = original;
        bad[offset] ^= 1;
        let mut store = snapshot(Nor::blank());
        must(store.commit(0, &[0]));
        must(store.commit(1, &bad[..metadata.length]));
        assert!(
            JobStore::<_, _, _, _, 32, 4096>::open(
                store,
                Boot::new(),
                Policy::new(),
                Isolation::new()
            )
            .is_err(),
            "accepted corrupt offset {offset}"
        );
    }
}
#[test]
fn test_should_refuse_initialization_when_real_snapshot_cannot_hold_declared_retention() {
    let small = must(device_storage::persistence::AtomicSnapshot::<_, 64>::open(
        Nor::blank(),
        device_storage::persistence::Layout {
            start: 0,
            bank_bytes: 8192,
        },
    ));
    let mut ledger = must(JobStore::<_, _, _, _, 32, 4096>::open(
        small,
        Boot::new(),
        Policy::new(),
        Isolation::new(),
    ));
    assert_eq!(ledger.initialize(), Err(Error::Capacity));
    assert!(ledger.revision().is_err());
}

#[test]
fn test_should_reject_actual_committed_payload_or_metadata_crc_corruption() {
    for relative in [8, 32 + 64] {
        let media = Nor::blank();
        let mut ledger = open(media.clone(), Boot::new(), Policy::new(), Isolation::new());
        must(ledger.initialize());
        must(ledger.claim_atomic(request(1, 1), &Access::new(7)));
        drop(ledger);
        media.0.borrow_mut().bytes[8192 + relative] ^= 1;
        assert!(
            device_storage::persistence::AtomicSnapshot::<_, 4096>::open(
                media,
                device_storage::persistence::Layout {
                    start: 0,
                    bank_bytes: 8192
                }
            )
            .is_err()
        );
    }
}

#[test]
fn test_should_refuse_reachable_boot_revision_exhaustion_before_any_store_write() {
    struct Exhausted {
        image: [u8; 2112],
    }
    impl SnapshotStore for Exhausted {
        fn load(&self, out: &mut [u8]) -> Result<Option<Snapshot>, StorageError> {
            out[..2112].copy_from_slice(&self.image);
            Ok(Some(Snapshot {
                revision: u64::MAX,
                length: 2112,
            }))
        }
        fn commit(&mut self, _: u64, _: &[u8]) -> Result<Snapshot, StorageError> {
            panic!("exhausted revision reached storage mutation")
        }
        fn recover(&mut self) -> Result<(), StorageError> {
            Ok(())
        }
    }
    let ledger = initialized();
    let (store, _, _, _) = ledger.into_parts();
    let mut bytes = [0; 4096];
    must(store.load(&mut bytes));
    bytes[16..24].copy_from_slice(&u64::MAX.to_le_bytes());
    bytes[24..32].copy_from_slice(&u64::MAX.to_le_bytes());
    bytes[40..48].copy_from_slice(&(u64::MAX - 1).to_le_bytes());
    let mut image = [0; 2112];
    image.copy_from_slice(&bytes[..2112]);
    let boot = Boot::new();
    boot.0.set(Ok(u64::MAX));
    let mut ledger = must(JobStore::<_, _, _, _, 32, 4096>::open(
        Exhausted { image },
        boot,
        Policy::new(),
        Isolation::new(),
    ));
    assert_eq!(
        ledger.claim_atomic(request(1, 1), &Access::new(7)),
        Err(Error::Storage)
    );
    assert_eq!(must(ledger.revision()), u64::MAX);
    assert_eq!(must(ledger.retained()), 0);
}

#[test]
fn test_should_reject_valid_newer_images_that_replace_or_drop_confirmed_identities() {
    use std::cell::RefCell;
    type Replacement = Rc<RefCell<Option<([u8; 4096], Snapshot)>>>;
    struct Injected {
        store: Store,
        replacement: Replacement,
    }
    impl SnapshotStore for Injected {
        fn load(&self, out: &mut [u8]) -> Result<Option<Snapshot>, StorageError> {
            if let Some((bytes, metadata)) = &*self.replacement.borrow() {
                out[..metadata.length].copy_from_slice(&bytes[..metadata.length]);
                return Ok(Some(*metadata));
            }
            self.store.load(out)
        }
        fn commit(&mut self, expected: u64, bytes: &[u8]) -> Result<Snapshot, StorageError> {
            self.store.commit(expected, bytes)
        }
        fn recover(&mut self) -> Result<(), StorageError> {
            self.store.recover()
        }
    }
    for dropped in [false, true] {
        let mut old = initialized();
        must(old.claim_atomic(request(1, 1), &Access::new(7)));
        if dropped {
            must(old.claim_atomic(request(2, 1), &Access::new(7)));
        }
        let (store, boot, policy, isolation) = old.into_parts();
        let replacement = Rc::new(RefCell::new(None));
        let mut ledger = must(JobStore::<_, _, _, _, 32, 4096>::open(
            Injected {
                store,
                replacement: replacement.clone(),
            },
            boot,
            policy,
            isolation,
        ));
        let mut newer = initialized();
        let record =
            must(newer.claim_atomic(request(if dropped { 1 } else { 9 }, 1), &Access::new(7)))
                .record;
        let ExecutionClaim::Acquired(record) =
            must(newer.begin_execution_atomic(record, &Access::new(7)))
        else {
            panic!("missing claim")
        };
        must(newer.finish_atomic(record, terminal()));
        let (store, _, _, _) = newer.into_parts();
        let mut bytes = [0; 4096];
        let metadata =
            must(store.load(&mut bytes)).unwrap_or_else(|| panic!("missing newer image"));
        *replacement.borrow_mut() = Some((bytes, metadata));
        assert_eq!(ledger.recover_and_reload(), Err(Error::Storage));
        assert_eq!(ledger.revision(), Err(Error::Storage));
        *replacement.borrow_mut() = None;
        must(ledger.recover_and_reload());
        assert_eq!(must(ledger.retained()), if dropped { 2 } else { 1 });
    }
}

struct Switched(Rc<std::cell::RefCell<Store>>);
impl SnapshotStore for Switched {
    fn load(&self, out: &mut [u8]) -> Result<Option<Snapshot>, StorageError> {
        self.0.borrow().load(out)
    }
    fn commit(&mut self, expected: u64, bytes: &[u8]) -> Result<Snapshot, StorageError> {
        self.0.borrow_mut().commit(expected, bytes)
    }
    fn recover(&mut self) -> Result<(), StorageError> {
        self.0.borrow_mut().recover()
    }
}
fn lineage(epochs: &[u64], append: bool) -> Store {
    let boot = Boot::new();
    let mut ledger = open(Nor::blank(), boot.clone(), Policy::new(), Isolation::new());
    must(ledger.initialize());
    let accepted = must(ledger.claim_atomic(request(1, 1), &Access::new(7))).record;
    let ExecutionClaim::Acquired(running) =
        must(ledger.begin_execution_atomic(accepted, &Access::new(7)))
    else {
        panic!("missing running")
    };
    must(ledger.finish_atomic(running, terminal()));
    for &epoch in epochs {
        boot.0.set(Ok(epoch));
        must(ledger.recover_and_reload());
    }
    if append {
        must(ledger.claim_atomic(request(2, 1), &Access::new(7)));
    }
    ledger.into_parts().0
}
fn recover_lineage(
    old: Store,
    new: Store,
    old_boot: u64,
    new_boot: u64,
    expected: Result<(), Error>,
) {
    let backing = Rc::new(std::cell::RefCell::new(old));
    let boot = Boot::new();
    boot.0.set(Ok(old_boot));
    let mut ledger = must(JobStore::<_, _, _, _, 32, 4096>::open(
        Switched(backing.clone()),
        boot.clone(),
        Policy::new(),
        Isolation::new(),
    ));
    *backing.borrow_mut() = new;
    boot.0.set(Ok(new_boot));
    assert_eq!(ledger.recover_and_reload(), expected);
    if expected.is_err() {
        assert_eq!(ledger.revision(), Err(Error::Storage));
    }
}
#[test]
fn test_should_reject_recovery_extra_boot_update_without_epoch_advance() {
    recover_lineage(
        lineage(&[3], false),
        lineage(&[2, 3], false),
        3,
        3,
        Err(Error::Storage),
    );
}
#[test]
fn test_should_reject_recovery_epoch_advance_without_new_boot_update() {
    recover_lineage(
        lineage(&[2], false),
        lineage(&[3], true),
        2,
        3,
        Err(Error::Storage),
    );
}
#[test]
fn test_should_reject_recovery_backdated_new_identity_before_confirmed_boot() {
    let boot = Boot::new();
    boot.0.set(Ok(2));
    let mut alternate = must(JobStore::<_, _, _, _, 32, 4096>::open(
        lineage(&[2], false),
        boot.clone(),
        Policy::new(),
        Isolation::new(),
    ));
    must(alternate.claim_atomic(request(2, 1), &Access::new(7)));
    boot.0.set(Ok(4));
    must(alternate.recover_and_reload());
    recover_lineage(
        lineage(&[3], false),
        alternate.into_parts().0,
        3,
        4,
        Err(Error::Storage),
    );
}
#[test]
fn test_should_preserve_recovery_with_skipped_epochs_and_multiple_valid_transitions() {
    recover_lineage(
        lineage(&[2], false),
        lineage(&[2, 4, 8], true),
        2,
        8,
        Ok(()),
    );
    recover_lineage(lineage(&[2], false), lineage(&[2], true), 2, 2, Ok(()));
}

#[test]
fn test_should_reject_reverse_admission_order_with_valid_aggregate_and_outer_crc() {
    let boot = Boot::new();
    boot.0.set(Ok(2));
    let mut ledger = must(JobStore::<_, _, _, _, 32, 4096>::open(
        lineage(&[2], true),
        boot.clone(),
        Policy::new(),
        Isolation::new(),
    ));
    let accepted = must(ledger.query([2; 16], &Access::new(7)));
    let ExecutionClaim::Acquired(running) =
        must(ledger.begin_execution_atomic(accepted, &Access::new(7)))
    else {
        panic!("missing running")
    };
    must(ledger.finish_atomic(running, terminal()));
    let (store, _, _, _) = ledger.into_parts();
    let mut bytes = [0; 4096];
    let metadata = must(store.load(&mut bytes)).unwrap_or_else(|| panic!("missing image"));
    assert_eq!(metadata.revision, 8);
    bytes[116..124].copy_from_slice(&2_u64.to_le_bytes());
    bytes[180..188].copy_from_slice(&1_u64.to_le_bytes());
    let mut store = snapshot(Nor::blank());
    for expected in 0..7 {
        must(store.commit(expected, &[0]));
    }
    must(store.commit(7, &bytes[..metadata.length]));
    assert!(
        JobStore::<_, _, _, _, 32, 4096>::open(store, boot, Policy::new(), Isolation::new())
            .is_err()
    );
}

fn complete_second_at_then_boot(admission: u64, current: u64) -> Store {
    let boot = Boot::new();
    boot.0.set(Ok(admission));
    let mut ledger = must(JobStore::<_, _, _, _, 32, 4096>::open(
        lineage(&[admission], true),
        boot.clone(),
        Policy::new(),
        Isolation::new(),
    ));
    let accepted = must(ledger.query([2; 16], &Access::new(7)));
    let ExecutionClaim::Acquired(running) =
        must(ledger.begin_execution_atomic(accepted, &Access::new(7)))
    else {
        panic!("missing running")
    };
    must(ledger.finish_atomic(running, terminal()));
    boot.0.set(Ok(current));
    must(ledger.recover_and_reload());
    ledger.into_parts().0
}
#[test]
fn test_should_reject_too_few_updates_for_distinct_retained_admission_epochs() {
    let store = complete_second_at_then_boot(2, 3);
    let mut bytes = [0; 4096];
    let metadata = must(store.load(&mut bytes)).unwrap_or_else(|| panic!("missing image"));
    assert_eq!(metadata.revision, 9);
    bytes[40..48].copy_from_slice(&1_u64.to_le_bytes());
    bytes[16..24].copy_from_slice(&8_u64.to_le_bytes());
    let mut store = snapshot(Nor::blank());
    for expected in 0..7 {
        must(store.commit(expected, &[0]));
    }
    must(store.commit(7, &bytes[..metadata.length]));
    let boot = Boot::new();
    boot.0.set(Ok(3));
    assert!(
        JobStore::<_, _, _, _, 32, 4096>::open(store, boot, Policy::new(), Isolation::new())
            .is_err()
    );
}
#[test]
fn test_should_reject_too_few_delta_updates_for_new_admission_epochs() {
    recover_lineage(
        lineage(&[2], false),
        complete_second_at_then_boot(3, 4),
        2,
        4,
        Err(Error::Storage),
    );
}
