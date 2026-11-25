//! The simulated NOR owns in-flight bytes and can lose power at each byte/slice.
use core::task::Poll;
use device_storage::incremental::{
    AsyncError, AsyncMedium, CommitId, Completion, DurableReceipt, IncrementalSnapshot, MediumOp,
    StartError,
};
use device_storage::persistence::{
    AtomicSnapshot, Error, Geometry, Layout, Medium, Snapshot, SnapshotStore,
};
use std::{cell::Cell, rc::Rc};

const LAYOUT: Layout = Layout {
    start: 0,
    bank_bytes: 512,
};
const GEOMETRY: Geometry = Geometry {
    capacity: 1024,
    erase_size: 64,
    write_size: 4,
};
#[derive(Clone, Debug)]
enum Owned {
    Read(u32, usize),
    Program(u32, Vec<u8>),
    Erase(u32),
    Sync,
}
#[derive(Clone, Debug)]
struct Nor {
    data: Vec<u8>,
    durable: Vec<u8>,
    geometry: Geometry,
    pending: Option<Owned>,
    pending_polls: usize,
    actions: Rc<Cell<usize>>,
    cut: Option<usize>,
    effects: usize,
    reverse: bool,
    action_fail: Option<usize>,
    sync_fail: Option<usize>,
    syncs: usize,
    flush_cut: usize,
    flush_reverse: bool,
    buffered: bool,
    malformed: Option<usize>,
    corrupt_read: Option<usize>,
    reads: usize,
    busy_once: Option<()>,
}
impl Nor {
    fn blank() -> Self {
        Self {
            data: vec![255; 1024],
            durable: vec![255; 1024],
            geometry: GEOMETRY,
            pending: None,
            pending_polls: 0,
            actions: Rc::new(Cell::new(0)),
            cut: None,
            effects: 0,
            reverse: false,
            action_fail: None,
            sync_fail: None,
            syncs: 0,
            flush_cut: 0,
            flush_reverse: false,
            buffered: false,
            malformed: None,
            corrupt_read: None,
            reads: 0,
            busy_once: None,
        }
    }
    fn effect(&mut self) -> Result<(), Error> {
        if self.cut == Some(self.effects) {
            return Err(Error::Io);
        }
        self.effects += 1;
        Ok(())
    }
    fn action(&mut self) -> Result<usize, Error> {
        let at = self.actions.get();
        self.actions.set(at + 1);
        if self.action_fail == Some(at) {
            Err(Error::Io)
        } else {
            Ok(at)
        }
    }
    fn restart(mut self) -> Self {
        if self.buffered {
            self.data.clone_from(&self.durable);
        }
        self.pending = None;
        self.cut = None;
        self.effects = 0;
        self.action_fail = None;
        self.sync_fail = None;
        self.syncs = 0;
        self.malformed = None;
        self.corrupt_read = None;
        self.reads = 0;
        self.actions = Rc::new(Cell::new(0));
        self.busy_once = None;
        self
    }
}
impl Medium for Nor {
    fn geometry(&self) -> Geometry {
        self.geometry
    }
    fn read(&mut self, offset: u32, out: &mut [u8]) -> Result<(), Error> {
        out.copy_from_slice(&self.data[offset as usize..offset as usize + out.len()]);
        Ok(())
    }
    fn program(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Error> {
        assert!(offset.is_multiple_of(self.geometry.write_size));
        assert!(
            bytes
                .len()
                .is_multiple_of(self.geometry.write_size as usize)
        );
        for i in 0..bytes.len() {
            let i = if self.reverse { bytes.len() - 1 - i } else { i };
            self.effect()?;
            let at = offset as usize + i;
            assert_eq!(self.data[at] & bytes[i], bytes[i], "NOR 0-to-1");
            self.data[at] = bytes[i];
        }
        Ok(())
    }
    fn erase(&mut self, offset: u32, length: u32) -> Result<(), Error> {
        assert!(offset.is_multiple_of(self.geometry.erase_size));
        for i in 0..length {
            let i = if self.reverse { length - 1 - i } else { i };
            self.effect()?;
            self.data[(offset + i) as usize] = 255;
        }
        Ok(())
    }
    fn sync(&mut self) -> Result<(), Error> {
        let at = self.syncs;
        self.syncs += 1;
        if self.sync_fail == Some(at) {
            let range = if self.flush_reverse {
                self.data.len() - self.flush_cut..self.data.len()
            } else {
                0..self.flush_cut
            };
            self.durable[range.clone()].copy_from_slice(&self.data[range]);
            return Err(Error::Io);
        }
        self.durable.clone_from(&self.data);
        Ok(())
    }
}
impl AsyncMedium for Nor {
    fn geometry(&self) -> Geometry {
        self.geometry
    }
    fn begin(&mut self, op: MediumOp<'_>) -> Result<(), StartError> {
        self.action().map_err(|_| StartError::Io)?;
        if self.pending.is_some() || self.busy_once.take().is_some() {
            self.busy_once = None;
            return Err(StartError::Busy);
        }
        self.pending = Some(match op {
            MediumOp::Read { offset, length } => {
                assert!((1..=256).contains(&length));
                Owned::Read(offset, length)
            }
            MediumOp::Program { offset, bytes } => {
                assert!((1..=256).contains(&bytes.len()));
                Owned::Program(offset, bytes.to_vec())
            }
            MediumOp::Erase { offset } => Owned::Erase(offset),
            MediumOp::Sync => Owned::Sync,
        });
        self.pending_polls = 1;
        Ok(())
    }
    fn poll(&mut self, out: &mut [u8]) -> Poll<Result<Completion, Error>> {
        let action = match self.action() {
            Ok(at) => at,
            Err(e) => {
                self.pending = None;
                return Poll::Ready(Err(e));
            }
        };
        assert_eq!(out.len(), 256);
        if self.pending_polls > 0 {
            self.pending_polls -= 1;
            return Poll::Pending;
        }
        let op = self
            .pending
            .take()
            .unwrap_or_else(|| panic!("poll without admission"));
        let result = match op {
            Owned::Read(offset, length) => {
                let read = self.reads;
                self.reads += 1;
                self.read(offset, &mut out[..length]).map(|()| {
                    if self.corrupt_read == Some(read) {
                        out[0] ^= 1;
                    }
                    Completion::Read(length)
                })
            }
            Owned::Program(offset, bytes) => self
                .program(offset, &bytes)
                .map(|()| Completion::Programmed),
            Owned::Erase(offset) => self
                .erase(offset, self.geometry.erase_size)
                .map(|()| Completion::Erased),
            Owned::Sync => self.sync().map(|()| Completion::Synced),
        };
        Poll::Ready(if self.malformed == Some(action) {
            Ok(Completion::Read(257))
        } else {
            result
        })
    }
}
type Store<const N: usize = 256> = IncrementalSnapshot<Nor, N>;
fn drive_recover<const N: usize>(s: &mut Store<N>) -> Result<(), AsyncError> {
    s.begin_recover()?;
    for _ in 0..100_000 {
        match s.poll_recover() {
            Poll::Pending => {}
            Poll::Ready(r) => return r,
        }
    }
    panic!("incremental recovery never completed")
}
fn open<const N: usize>(m: Nor) -> Store<N> {
    let actions = m.actions.clone();
    let before = actions.get();
    let mut s = Store::new_unrecovered(m, LAYOUT).unwrap_or_else(|e| panic!("layout {e:?}"));
    assert_eq!(actions.get(), before, "constructor must not do I/O");
    assert_eq!(s.load_committed(&mut [0; N]), Err(Error::NeedsRecovery));
    drive_recover(&mut s).unwrap_or_else(|e| panic!("recover {e:?}"));
    s
}
fn drive_commit<const N: usize>(
    s: &mut Store<N>,
    id: CommitId,
) -> Result<DurableReceipt, AsyncError> {
    for _ in 0..100_000 {
        match s.poll_commit(id) {
            Poll::Pending => {}
            Poll::Ready(r) => return r,
        }
    }
    panic!("incremental commit never completed")
}
fn value<const N: usize>(s: &Store<N>) -> Option<(u64, Vec<u8>)> {
    let mut out = [0; N];
    s.load_committed(&mut out)
        .unwrap_or_else(|e| panic!("load {e:?}"))
        .map(|s| (s.revision, out[..s.length].to_vec()))
}
fn populated(generation: u64) -> Nor {
    let mut s = AtomicSnapshot::<_, 256>::open(Nor::blank(), LAYOUT)
        .unwrap_or_else(|e| panic!("legacy {e:?}"));
    for r in 1..=generation {
        s.commit(r - 1, &[u8::try_from(r).unwrap_or_default(); 13])
            .unwrap_or_else(|e| panic!("legacy {e:?}"));
    }
    s.into_medium().restart()
}
fn legacy_value(m: Nor) -> Option<(u64, Vec<u8>)> {
    let s = AtomicSnapshot::<_, 256>::open(m.restart(), LAYOUT)
        .unwrap_or_else(|e| panic!("legacy reopen {e:?}"));
    let mut out = [0; 256];
    s.load(&mut out)
        .unwrap_or_else(|e| panic!("legacy load {e:?}"))
        .map(|s| (s.revision, out[..s.length].to_vec()))
}
#[test]
fn test_should_recover_and_preserve_owned_old_image_until_exact_durable_completion() {
    let mut s = open::<256>(populated(3));
    let mut candidate = [7; 256];
    let id = s
        .begin_commit(3, &candidate)
        .unwrap_or_else(|e| panic!("begin {e:?}"));
    candidate.fill(9);
    assert_eq!(s.begin_commit(3, b"other"), Err(AsyncError::Busy));
    assert_eq!(s.begin_recover(), Err(AsyncError::Busy));
    loop {
        assert_eq!(value(&s), Some((3, vec![3; 13])));
        match s.poll_commit(id) {
            Poll::Pending => {}
            Poll::Ready(r) => {
                assert_eq!(
                    r,
                    Ok(DurableReceipt {
                        id,
                        snapshot: Snapshot {
                            revision: 4,
                            length: 256
                        }
                    })
                );
                break;
            }
        }
    }
    assert_eq!(value(&s), Some((4, vec![7; 256])));
    assert_eq!(
        s.poll_commit(id),
        Poll::Ready(Ok(DurableReceipt {
            id,
            snapshot: Snapshot {
                revision: 4,
                length: 256
            }
        }))
    );
    assert_eq!(legacy_value(s.into_medium()), Some((4, vec![7; 256])));
}
#[test]
fn test_should_do_at_most_one_admission_or_poll_per_slice_and_recover_blank() {
    let m = Nor::blank();
    let actions = m.actions.clone();
    let mut s = Store::<256>::new_unrecovered(m, LAYOUT).unwrap_or_else(|e| panic!("new {e:?}"));
    s.begin_recover()
        .unwrap_or_else(|e| panic!("begin recover {e:?}"));
    loop {
        let before = actions.get();
        let result = s.poll_recover();
        assert!(actions.get() - before <= 1);
        if let Poll::Ready(r) = result {
            assert_eq!(r, Ok(()));
            break;
        }
    }
    assert_eq!(value(&s), None);
    let id = s
        .begin_commit(0, &[42; 256])
        .unwrap_or_else(|e| panic!("begin {e:?}"));
    loop {
        let before = actions.get();
        let result = s.poll_commit(id);
        assert!(actions.get() - before <= 1);
        if let Poll::Ready(r) = result {
            assert!(r.is_ok());
            break;
        }
    }
    assert_eq!(legacy_value(s.into_medium()), Some((1, vec![42; 256])));
}
#[test]
fn test_should_reject_stale_tokens_preflight_and_keep_terminal_results() {
    let mut s = open::<256>(Nor::blank());
    assert_eq!(
        s.begin_commit(1, b"bad"),
        Err(AsyncError::Storage(Error::Conflict))
    );
    assert_eq!(
        s.begin_commit(0, &[1; 257]),
        Err(AsyncError::Storage(Error::Capacity))
    );
    let first = s
        .begin_commit(0, b"first")
        .unwrap_or_else(|e| panic!("begin {e:?}"));
    let receipt = drive_commit(&mut s, first).unwrap_or_else(|e| panic!("commit {e:?}"));
    assert_eq!(
        s.begin_commit(0, b"conflict"),
        Err(AsyncError::Storage(Error::Conflict))
    );
    assert_eq!(s.poll_commit(first), Poll::Ready(Ok(receipt)));
    let second = s
        .begin_commit(1, b"second")
        .unwrap_or_else(|e| panic!("begin {e:?}"));
    assert_ne!(first, second);
    assert_eq!(
        s.poll_commit(first),
        Poll::Ready(Err(AsyncError::InvalidOperation))
    );
    assert!(drive_commit(&mut s, second).is_ok());
    drive_recover(&mut s).unwrap_or_else(|e| panic!("recover {e:?}"));
    assert_eq!(
        s.poll_commit(second),
        Poll::Ready(Err(AsyncError::InvalidOperation))
    );
    assert_eq!(s.poll_recover(), Poll::Ready(Ok(())));
    assert_eq!(value(&s), Some((2, b"second".to_vec())));
}
#[test]
fn test_should_recover_old_or_new_at_every_mutation_byte_in_both_orders() {
    for generation in 0..=5 {
        for reverse in [false, true] {
            let mut m = populated(generation);
            m.reverse = reverse;
            let mut s = open::<256>(m.clone());
            let id = s
                .begin_commit(generation, b"replacement-longer")
                .unwrap_or_else(|e| panic!("begin {e:?}"));
            assert!(drive_commit(&mut s, id).is_ok());
            let effects = s.into_medium().effects;
            for cut in 0..=effects {
                let mut m = m.clone();
                m.cut = Some(cut);
                let mut s = open::<256>(m);
                let id = s
                    .begin_commit(generation, b"replacement-longer")
                    .unwrap_or_else(|e| panic!("begin {e:?}"));
                let result = drive_commit(&mut s, id);
                if result.is_err() {
                    assert_eq!(s.load_committed(&mut [0; 256]), Err(Error::NeedsRecovery));
                    assert_eq!(
                        s.begin_commit(generation, b"bad"),
                        Err(AsyncError::Storage(Error::NeedsRecovery))
                    );
                    assert_eq!(s.poll_commit(id), Poll::Ready(result));
                }
                let m = s.into_medium().restart();
                let old = (generation > 0).then(|| {
                    (
                        generation,
                        vec![
                            u8::try_from(generation).unwrap_or_else(|e| panic!("generation {e:?}"));
                            13
                        ],
                    )
                });
                let new = Some((generation + 1, b"replacement-longer".to_vec()));
                let actual = legacy_value(m.clone());
                assert!(
                    actual == old || actual == new,
                    "g {generation} reverse {reverse} cut {cut}: {actual:?}"
                );
                if result.is_ok() {
                    assert_eq!(actual, new);
                }
                assert_eq!(value(&open::<256>(m)), actual);
            }
        }
    }
}
#[test]
fn test_should_survive_power_loss_before_and_after_every_admission_and_completion() {
    for generation in [0, 1, 3, 4] {
        let baseline = populated(generation);
        let mut full = open::<256>(baseline.clone());
        let id = full
            .begin_commit(generation, b"new")
            .unwrap_or_else(|e| panic!("begin {e:?}"));
        let mut slices = 0;
        loop {
            slices += 1;
            if full.poll_commit(id).is_ready() {
                break;
            }
        }
        for cut in 0..=slices {
            let mut s = open::<256>(baseline.clone());
            let id = s
                .begin_commit(generation, b"new")
                .unwrap_or_else(|e| panic!("begin {e:?}"));
            let mut acknowledged = false;
            for _ in 0..cut {
                acknowledged |= matches!(s.poll_commit(id), Poll::Ready(Ok(_)));
            }
            let m = s.into_medium().restart();
            let actual = legacy_value(m.clone());
            let old = (generation > 0).then(|| {
                (
                    generation,
                    vec![
                        u8::try_from(generation).unwrap_or_else(|e| panic!("generation {e:?}"));
                        13
                    ],
                )
            });
            let new = Some((generation + 1, b"new".to_vec()));
            assert!(actual == old || actual == new);
            if acknowledged {
                assert_eq!(actual, new);
            }
            assert_eq!(value(&open::<256>(m)), actual);
        }
    }
}
#[test]
fn test_should_reject_corruption_and_invalid_completions_in_recovery() {
    for offset in [0, 8, 16, 20, 24, 28, 32, 512, 544] {
        let mut m = populated(2);
        m.data[offset] ^= 1;
        let mut s =
            Store::<256>::new_unrecovered(m, LAYOUT).unwrap_or_else(|e| panic!("new {e:?}"));
        assert_eq!(
            drive_recover(&mut s),
            Err(AsyncError::Storage(Error::Corrupt))
        );
        assert_eq!(s.load_committed(&mut [0; 256]), Err(Error::NeedsRecovery));
    }
    let mut m = populated(1);
    let a = m.data[..512].to_vec();
    m.data[512..].copy_from_slice(&a);
    let mut s = Store::<256>::new_unrecovered(m, LAYOUT).unwrap_or_else(|e| panic!("new {e:?}"));
    assert_eq!(
        drive_recover(&mut s),
        Err(AsyncError::Storage(Error::Corrupt))
    );
    for at in [2, 5, 8, 11, 14, 17, 20] {
        let mut m = populated(2);
        m.malformed = Some(at);
        let mut s =
            Store::<256>::new_unrecovered(m, LAYOUT).unwrap_or_else(|e| panic!("new {e:?}"));
        assert_eq!(drive_recover(&mut s), Err(AsyncError::InvalidCompletion));
        assert_eq!(s.load_committed(&mut [0; 256]), Err(Error::NeedsRecovery));
    }
}
#[test]
fn test_should_retain_old_or_new_after_every_partial_barrier_flush() {
    for generation in [0, 3] {
        for barrier in 1..=3 {
            for reverse in [false, true] {
                for cut in 0..=1024 {
                    let mut m = populated(generation);
                    m.buffered = true;
                    m.sync_fail = Some(barrier);
                    m.flush_cut = cut;
                    m.flush_reverse = reverse;
                    let mut s = open::<256>(m);
                    let id = s
                        .begin_commit(generation, b"new")
                        .unwrap_or_else(|e| panic!("begin {e:?}"));
                    assert_eq!(
                        drive_commit(&mut s, id),
                        Err(AsyncError::Storage(Error::Io))
                    );
                    assert_eq!(s.load_committed(&mut [0; 256]), Err(Error::NeedsRecovery));
                    let m = s.into_medium().restart();
                    let actual = legacy_value(m.clone());
                    assert!(
                        actual
                            == (generation > 0).then(|| (
                                generation,
                                vec![
                                    u8::try_from(generation)
                                        .unwrap_or_else(|e| panic!("generation {e:?}"));
                                    13
                                ]
                            ))
                            || actual == Some((generation + 1, b"new".to_vec()))
                    );
                    assert_eq!(value(&open::<256>(m)), actual);
                }
            }
        }
    }
}
#[test]
fn test_should_support_legacy_padding_nonpower_units_and_empty_payload() {
    for write_size in [1, 2, 3, 7, 16, 64, 256] {
        let mut m = Nor::blank();
        m.geometry = Geometry {
            capacity: 21504,
            erase_size: 64,
            write_size,
        };
        m.data = vec![255; 21504];
        m.durable = m.data.clone();
        let layout = Layout {
            start: 5376,
            bank_bytes: 5376,
        };
        let mut legacy =
            AtomicSnapshot::<_, 256>::open(m, layout).unwrap_or_else(|e| panic!("legacy {e:?}"));
        legacy
            .commit(0, &[])
            .unwrap_or_else(|e| panic!("legacy commit {e:?}"));
        let mut s = Store::<256>::new_unrecovered(legacy.into_medium(), layout)
            .unwrap_or_else(|e| panic!("new {e:?}"));
        drive_recover(&mut s).unwrap_or_else(|e| panic!("recover {e:?}"));
        assert_eq!(value(&s), Some((1, vec![])));
        let id = s
            .begin_commit(1, &[42; 256])
            .unwrap_or_else(|e| panic!("begin {e:?}"));
        assert!(drive_commit(&mut s, id).is_ok());
        let m = s.into_medium();
        assert!(m.data[..5376].iter().all(|&b| b == 255));
        assert!(m.data[16128..].iter().all(|&b| b == 255));
        let legacy =
            AtomicSnapshot::<_, 256>::open(m, layout).unwrap_or_else(|e| panic!("legacy {e:?}"));
        let mut out = [0; 256];
        assert_eq!(
            legacy.load(&mut out),
            Ok(Some(Snapshot {
                revision: 2,
                length: 256
            }))
        );
        assert_eq!(out, [42; 256]);
    }
    let mut s = open::<0>(Nor::blank());
    assert_eq!(
        s.begin_commit(0, b"x"),
        Err(AsyncError::Storage(Error::Capacity))
    );
    let id = s
        .begin_commit(0, b"")
        .unwrap_or_else(|e| panic!("begin {e:?}"));
    assert!(drive_commit(&mut s, id).is_ok());
    assert_eq!(value(&s), Some((1, vec![])));
}
#[test]
fn test_should_validate_layout_without_io_and_not_expose_debug_payloads() {
    for (geometry, layout) in [
        (
            Geometry {
                erase_size: 0,
                ..GEOMETRY
            },
            LAYOUT,
        ),
        (
            Geometry {
                write_size: 0,
                ..GEOMETRY
            },
            LAYOUT,
        ),
        (
            Geometry {
                write_size: 257,
                ..GEOMETRY
            },
            LAYOUT,
        ),
        (
            GEOMETRY,
            Layout {
                start: u32::MAX - 3,
                ..LAYOUT
            },
        ),
        (GEOMETRY, Layout { start: 1, ..LAYOUT }),
        (
            GEOMETRY,
            Layout {
                bank_bytes: 256,
                ..LAYOUT
            },
        ),
    ] {
        let mut m = Nor::blank();
        m.geometry = geometry;
        let actions = m.actions.clone();
        assert!(matches!(
            Store::<256>::new_unrecovered(m, layout),
            Err(Error::InvalidLayout)
        ));
        assert_eq!(actions.get(), 0);
    }
    let mut s = open::<256>(Nor::blank());
    let id = s
        .begin_commit(0, b"secret-policy")
        .unwrap_or_else(|e| panic!("begin {e:?}"));
    let debug = format!("{s:?}");
    assert!(!debug.contains("secret"));
    assert!(!debug.contains("115, 101"));
    assert!(drive_commit(&mut s, id).is_ok());
    assert_eq!(s.load_committed(&mut [0; 2]), Err(Error::Capacity));
}

#[test]
fn test_should_poison_each_failed_admission_poll_and_replay_failure_until_recovery() {
    let mut probe = open::<256>(populated(3));
    let actions = probe.into_medium().actions;
    let recovery_actions = actions.get();
    let m = populated(3);
    let counter = m.actions.clone();
    probe = open::<256>(m);
    let id = probe
        .begin_commit(3, b"next")
        .unwrap_or_else(|e| panic!("begin {e:?}"));
    assert!(drive_commit(&mut probe, id).is_ok());
    let commit_actions = counter.get() - recovery_actions;
    for at in 0..commit_actions {
        let mut m = populated(3);
        m.action_fail = Some(recovery_actions + at);
        let mut s = open::<256>(m);
        let id = s
            .begin_commit(3, b"next")
            .unwrap_or_else(|e| panic!("begin {e:?}"));
        assert_eq!(
            drive_commit(&mut s, id),
            Err(AsyncError::Storage(Error::Io)),
            "action {at}"
        );
        assert_eq!(s.load_committed(&mut [0; 256]), Err(Error::NeedsRecovery));
        assert_eq!(
            s.poll_commit(id),
            Poll::Ready(Err(AsyncError::Storage(Error::Io)))
        );
        assert_eq!(
            s.begin_commit(3, b"bad"),
            Err(AsyncError::Storage(Error::NeedsRecovery))
        );
        drive_recover(&mut s).unwrap_or_else(|e| panic!("recovery {at}: {e:?}"));
        let actual = value(&s);
        assert!(actual == Some((3, vec![3; 13])) || actual == Some((4, b"next".to_vec())));
    }
    for at in 0..recovery_actions {
        let mut m = populated(3);
        m.action_fail = Some(at);
        let mut s =
            Store::<256>::new_unrecovered(m, LAYOUT).unwrap_or_else(|e| panic!("new {e:?}"));
        assert_eq!(
            drive_recover(&mut s),
            Err(AsyncError::Storage(Error::Io)),
            "recover action {at}"
        );
        assert_eq!(s.load_committed(&mut [0; 256]), Err(Error::NeedsRecovery));
        assert_eq!(
            s.poll_recover(),
            Poll::Ready(Err(AsyncError::Storage(Error::Io)))
        );
        drive_recover(&mut s).unwrap_or_else(|e| panic!("retry {e:?}"));
        assert_eq!(value(&s), Some((3, vec![3; 13])));
    }
}
#[test]
fn test_should_require_exact_final_readback_and_allow_recovery_to_confirm_uncertain_new() {
    // Two initialized banks need 7 reads including selected-bank reload.
    for read in 7..10 {
        let mut m = populated(3);
        m.corrupt_read = Some(read);
        let mut s = open::<256>(m);
        let id = s
            .begin_commit(3, b"next")
            .unwrap_or_else(|e| panic!("begin {e:?}"));
        assert_eq!(
            drive_commit(&mut s, id),
            Err(AsyncError::Storage(Error::Corrupt)),
            "read {read}"
        );
        assert_eq!(s.load_committed(&mut [0; 256]), Err(Error::NeedsRecovery));
        drive_recover(&mut s).unwrap_or_else(|e| panic!("recover {e:?}"));
        assert_eq!(value(&s), Some((4, b"next".to_vec())));
        assert_eq!(legacy_value(s.into_medium()), Some((4, b"next".to_vec())));
    }
    // Final Sync can report failure even when it has persisted the entire image.
    let mut m = populated(3);
    m.buffered = true;
    m.sync_fail = Some(3);
    m.flush_cut = 1024;
    let mut s = open::<256>(m);
    let id = s
        .begin_commit(3, b"next")
        .unwrap_or_else(|e| panic!("begin {e:?}"));
    assert_eq!(
        drive_commit(&mut s, id),
        Err(AsyncError::Storage(Error::Io))
    );
    drive_recover(&mut s).unwrap_or_else(|e| panic!("recover {e:?}"));
    assert_eq!(value(&s), Some((4, b"next".to_vec())));
    assert_eq!(legacy_value(s.into_medium()), Some((4, b"next".to_vec())));
}
#[test]
fn test_should_do_no_io_for_conflicts_wrong_tokens_and_failed_preflight() {
    let m = populated(1);
    let actions = m.actions.clone();
    let mut s = open::<256>(m);
    let before = actions.get();
    assert_eq!(
        s.begin_commit(0, b"bad"),
        Err(AsyncError::Storage(Error::Conflict))
    );
    assert_eq!(
        s.begin_commit(1, &[0; 257]),
        Err(AsyncError::Storage(Error::Capacity))
    );
    assert_eq!(actions.get(), before);
    let first = s
        .begin_commit(1, b"new")
        .unwrap_or_else(|e| panic!("begin {e:?}"));
    assert_eq!(actions.get(), before);
    assert!(drive_commit(&mut s, first).is_ok());
    let second = s
        .begin_commit(2, b"newer")
        .unwrap_or_else(|e| panic!("begin {e:?}"));
    let before = actions.get();
    assert_eq!(
        s.poll_commit(first),
        Poll::Ready(Err(AsyncError::InvalidOperation))
    );
    assert_eq!(
        s.poll_recover(),
        Poll::Ready(Err(AsyncError::InvalidOperation))
    );
    assert_eq!(actions.get(), before);
    assert!(drive_commit(&mut s, second).is_ok());
}
#[test]
fn test_should_chunk_large_payload_crc_readback_and_recovery_and_retry_busy_admission() {
    let mut m = Nor::blank();
    m.geometry.capacity = 2048;
    m.data = vec![255; 2048];
    m.durable = m.data.clone();
    m.busy_once = Some(());
    let actions = m.actions.clone();
    let layout = Layout {
        start: 0,
        bank_bytes: 1024,
    };
    let mut s = Store::<600>::new_unrecovered(m, layout).unwrap_or_else(|e| panic!("new {e:?}"));
    drive_recover(&mut s).unwrap_or_else(|e| panic!("recover {e:?}"));
    let id = s
        .begin_commit(0, &[77; 600])
        .unwrap_or_else(|e| panic!("begin {e:?}"));
    loop {
        let before = actions.get();
        let result = s.poll_commit(id);
        assert!(actions.get() - before <= 1);
        if let Poll::Ready(r) = result {
            assert!(r.is_ok());
            break;
        }
    }
    drive_recover(&mut s).unwrap_or_else(|e| panic!("recover {e:?}"));
    assert_eq!(value(&s), Some((1, vec![77; 600])));
}
#[test]
fn test_should_reject_crc_valid_revision_and_length_errors_without_io_on_exhaustion() {
    let headers = [
        [
            68, 83, 78, 65, 80, 48, 48, 49, 255, 255, 255, 255, 255, 255, 255, 255, 13, 0, 0, 0,
            117, 101, 50, 122, 0, 0, 0, 0, 148, 219, 36, 212,
        ],
        [
            68, 83, 78, 65, 80, 48, 48, 49, 0, 0, 0, 0, 0, 0, 0, 0, 13, 0, 0, 0, 117, 101, 50, 122,
            0, 0, 0, 0, 64, 23, 145, 42,
        ],
        [
            68, 83, 78, 65, 80, 48, 48, 49, 1, 0, 0, 0, 0, 0, 0, 0, 65, 0, 0, 0, 117, 101, 50, 122,
            0, 0, 0, 0, 0, 7, 208, 50,
        ],
    ];
    let mut m = populated(1);
    m.data[..32].copy_from_slice(&headers[0]);
    let actions = m.actions.clone();
    let mut s = open::<256>(m);
    let before = actions.get();
    assert_eq!(
        s.begin_commit(u64::MAX, b"bad"),
        Err(AsyncError::Storage(Error::RevisionExhausted))
    );
    assert_eq!(actions.get(), before);
    for header in &headers[1..] {
        let mut m = populated(1);
        m.data[..32].copy_from_slice(header);
        let mut s = Store::<64>::new_unrecovered(m, LAYOUT).unwrap_or_else(|e| panic!("new {e:?}"));
        assert_eq!(
            drive_recover(&mut s),
            Err(AsyncError::Storage(Error::Corrupt))
        );
    }
}
#[test]
fn test_should_report_host_engine_layout_without_medium_storage() {
    println!(
        "host engine M=(): N128={} N256={} N600={} legacy N128={} N256={}",
        core::mem::size_of::<IncrementalSnapshot<(), 128>>(),
        core::mem::size_of::<IncrementalSnapshot<(), 256>>(),
        core::mem::size_of::<IncrementalSnapshot<(), 600>>(),
        core::mem::size_of::<AtomicSnapshot<(), 128>>(),
        core::mem::size_of::<AtomicSnapshot<(), 256>>()
    );
}

#[test]
fn test_should_report_busy_through_commit_and_recovery_without_io_or_consuming_results() {
    let m = Nor::blank();
    let actions = m.actions.clone();
    let mut s = Store::<128>::new_unrecovered(m, LAYOUT).unwrap_or_else(|e| panic!("new {e:?}"));
    assert!(!s.is_busy());
    assert_eq!(s.load_committed(&mut [0; 128]), Err(Error::NeedsRecovery));
    assert_eq!(actions.get(), 0);
    s.begin_recover()
        .unwrap_or_else(|e| panic!("begin recover {e:?}"));
    loop {
        let before = actions.get();
        assert!(s.is_busy());
        assert_eq!(actions.get(), before, "busy query must not perform I/O");
        match s.poll_recover() {
            Poll::Pending => assert!(s.is_busy()),
            Poll::Ready(result) => {
                assert_eq!(result, Ok(()));
                break;
            }
        }
    }
    let before = actions.get();
    assert!(!s.is_busy());
    assert_eq!(s.poll_recover(), Poll::Ready(Ok(())));
    assert_eq!(actions.get(), before);
    let id = s
        .begin_commit(0, b"new")
        .unwrap_or_else(|e| panic!("begin {e:?}"));
    loop {
        let before = actions.get();
        assert!(s.is_busy(), "accepted work remains busy between commands");
        assert_eq!(actions.get(), before);
        match s.poll_commit(id) {
            Poll::Pending => assert!(s.is_busy()),
            Poll::Ready(result) => {
                assert!(result.is_ok());
                break;
            }
        }
    }
    let before = actions.get();
    assert!(!s.is_busy());
    assert!(matches!(s.poll_commit(id), Poll::Ready(Ok(_))));
    assert_eq!(
        actions.get(),
        before,
        "query must preserve the terminal receipt"
    );
    assert_eq!(value(&s), Some((1, b"new".to_vec())));
    s.begin_recover()
        .unwrap_or_else(|e| panic!("begin recover {e:?}"));
    assert!(s.is_busy());
    assert_eq!(s.load_committed(&mut [0; 128]), Err(Error::NeedsRecovery));
    loop {
        assert!(s.is_busy());
        if let Poll::Ready(result) = s.poll_recover() {
            assert_eq!(result, Ok(()));
            break;
        }
    }
    assert!(!s.is_busy());
    assert_eq!(value(&s), Some((1, b"new".to_vec())));
}

#[test]
fn test_should_report_not_busy_for_terminal_uncertainty_while_reads_still_require_recovery() {
    let mut m = Nor::blank();
    m.sync_fail = Some(0);
    let actions = m.actions.clone();
    let mut s = Store::<128>::new_unrecovered(m, LAYOUT).unwrap_or_else(|e| panic!("new {e:?}"));
    assert!(!s.is_busy());
    s.begin_recover()
        .unwrap_or_else(|e| panic!("begin recover {e:?}"));
    loop {
        assert!(s.is_busy());
        if let Poll::Ready(result) = s.poll_recover() {
            assert_eq!(result, Err(AsyncError::Storage(Error::Io)));
            break;
        }
    }
    let before = actions.get();
    assert!(!s.is_busy());
    assert_eq!(s.load_committed(&mut [0; 128]), Err(Error::NeedsRecovery));
    assert_eq!(
        s.poll_recover(),
        Poll::Ready(Err(AsyncError::Storage(Error::Io)))
    );
    assert_eq!(actions.get(), before);
}

// Reject exposing the pending slot, blank slot contents, or any image while
// recovery is required. The old copying API supplies an independent comparison.
fn inspected<const N: usize>(s: &Store<N>) -> Result<(Option<Snapshot>, Vec<u8>), Error> {
    s.inspect_committed(|snapshot, bytes| (snapshot, bytes.to_vec()))
}
#[test]
fn test_should_inspect_exact_confirmed_bytes_without_io_across_blank_pending_and_completion() {
    let medium = Nor::blank();
    let actions = medium.actions.clone();
    let mut s = open::<256>(medium);
    let before = actions.get();
    assert_eq!(inspected(&s), Ok((None, vec![])));
    assert_eq!(actions.get(), before);
    let id = s
        .begin_commit(0, b"first")
        .unwrap_or_else(|e| panic!("begin {e:?}"));
    while s.poll_commit(id).is_pending() {
        assert_eq!(inspected(&s), Ok((None, vec![])));
    }
    assert_eq!(
        inspected(&s),
        Ok((
            Some(Snapshot {
                revision: 1,
                length: 5
            }),
            b"first".to_vec()
        ))
    );
    let id = s
        .begin_commit(1, &[7; 256])
        .unwrap_or_else(|e| panic!("begin {e:?}"));
    loop {
        let before = actions.get();
        let mut copied = [0; 256];
        let snapshot = s
            .load_committed(&mut copied)
            .unwrap_or_else(|e| panic!("copy {e:?}"));
        assert_eq!(inspected(&s), Ok((snapshot, b"first".to_vec())));
        assert_eq!(&copied[..5], b"first");
        assert_eq!(actions.get(), before);
        if s.poll_commit(id).is_ready() {
            break;
        }
    }
    assert_eq!(
        inspected(&s),
        Ok((
            Some(Snapshot {
                revision: 2,
                length: 256
            }),
            vec![7; 256]
        ))
    );
}
#[test]
fn test_should_never_call_inspector_before_recovery_or_after_uncertain_commit() {
    let mut m = populated(1);
    m.sync_fail = Some(1); // Recovery sync succeeds, commit retire sync fails.
    let mut s = Store::<256>::new_unrecovered(m, LAYOUT).unwrap_or_else(|e| panic!("new {e:?}"));
    let calls = Cell::new(0);
    let blocked = |_: Option<Snapshot>, _: &[u8]| {
        calls.set(calls.get() + 1);
    };
    assert_eq!(s.inspect_committed(blocked), Err(Error::NeedsRecovery));
    s.begin_recover()
        .unwrap_or_else(|e| panic!("recover {e:?}"));
    loop {
        assert_eq!(s.inspect_committed(blocked), Err(Error::NeedsRecovery));
        if s.poll_recover().is_ready() {
            break;
        }
    }
    assert_eq!(calls.get(), 0);
    let id = s
        .begin_commit(1, b"next")
        .unwrap_or_else(|e| panic!("begin {e:?}"));
    assert!(drive_commit(&mut s, id).is_err());
    assert_eq!(s.load_committed(&mut [0; 256]), Err(Error::NeedsRecovery));
    assert_eq!(s.inspect_committed(blocked), Err(Error::NeedsRecovery));
    assert_eq!(calls.get(), 0);
    s.begin_recover()
        .unwrap_or_else(|e| panic!("recover {e:?}"));
    loop {
        assert_eq!(s.inspect_committed(blocked), Err(Error::NeedsRecovery));
        if s.poll_recover().is_ready() {
            break;
        }
    }
    assert_eq!(calls.get(), 0);
    let mut copied = [0; 256];
    let snapshot = s
        .load_committed(&mut copied)
        .unwrap_or_else(|e| panic!("copy {e:?}"));
    assert_eq!(inspected(&s), Ok((snapshot, vec![1; 13])));
    assert_eq!(&copied[..13], &[1; 13]);
}

#[test]
fn test_should_encode_directly_with_identical_durable_image_and_retained_receipt() {
    use device_storage::incremental::CommitEncodeError;
    let media = populated(1);
    let mut ordinary = open::<256>(media.clone());
    let mut encoded = open::<256>(media);
    let prior = value(&encoded);
    let a = ordinary
        .begin_commit(1, b"encoded-snapshot")
        .unwrap_or_else(|error| panic!("test setup/progress {error:?}"));
    let b = encoded
        .begin_commit_encoded(1, |out| {
            out[..16].copy_from_slice(b"encoded-snapshot");
            Ok::<_, &'static str>(16)
        })
        .unwrap_or_else(|error| panic!("test setup/progress {error:?}"));
    assert_eq!(value(&encoded), prior);
    let receipt = drive_commit(&mut encoded, b)
        .unwrap_or_else(|error| panic!("test setup/progress {error:?}"));
    assert_eq!(
        drive_commit(&mut ordinary, a)
            .unwrap_or_else(|error| panic!("test setup/progress {error:?}"))
            .snapshot,
        receipt.snapshot
    );
    assert_eq!(
        encoded.begin_commit_encoded(2, |out| {
            out.fill(0x55);
            Err::<usize, _>("encoding-failed")
        }),
        Err(CommitEncodeError::Encoding("encoding-failed"))
    );
    assert_eq!(
        encoded.begin_commit_encoded(2, |out| {
            out.fill(0xaa);
            Ok::<_, &'static str>(257)
        }),
        Err(CommitEncodeError::Storage(AsyncError::Storage(
            Error::Capacity
        )))
    );
    assert_eq!(encoded.poll_commit(b), Poll::Ready(Ok(receipt)));
    assert_eq!(value(&encoded), Some((2, b"encoded-snapshot".to_vec())));
    assert_eq!(encoded.into_medium().data, ordinary.into_medium().data);
}
#[test]
fn test_should_encode_only_after_preflight_and_perform_no_admission_io() {
    use device_storage::incremental::CommitEncodeError;
    let media = Nor::blank();
    let actions = media.actions.clone();
    let mut store = Store::<256>::new_unrecovered(media, LAYOUT)
        .unwrap_or_else(|error| panic!("test setup/progress {error:?}"));
    let called = Cell::new(false);
    let encode = |out: &mut [u8; 256]| {
        called.set(true);
        out[0] = 9;
        Ok::<_, ()>(1)
    };
    assert_eq!(
        store.begin_commit_encoded(0, encode),
        Err(CommitEncodeError::Storage(AsyncError::Storage(
            Error::NeedsRecovery
        )))
    );
    assert!(!called.get());
    drive_recover(&mut store).unwrap_or_else(|error| panic!("test setup/progress {error:?}"));
    let before = actions.get();
    assert_eq!(
        store.begin_commit_encoded(1, encode),
        Err(CommitEncodeError::Storage(AsyncError::Storage(
            Error::Conflict
        )))
    );
    assert!(!called.get());
    let id = store
        .begin_commit_encoded(0, encode)
        .unwrap_or_else(|error| panic!("test setup/progress {error:?}"));
    assert!(called.get());
    called.set(false);
    assert_eq!(
        store.begin_commit_encoded(0, encode),
        Err(CommitEncodeError::Storage(AsyncError::Busy))
    );
    assert!(!called.get());
    assert_eq!(actions.get(), before);
    assert!(drive_commit(&mut store, id).is_ok());
}
#[test]
fn test_should_encode_and_recover_old_or_new_at_every_mutation_byte() {
    for reverse in [false, true] {
        let media = populated(1);
        let mut full = open::<256>(media.clone());
        let id = full
            .begin_commit_encoded(1, |out| {
                out[..5].copy_from_slice(b"newer");
                Ok::<_, ()>(5)
            })
            .unwrap_or_else(|error| panic!("test setup/progress {error:?}"));
        drive_commit(&mut full, id).unwrap_or_else(|error| panic!("test setup/progress {error:?}"));
        let effects = full.into_medium().effects;
        for cut in 0..=effects {
            let mut nor = media.clone();
            nor.reverse = reverse;
            nor.cut = Some(cut);
            let mut store = open::<256>(nor);
            let id = store
                .begin_commit_encoded(1, |out| {
                    out[..5].copy_from_slice(b"newer");
                    Ok::<_, ()>(5)
                })
                .unwrap_or_else(|error| panic!("test setup/progress {error:?}"));
            let result = drive_commit(&mut store, id);
            let recovered = open::<256>(store.into_medium().restart());
            let actual = value(&recovered);
            let old = Some((1, vec![1; 13]));
            let new = Some((2, b"newer".to_vec()));
            assert!(
                actual == old || actual == new,
                "cut {cut} reverse {reverse}: {actual:?}"
            );
            if result.is_ok() {
                assert_eq!(actual, new);
            }
        }
    }
}

// Offline invariants intentionally fail immediately; these helpers are not runtime code.
#[allow(clippy::unwrap_used)]
mod borrowed_workspace {
    use super::*;

