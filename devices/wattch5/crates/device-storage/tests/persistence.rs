//! Portable persistence behavior and byte-granularity power-cut regression.
use device_storage::persistence::{
    AtomicSnapshot, Error, Geometry, Layout, Medium, Snapshot, SnapshotStore,
};

#[derive(Clone, Debug)]
struct Nor {
    data: Vec<u8>,
    geometry: Geometry,
    cut: Option<usize>,
    effects: usize,
    sync_cut: Option<usize>,
    syncs: usize,
    read_fail: bool,
    read_cut: Option<usize>,
    reads: usize,
    reverse: bool,
}
impl Nor {
    fn blank() -> Self {
        Self {
            data: vec![255; 512],
            geometry: Geometry {
                capacity: 512,
                erase_size: 64,
                write_size: 4,
            },
            cut: None,
            effects: 0,
            sync_cut: None,
            syncs: 0,
            read_fail: false,
            read_cut: None,
            reads: 0,
            reverse: false,
        }
    }
    fn mutate(&mut self) -> Result<(), Error> {
        if self.cut == Some(self.effects) {
            return Err(Error::Io);
        }
        self.effects += 1;
        Ok(())
    }
    fn healed(mut self) -> Self {
        self.cut = None;
        self.sync_cut = None;
        self.read_fail = false;
        self.read_cut = None;
        self.reads = 0;
        self.effects = 0;
        self.syncs = 0;
        self
    }
}
impl Medium for Nor {
    fn geometry(&self) -> Geometry {
        self.geometry
    }
    fn read(&mut self, offset: u32, out: &mut [u8]) -> Result<(), Error> {
        let read = self.reads;
        self.reads += 1;
        if self.read_fail || self.read_cut == Some(read) {
            return Err(Error::Io);
        }
        let start = usize::try_from(offset)
            .unwrap_or_else(|error| panic!("test fixture failed: {error:?}"));
        out.copy_from_slice(&self.data[start..start + out.len()]);
        Ok(())
    }
    fn erase(&mut self, offset: u32, length: u32) -> Result<(), Error> {
        assert_eq!(offset % self.geometry.erase_size, 0);
        assert_eq!(length % self.geometry.erase_size, 0);
        for relative in 0..length {
            let index = offset
                + if self.reverse {
                    length - 1 - relative
                } else {
                    relative
                };
            self.mutate()?;
            self.data[usize::try_from(index)
                .unwrap_or_else(|error| panic!("test fixture failed: {error:?}"))] = 255;
        }
        Ok(())
    }
    fn program(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Error> {
        assert_eq!(offset % self.geometry.write_size, 0);
        assert_eq!(
            bytes.len()
                % usize::try_from(self.geometry.write_size)
                    .unwrap_or_else(|error| panic!("test fixture failed: {error:?}")),
            0
        );
        for relative in 0..bytes.len() {
            let index = if self.reverse {
                bytes.len() - 1 - relative
            } else {
                relative
            };
            let byte = bytes[index];
            self.mutate()?;
            let at = usize::try_from(offset)
                .unwrap_or_else(|error| panic!("test fixture failed: {error:?}"))
                + index;
            assert_eq!(self.data[at] & byte, byte, "NOR 0-to-1 transition");
            self.data[at] = byte;
        }
        Ok(())
    }
    fn sync(&mut self) -> Result<(), Error> {
        let at = self.syncs;
        self.syncs += 1;
        if self.sync_cut == Some(at) {
            Err(Error::Io)
        } else {
            Ok(())
        }
    }
}
const LAYOUT: Layout = Layout {
    start: 0,
    bank_bytes: 256,
};
type Store = AtomicSnapshot<Nor, 64>;
fn loaded(store: &Store) -> Option<(u64, Vec<u8>)> {
    let mut out = [0; 64];
    store
        .load(&mut out)
        .unwrap_or_else(|error| panic!("test fixture failed: {error:?}"))
        .map(|s| (s.revision, out[..s.length].to_vec()))
}
fn populated(generations: u64) -> Nor {
    let mut store = Store::open(Nor::blank(), LAYOUT)
        .unwrap_or_else(|error| panic!("test fixture failed: {error:?}"));
    for revision in 1..=generations {
        store
            .commit(
                revision - 1,
                &[u8::try_from(revision)
                    .unwrap_or_else(|error| panic!("test fixture failed: {error:?}"));
                    13],
            )
            .unwrap_or_else(|error| panic!("test fixture failed: {error:?}"));
    }
    store.into_medium().healed()
}
#[test]
fn test_should_initialize_explicitly_and_reopen_whole_snapshots() {
    let mut store = Store::open(Nor::blank(), LAYOUT)
        .unwrap_or_else(|error| panic!("test fixture failed: {error:?}"));
    assert_eq!(loaded(&store), None);
    assert_eq!(
        store.commit(0, b"policy"),
        Ok(Snapshot {
            revision: 1,
            length: 6
        })
    );
    assert_eq!(
        loaded(
            &Store::open(store.into_medium(), LAYOUT)
                .unwrap_or_else(|error| panic!("test fixture failed: {error:?}"))
        ),
        Some((1, b"policy".to_vec()))
    );
}
#[test]
fn test_should_recover_old_or_complete_new_at_every_mutation_byte() {
    for (generation, reverse) in
        (0..=5).flat_map(|generation| [false, true].map(|reverse| (generation, reverse)))
    {
        let mut image = populated(generation);
        image.reverse = reverse;
        let old = (generation > 0).then(|| {
            (
                generation,
                vec![
                    u8::try_from(generation)
                        .unwrap_or_else(|error| panic!("test fixture failed: {error:?}"));
                    13
                ],
            )
        });
        let next = Some((generation + 1, b"replacement-longer".to_vec()));
        let mut successful = Store::open(image.clone(), LAYOUT)
            .unwrap_or_else(|error| panic!("test fixture failed: {error:?}"));
        successful
            .commit(generation, b"replacement-longer")
            .unwrap_or_else(|error| panic!("test fixture failed: {error:?}"));
        let effects = successful.into_medium().effects;
        for cut in 0..=effects {
            let mut medium = image.clone();
            medium.cut = Some(cut);
            let mut store = Store::open(medium, LAYOUT)
                .unwrap_or_else(|error| panic!("test fixture failed: {error:?}"));
            let result = store.commit(generation, b"replacement-longer");
            if result.is_err() {
                assert_eq!(store.load(&mut [0; 64]), Err(Error::NeedsRecovery));
                assert_eq!(store.commit(generation, b"x"), Err(Error::NeedsRecovery));
            }
            let mut reopened = Store::open(store.into_medium().healed(), LAYOUT)
                .unwrap_or_else(|error| panic!("test fixture failed: {error:?}"));
            let actual = loaded(&reopened);
            if result.is_ok() {
                assert_eq!(actual, next, "acknowledged cut {cut}");
            }
            assert!(
                actual == old || actual == next,
                "generation {generation} cut {cut}: {actual:?}"
            );
            let revision = actual.map_or(0, |value| value.0);
            reopened
                .commit(revision, b"after-recovery")
                .unwrap_or_else(|error| panic!("test fixture failed: {error:?}"));
            assert_eq!(
                loaded(&reopened),
                Some((revision + 1, b"after-recovery".to_vec()))
            );
        }
    }
}
#[test]
fn test_should_poison_every_failed_barrier_until_recovery() {
    for sync_cut in 1..4 {
        let mut image = populated(3);
        image.sync_cut = Some(sync_cut);
        let mut store = Store::open(image, LAYOUT)
            .unwrap_or_else(|error| panic!("test fixture failed: {error:?}"));
        assert_eq!(store.commit(3, b"next"), Err(Error::Io));
        assert_eq!(store.load(&mut [0; 64]), Err(Error::NeedsRecovery));
        store
            .recover()
            .unwrap_or_else(|error| panic!("test fixture failed: {error:?}"));
        let actual = loaded(&store);
        assert!(actual == Some((3, vec![3; 13])) || actual == Some((4, b"next".to_vec())));
    }
}
#[test]
fn test_should_validate_before_any_mutation() {
    let mut store = Store::open(Nor::blank(), LAYOUT)
        .unwrap_or_else(|error| panic!("test fixture failed: {error:?}"));
    assert_eq!(store.commit(1, b"bad"), Err(Error::Conflict));
    assert_eq!(store.commit(0, &[1; 65]), Err(Error::Capacity));
    assert_eq!(store.into_medium().effects, 0);
    for (geometry, layout) in [
        (
            Geometry {
                capacity: 512,
                erase_size: 0,
                write_size: 4,
            },
            LAYOUT,
        ),
        (
            Geometry {
                capacity: 512,
                erase_size: 64,
                write_size: 0,
            },
            LAYOUT,
        ),
        (
            Geometry {
                capacity: 512,
                erase_size: 64,
                write_size: 4,
            },
            Layout {
                start: 1,
                bank_bytes: 256,
            },
        ),
        (
            Geometry {
                capacity: 512,
                erase_size: 64,
                write_size: 4,
            },
            Layout {
                start: u32::MAX - 3,
                bank_bytes: 256,
            },
        ),
        (
            Geometry {
                capacity: 512,
                erase_size: 64,
                write_size: 4,
            },
            Layout {
                start: 0,
                bank_bytes: 320,
            },
        ),
    ] {
        let mut medium = Nor::blank();
        medium.geometry = geometry;
        assert!(matches!(
            Store::open(medium, layout),
            Err(Error::InvalidLayout)
        ));
    }
}
#[test]
fn test_should_reject_committed_data_corruption_and_equal_revisions() {
    let mut image = populated(2);
    image.data[32] ^= 1;
    assert!(matches!(Store::open(image, LAYOUT), Err(Error::Corrupt)));
    let mut image = populated(1);
    let copy = image.data[..256].to_vec();
    image.data[256..].copy_from_slice(&copy);
    assert!(matches!(Store::open(image, LAYOUT), Err(Error::Corrupt)));
}
#[test]
fn test_should_preserve_cache_on_short_output_and_fail_closed_on_read_errors() {
    let store = Store::open(populated(1), LAYOUT)
        .unwrap_or_else(|error| panic!("test fixture failed: {error:?}"));
    assert_eq!(store.load(&mut [0; 12]), Err(Error::Capacity));
    assert_eq!(loaded(&store), Some((1, vec![1; 13])));
    let mut image = store.into_medium();
    image.read_fail = true;
    assert!(matches!(Store::open(image, LAYOUT), Err(Error::Io)));
}

#[derive(Debug)]
struct Buffered {
    live: Nor,
    durable: Vec<u8>,
    fail_sync: Option<usize>,
    syncs: usize,
    flush_cut: usize,
    flush_reverse: bool,
}
impl Medium for Buffered {
    fn geometry(&self) -> Geometry {
        self.live.geometry()
    }
    fn read(&mut self, offset: u32, out: &mut [u8]) -> Result<(), Error> {
        self.live.read(offset, out)
    }
    fn erase(&mut self, offset: u32, length: u32) -> Result<(), Error> {
        self.live.erase(offset, length)
    }
    fn program(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Error> {
        self.live.program(offset, bytes)
    }
    fn sync(&mut self) -> Result<(), Error> {
        let at = self.syncs;
        self.syncs += 1;
        if self.fail_sync == Some(at) {
            let range = if self.flush_reverse {
                self.durable.len() - self.flush_cut..self.durable.len()
            } else {
                0..self.flush_cut
            };
            self.durable[range.clone()].copy_from_slice(&self.live.data[range]);
            return Err(Error::Io);
        }
        self.durable.clone_from(&self.live.data);
        Ok(())
    }
}
#[test]
fn test_should_make_recovered_read_visible_state_durable() {
    let image = populated(1);
    let medium = Buffered {
        durable: image.data.clone(),
        live: image,
        fail_sync: Some(3),
        syncs: 0,
        flush_cut: 0,
        flush_reverse: false,
    };
    let mut store = AtomicSnapshot::<_, 64>::open(medium, LAYOUT)
        .unwrap_or_else(|error| panic!("test fixture failed: {error:?}"));
    assert_eq!(store.commit(1, b"new"), Err(Error::Io));
    store
        .recover()
        .unwrap_or_else(|error| panic!("test fixture failed: {error:?}"));
    let mut out = [0; 64];
    let recovered = store
        .load(&mut out)
        .unwrap_or_else(|error| panic!("test fixture failed: {error:?}"))
        .unwrap_or_else(|| panic!("expected snapshot"));
    let expected = out[..recovered.length].to_vec();
    let medium = store.into_medium();
    let mut restarted = medium.live.healed();
    restarted.data = medium.durable;
    assert_eq!(
        loaded(
            &Store::open(restarted, LAYOUT)
                .unwrap_or_else(|error| panic!("test fixture failed: {error:?}"))
        ),
        Some((recovered.revision, expected))
    );
}

#[test]
fn test_should_reject_latest_payload_or_metadata_corruption_without_mutation() {
    for offset in [32, 288, 256, 268, 272, 276, 280, 284] {
        let mut image = populated(2);
        image.data[offset] ^= 1;
        assert!(
            matches!(Store::open(image, LAYOUT), Err(Error::Corrupt)),
            "corruption offset {offset}"
        );
    }
}

#[test]
fn test_should_require_recovery_after_every_failed_recovery_read_or_sync() {
    for read_cut in 0..6 {
        let mut image = populated(2);
        image.read_cut = Some(read_cut);
        assert!(matches!(Store::open(image, LAYOUT), Err(Error::Io)));
    }
    let mut image = populated(2);
    image.read_cut = Some(6);
    let mut store = Store::open(image, LAYOUT).unwrap_or_else(|error| panic!("fixture: {error:?}"));
    assert_eq!(store.recover(), Err(Error::Io));
    assert_eq!(store.load(&mut [0; 64]), Err(Error::NeedsRecovery));
    assert_eq!(store.commit(2, b"bad"), Err(Error::NeedsRecovery));
    store
        .recover()
        .unwrap_or_else(|error| panic!("fixture: {error:?}"));
    assert_eq!(loaded(&store), Some((2, vec![2; 13])));
    let mut image = populated(2);
    image.sync_cut = Some(1);
    let mut store = Store::open(image, LAYOUT).unwrap_or_else(|error| panic!("fixture: {error:?}"));
    assert_eq!(store.recover(), Err(Error::Io));
    assert_eq!(store.load(&mut [0; 64]), Err(Error::NeedsRecovery));
    store
        .recover()
        .unwrap_or_else(|error| panic!("fixture: {error:?}"));
    assert_eq!(loaded(&store), Some((2, vec![2; 13])));
}

#[test]
fn test_should_support_padded_program_units_offsets_empty_and_full_payloads() {
    for write_size in [1, 2, 3, 7, 16, 64, 256] {
        let mut medium = Nor::blank();
        medium.geometry = Geometry {
            capacity: 21_504,
            erase_size: 64,
            write_size,
        };
        medium.data = vec![255; 21_504];
        let layout = Layout {
            start: 5376,
            bank_bytes: 5376,
        };
        let mut store =
            Store::open(medium, layout).unwrap_or_else(|error| panic!("fixture: {error:?}"));
        store
            .commit(0, &[])
            .unwrap_or_else(|error| panic!("fixture: {error:?}"));
        assert_eq!(loaded(&store), Some((1, vec![])));
        store
            .commit(1, &[42; 64])
            .unwrap_or_else(|error| panic!("fixture: {error:?}"));
        let store = Store::open(store.into_medium(), layout)
            .unwrap_or_else(|error| panic!("fixture: {error:?}"));
        assert_eq!(loaded(&store), Some((2, vec![42; 64])));
        let medium = store.into_medium();
        assert!(medium.data[..5376].iter().all(|&byte| byte == 255));
        assert!(medium.data[16_128..].iter().all(|&byte| byte == 255));
    }
}

#[test]
fn test_should_ignore_uncommitted_preparation_and_redact_debug_payload() {
    let mut image = populated(1);
    image.data[252..256].fill(255);
    assert_eq!(
        loaded(&Store::open(image, LAYOUT).unwrap_or_else(|error| panic!("fixture: {error:?}"))),
        None
    );
    let mut store =
        Store::open(Nor::blank(), LAYOUT).unwrap_or_else(|error| panic!("fixture: {error:?}"));
    store
        .commit(0, b"secret-policy")
        .unwrap_or_else(|error| panic!("fixture: {error:?}"));
    let debug = format!("{store:?}");
    assert!(!debug.contains("secret"));
    assert!(!debug.contains("115, 101, 99"));
    let image = store.into_medium().healed();
    let mut store = Store::open(image, LAYOUT).unwrap_or_else(|error| panic!("fixture: {error:?}"));
    assert_eq!(store.commit(0, b"blind"), Err(Error::Conflict));
    assert_eq!(store.into_medium().effects, 0);
}

#[test]
fn test_should_never_wrap_revision_or_accept_crc_valid_invalid_bounds() {
    // Independent non-reflected polynomial-division CRC32C fixtures, checked against
    // the standard 123456789 => e3069283 vector, not production encoding helpers.
    let maximum_header = [
        68, 83, 78, 65, 80, 48, 48, 49, 255, 255, 255, 255, 255, 255, 255, 255, 13, 0, 0, 0, 117,
        101, 50, 122, 0, 0, 0, 0, 148, 219, 36, 212,
    ];
    let mut image = populated(1);
    image.data[..32].copy_from_slice(&maximum_header);
    let mut store = Store::open(image, LAYOUT).unwrap_or_else(|error| panic!("fixture: {error:?}"));
    assert_eq!(loaded(&store), Some((u64::MAX, vec![1; 13])));
    assert_eq!(
        store.commit(u64::MAX, b"bad"),
        Err(Error::RevisionExhausted)
    );
    assert_eq!(store.into_medium().effects, 0);
    for header in [
        [
            68, 83, 78, 65, 80, 48, 48, 49, 0, 0, 0, 0, 0, 0, 0, 0, 13, 0, 0, 0, 117, 101, 50, 122,
            0, 0, 0, 0, 64, 23, 145, 42,
        ],
        [
            68, 83, 78, 65, 80, 48, 48, 49, 1, 0, 0, 0, 0, 0, 0, 0, 65, 0, 0, 0, 117, 101, 50, 122,
            0, 0, 0, 0, 0, 7, 208, 50,
        ],
    ] {
        let mut image = populated(1);
        image.data[..32].copy_from_slice(&header);
        assert!(matches!(Store::open(image, LAYOUT), Err(Error::Corrupt)));
    }
    assert!(matches!(
        AtomicSnapshot::<_, 221>::open(Nor::blank(), LAYOUT),
        Err(Error::InvalidLayout)
    ));
    let mut empty = AtomicSnapshot::<_, 0>::open(Nor::blank(), LAYOUT)
        .unwrap_or_else(|error| panic!("fixture: {error:?}"));
    assert_eq!(empty.commit(0, b"x"), Err(Error::Capacity));
    assert_eq!(
        empty.commit(0, b""),
        Ok(Snapshot {
            revision: 1,
            length: 0
        })
    );
}

#[test]
fn test_should_recover_whole_snapshots_after_partial_barrier_flushes() {
    let image = populated(3);
    for fail_sync in 1..=3 {
        for flush_reverse in [false, true] {
            for flush_cut in 0..=512 {
                let medium = Buffered {
                    durable: image.data.clone(),
                    live: image.clone(),
                    fail_sync: Some(fail_sync),
                    syncs: 0,
                    flush_cut,
                    flush_reverse,
                };
                let mut store = AtomicSnapshot::<_, 64>::open(medium, LAYOUT)
                    .unwrap_or_else(|error| panic!("fixture: {error:?}"));
                assert_eq!(store.commit(3, b"new-policy"), Err(Error::Io));
                assert_eq!(store.load(&mut [0; 64]), Err(Error::NeedsRecovery));
                let medium = store.into_medium();
                let mut restarted = medium.live.healed();
                restarted.data = medium.durable;
                let actual = loaded(&Store::open(restarted, LAYOUT).unwrap_or_else(|error| {
                    panic!(
                        "barrier {fail_sync}, cut {flush_cut}, reverse {flush_reverse}: {error:?}"
                    )
                }));
                assert!(
                    actual == Some((3, vec![3; 13])) || actual == Some((4, b"new-policy".to_vec())),
                    "barrier {fail_sync}, cut {flush_cut}, reverse {flush_reverse}: {actual:?}"
                );
            }
        }
    }
}