    #[test]
    fn test_should_reuse_overwritten_transfer_page_across_two_snapshot_jobs() {
        let mut a = IncrementalSnapshot::<_, 64>::new_unrecovered(Nor::blank(), LAYOUT).unwrap();
        let mut b = IncrementalSnapshot::<_, 64>::new_unrecovered(Nor::blank(), LAYOUT).unwrap();
        let mut scratch = [0; 256];
        a.begin_recover().unwrap();
        b.begin_recover().unwrap();
        for s in [&mut a, &mut b] {
            loop {
                scratch.fill(0x39);
                if let Poll::Ready(result) = s.poll_recover_with_scratch(&mut scratch) {
                    result.unwrap();
                    break;
                }
            }
        }
        let ta = a.begin_commit(0, &[11; 64]).unwrap();
        let tb = b.begin_commit(0, &[29; 63]).unwrap();
        let mut done = [false; 2];
        for _ in 0..10000 {
            for (i, (s, token)) in [(&mut a, ta), (&mut b, tb)].into_iter().enumerate() {
                scratch.fill(0xa9);
                if let Poll::Ready(result) = s.poll_commit_with_scratch(token, &mut scratch) {
                    result.unwrap();
                    done[i] = true;
                }
            }
            if done == [true; 2] {
                break;
            }
        }
        assert_eq!(done, [true; 2]);
        for (s, value, length) in [(&a, 11, 64), (&b, 29, 63)] {
            let mut bytes = [0; 64];
            assert_eq!(
                s.load_committed(&mut bytes).unwrap().unwrap().length,
                length
            );
            assert_eq!(&bytes[..length], &vec![value; length]);
        }
        for (s, value, length) in [(a, 11, 64), (b, 29, 63)] {
            let mut restored =
                IncrementalSnapshot::<_, 64>::new_unrecovered(s.into_medium().restart(), LAYOUT)
                    .unwrap();
            restored.begin_recover().unwrap();
            loop {
                scratch.fill(0x17);
                if let Poll::Ready(result) = restored.poll_recover_with_scratch(&mut scratch) {
                    result.unwrap();
                    break;
                }
            }
            let mut bytes = [0; 64];
            assert_eq!(
                restored.load_committed(&mut bytes).unwrap().unwrap().length,
                length
            );
            assert_eq!(&bytes[..length], &vec![value; length]);
        }
    }

    #[test]
    fn test_should_drop_only_redundant_image_for_exclusive_metadata_writer() {
        let mut original =
            IncrementalSnapshot::<_, 64>::new_unrecovered(Nor::blank(), LAYOUT).unwrap();
        original.begin_recover().unwrap();
        while original.poll_recover().is_pending() {}
        let t = original.begin_commit(0, &[19; 64]).unwrap();
        while original.poll_commit(t).is_pending() {}
        let mut exclusive = original.into_exclusive().unwrap();
        let mut out = [0; 64];
        assert_eq!(
            exclusive
                .load_committed(&mut out)
                .unwrap()
                .unwrap()
                .revision,
            1
        );
        assert_eq!(out, [19; 64]);
        let fail = exclusive.begin_commit_encoded::<()>(1, |bytes| {
            bytes.fill(99);
            Err(())
        });
        assert!(fail.is_err());
        exclusive.load_committed(&mut out).unwrap();
        assert_eq!(out, [19; 64]);
        let t = exclusive.begin_commit(1, &[47; 64]).unwrap();
        // Only the history owner's confirmed decoded directory is readable during
        // a commit. This opt-in internal engine must not expose overwritten bytes.
        assert_eq!(
            exclusive.load_committed(&mut out),
            Err(Error::NeedsRecovery)
        );
        loop {
            if let Poll::Ready(r) = exclusive.poll_commit(t) {
                r.unwrap();
                break;
            }
        }
        assert_eq!(
            exclusive
                .load_committed(&mut out)
                .unwrap()
                .unwrap()
                .revision,
            2
        );
        assert_eq!(out, [47; 64]);
        let mut restored = IncrementalSnapshot::<_, 64>::new_unrecovered(
            exclusive.into_medium().restart(),
            LAYOUT,
        )
        .unwrap();
        restored.begin_recover().unwrap();
        loop {
            if let Poll::Ready(r) = restored.poll_recover() {
                r.unwrap();
                break;
            }
        }
        assert_eq!(
            restored.load_committed(&mut out).unwrap().unwrap().revision,
            2
        );
        assert_eq!(out, [47; 64]);
    }

    #[test]
    fn test_should_stage_default_expected_poll_until_completion_matches() {
        let mut medium = Nor::blank();
        medium
            .begin(MediumOp::Read {
                offset: 0,
                length: 4,
            })
            .unwrap();
        let mut out = [0x39; 256];
        loop {
            match medium.poll_expected(Completion::Synced, &mut out) {
                Poll::Pending => assert_eq!(out, [0x39; 256]),
                Poll::Ready(result) => {
                    assert_eq!(result, Err(Error::Corrupt));
                    break;
                }
            }
        }
        assert_eq!(out, [0x39; 256]);
    }

    #[test]
    fn test_should_return_active_engine_when_exclusive_conversion_is_busy() {
        let mut engine =
            IncrementalSnapshot::<_, 64>::new_unrecovered(Nor::blank(), LAYOUT).unwrap();
        engine.begin_recover().unwrap();
        let (mut engine, error) = engine.into_exclusive().unwrap_err();
        assert_eq!(error, AsyncError::Busy);
        loop {
            if let Poll::Ready(result) = engine.poll_recover() {
                result.unwrap();
                break;
            }
        }
        let token = engine.begin_commit(0, &[73; 64]).unwrap();
        loop {
            if let Poll::Ready(result) = engine.poll_commit(token) {
                result.unwrap();
                break;
            }
        }
        let mut out = [0; 64];
        engine.load_committed(&mut out).unwrap();
        assert_eq!(out, [73; 64]);
    }
}
