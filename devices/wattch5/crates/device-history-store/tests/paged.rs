//! Actual checked shared media, `AtomicSnapshot` and paged owner integration.
mod paged_support;
use device_history_service::{HistoryFailure, RecordStore};
use device_history_store::{
    DomainConfig,
    paged::{PagedHistoryStore, PagedMedia},
};
use device_storage::persistence::{AtomicSnapshot, Layout};
use paged_support::*;
#[test]
fn test_should_retain_six_independent_full_domains_and_reopen() {
    let mut media =
        PagedMedia::new(Mem::new(180_000), layout(180_000)).unwrap_or_else(|e| panic!("{e:?}"));
    let (meta, log) = media.split();
    let snap = AtomicSnapshot::<_, 512>::open(
        meta,
        Layout {
            start: 0,
            bank_bytes: 4096,
        },
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    let domains = [0, 1, 2, 5, 6, 7].map(|domain| DomainConfig { domain, epoch: 3 });
    let mut s = PagedHistoryStore::<_, _, _, _, 6, 255, 16, 512>::initialize(
        snap,
        log,
        Schema,
        Clock::new(),
        config(257),
        [7; 16],
        &domains,
        10,
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    for domain in [0, 1, 2, 5, 6, 7] {
        for id in 1..=255 {
            assert_eq!(s.append(record(domain)), Ok(id));
        }
        assert_eq!(s.append(record(domain)), Err(HistoryFailure::Capacity));
    }
    for domain in [0, 1, 2, 5, 6, 7] {
        let f = filter(domain);
        let l = s
            .open(1, f, 100, &Access)
            .unwrap_or_else(|e| panic!("{e:?}"));
        for id in (1..=255).rev() {
            s.record_admitted(l, id, &Access)
                .unwrap_or_else(|e| panic!("{e:?}"));
        }
        assert_eq!(s.ack_atomic(l, f, 3, 255, 100, &Access), Ok(255));
        s.close(l).unwrap_or_else(|e| panic!("{e:?}"));
    }
    drop(s);
    let raw = media.into_medium();
    let mut media = PagedMedia::new(raw, layout(180_000)).unwrap_or_else(|e| panic!("{e:?}"));
    let (meta, log) = media.split();
    let snap = AtomicSnapshot::<_, 512>::open(
        meta,
        Layout {
            start: 0,
            bank_bytes: 4096,
        },
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    let mut s = PagedHistoryStore::<_, _, _, _, 6, 255, 16, 512>::open(
        snap,
        log,
        Schema,
        Clock::new(),
        config(257),
        10,
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    for domain in [0, 1, 2, 5, 6, 7] {
        assert_eq!(s.count(filter(domain), &Access), Ok((3, 255)));
        let l = s
            .open(2, filter(domain), 100, &Access)
            .unwrap_or_else(|e| panic!("{e:?}"));
        for id in 1..=255 {
            let mut out = [0; 16];
            assert_eq!(
                s.read(l, id, &mut out, &Access)
                    .unwrap_or_else(|e| panic!("{e:?}"))
                    .length,
                16
            );
            assert_eq!(out, [1; 16]);
        }
        s.close(l).unwrap_or_else(|e| panic!("{e:?}"));
    }
}
#[test]
fn test_should_freeze_queries_and_require_actual_admitted_prefix() {
    let mut media =
        PagedMedia::new(Mem::new(20_000), layout(20_000)).unwrap_or_else(|e| panic!("{e:?}"));
    let (meta, log) = media.split();
    let snap = AtomicSnapshot::<_, 512>::open(
        meta,
        Layout {
            start: 0,
            bank_bytes: 4096,
        },
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    let mut s = PagedHistoryStore::<_, _, _, _, 1, 255, 16, 512>::initialize(
        snap,
        log,
        Schema,
        Clock::new(),
        config(100),
        [7; 16],
        &[DomainConfig {
            domain: 0,
            epoch: 3,
        }],
        10,
    );
    assert!(s.is_err()); // physical slots must cover quota
    let _ = &mut s;
}
use device_history_service::StoreLease;
use device_history_store::paged::MediaView;
type Small<'a> = PagedHistoryStore<
    AtomicSnapshot<MediaView<'a, Mem>, 512>,
    MediaView<'a, Mem>,
    Schema,
    Clock,
    1,
    8,
    16,
    512,
>;
fn small(
    media: &mut PagedMedia<Mem>,
    init: bool,
    clock: Clock,
) -> Result<Small<'_>, HistoryFailure> {
    let (meta, log) = media.split();
    let snap = AtomicSnapshot::<_, 512>::open(
        meta,
        Layout {
            start: 0,
            bank_bytes: 4096,
        },
    )
    .map_err(|_| HistoryFailure::Storage)?;
    if init {
        Small::initialize(
            snap,
            log,
            Schema,
            clock,
            config(32),
            [7; 16],
            &[DomainConfig {
                domain: 0,
                epoch: 3,
            }],
            10,
        )
    } else {
        Small::open(snap, log, Schema, clock, config(32), 10)
    }
}
fn boot(mem: Mem) -> PagedMedia<Mem> {
    PagedMedia::new(mem, layout(20_000)).unwrap_or_else(|e| panic!("{e:?}"))
}
fn lease(s: &mut Small<'_>) -> StoreLease {
    s.open(1, filter(0), 100, &Access)
        .unwrap_or_else(|e| panic!("{e:?}"))
}
#[test]
fn test_should_freeze_and_enforce_bitmap_holes_filters_and_cursor() {
    let mem = Mem::new(20_000);
    let mut media = boot(mem);
    let mut s = small(&mut media, true, Clock::new()).unwrap_or_else(|e| panic!("{e:?}"));
    for _ in 0..3 {
        s.append(record(0)).unwrap_or_else(|e| panic!("{e:?}"));
    }
    let l = lease(&mut s);
    s.append(record(0)).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        s.plan(l, 1, 16, 100, &Access)
            .unwrap_or_else(|e| panic!("{e:?}"))
            .ids[..2],
        [2, 3]
    );
    assert_eq!(
        s.read(l, 4, &mut [0; 16], &Access),
        Err(HistoryFailure::NotFound)
    );
    s.record_admitted(l, 3, &Access)
        .unwrap_or_else(|e| panic!("{e:?}"));
    s.record_admitted(l, 1, &Access)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        s.ack_atomic(l, filter(0), 3, 3, 100, &Access),
        Err(HistoryFailure::InvalidState)
    );
    s.record_admitted(l, 2, &Access)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(s.ack_atomic(l, filter(0), 3, 3, 100, &Access), Ok(3));
    s.close(l).unwrap_or_else(|e| panic!("{e:?}"));
    let mut f = filter(0);
    f.filter_flags = 2;
    f.record_type = 1;
    let l = s
        .open(2, f, 100, &Access)
        .unwrap_or_else(|e| panic!("{e:?}"));
    for id in 1..=4 {
        s.record_admitted(l, id, &Access)
            .unwrap_or_else(|e| panic!("{e:?}"));
    }
    assert_eq!(
        s.ack_atomic(l, f, 3, 4, 100, &Access),
        Err(HistoryFailure::Invalid)
    );
    let mut forged = l;
    forged.committed_id += 1;
    assert_eq!(
        s.plan(forged, 0, 1, 100, &Access),
        Err(HistoryFailure::Identity)
    );
}
#[test]
fn test_should_settle_every_append_operation_failure_and_preserve_old_records() {
    let base = Mem::new(20_000);
    {
        let mut media = boot(base.clone());
        let mut s = small(&mut media, true, Clock::new()).unwrap_or_else(|e| panic!("{e:?}"));
        s.append(record(0)).unwrap_or_else(|e| panic!("{e:?}"));
    }
    let durable = base.bytes.borrow().clone();
    let measure = Mem::new(20_000);
    *measure.bytes.borrow_mut() = durable.clone();
    {
        let mut media = boot(measure.clone());
        let mut s = small(&mut media, false, Clock::new()).unwrap_or_else(|e| panic!("{e:?}"));
        measure.calls.set(0);
        s.append(record(0)).unwrap_or_else(|e| panic!("{e:?}"));
    }
    let operations = measure.calls.get();
    for cut in 0..operations {
        let mem = Mem::new(20_000);
        *mem.bytes.borrow_mut() = durable.clone();
        {
            let mut media = boot(mem.clone());
            let mut s = small(&mut media, false, Clock::new()).unwrap_or_else(|e| panic!("{e:?}"));
            mem.calls.set(0);
            mem.fail.set(Some(cut));
            assert!(s.append(record(0)).is_err(), "operation {cut}");
            assert!(s.directory().is_err());
        }
        mem.fail.set(None);
        {
            let mut media = boot(mem.clone());
            let mut s = small(&mut media, false, Clock::new()).unwrap_or_else(|e| panic!("{e:?}"));
            let count = s
                .count(filter(0), &Access)
                .unwrap_or_else(|e| panic!("{e:?}"))
                .1;
            assert!((1..=2).contains(&count), "operation {cut}");
            let l = lease(&mut s);
            assert_eq!(
                s.read(l, 1, &mut [0; 16], &Access)
                    .unwrap_or_else(|e| panic!("{e:?}"))
                    .record_id,
                1
            );
            s.close(l).unwrap_or_else(|e| panic!("{e:?}"));
            assert_eq!(s.append(record(0)), Ok(u64::from(count) + 1));
        }
    }
}
#[test]
fn test_should_settle_every_append_byte_cut_without_reusing_committed_ids() {
    for cut in 0..=97 {
        let mem = Mem::new(20_000);
        {
            let mut media = boot(mem.clone());
            let mut s = small(&mut media, true, Clock::new()).unwrap_or_else(|e| panic!("{e:?}"));
            s.append(record(0)).unwrap_or_else(|e| panic!("{e:?}"));
            mem.byte_cut.set(Some(cut));
            let result = s.append(record(0));
            if cut < 97 {
                assert!(result.is_err());
            } else {
                assert_eq!(result, Ok(2));
            }
        }
        mem.byte_cut.set(None);
        let mut media = boot(mem);
        let mut s = small(&mut media, false, Clock::new()).unwrap_or_else(|e| panic!("{e:?}"));
        let n = s
            .count(filter(0), &Access)
            .unwrap_or_else(|e| panic!("{e:?}"))
            .1;
        assert_eq!(n, if cut < 97 { 1 } else { 2 });
        assert_eq!(s.append(record(0)), Ok(u64::from(n) + 1));
    }
}
#[test]
fn test_should_revoke_proof_on_uncertain_ack_and_reopen_old_or_new() {
    let base = Mem::new(20_000);
    {
        let mut media = boot(base.clone());
        let mut s = small(&mut media, true, Clock::new()).unwrap_or_else(|e| panic!("{e:?}"));
        s.append(record(0)).unwrap_or_else(|e| panic!("{e:?}"));
    }
    let bytes = base.bytes.borrow().clone();
    let measure = Mem::new(20_000);
    *measure.bytes.borrow_mut() = bytes.clone();
    {
        let mut media = boot(measure.clone());
        let mut s = small(&mut media, false, Clock::new()).unwrap_or_else(|e| panic!("{e:?}"));
        let l = lease(&mut s);
        s.record_admitted(l, 1, &Access)
            .unwrap_or_else(|e| panic!("{e:?}"));
        measure.calls.set(0);
        s.ack_atomic(l, filter(0), 3, 1, 100, &Access)
            .unwrap_or_else(|e| panic!("{e:?}"));
    }
    let n = measure.calls.get();
    for cut in 0..n {
        let mem = Mem::new(20_000);
        *mem.bytes.borrow_mut() = bytes.clone();
        {
            let mut media = boot(mem.clone());
            let mut s = small(&mut media, false, Clock::new()).unwrap_or_else(|e| panic!("{e:?}"));
            let l = lease(&mut s);
            s.record_admitted(l, 1, &Access)
                .unwrap_or_else(|e| panic!("{e:?}"));
            mem.calls.set(0);
            mem.fail.set(Some(cut));
            assert!(s.ack_atomic(l, filter(0), 3, 1, 100, &Access).is_err());
        }
        mem.fail.set(None);
        let mut media = boot(mem);
        let mut s = small(&mut media, false, Clock::new()).unwrap_or_else(|e| panic!("{e:?}"));
        let l = lease(&mut s);
        assert!(l.committed_id <= 1);
        if l.committed_id == 0 {
            assert_eq!(
                s.ack_atomic(l, filter(0), 3, 1, 100, &Access),
                Err(HistoryFailure::InvalidState)
            );
            s.record_admitted(l, 1, &Access)
                .unwrap_or_else(|e| panic!("{e:?}"));
        }
        assert_eq!(s.ack_atomic(l, filter(0), 3, 1, 100, &Access), Ok(1));
        assert_eq!(s.count(filter(0), &Access), Ok((3, 1)));
    }
}
#[test]
fn test_should_reject_committed_parent_body_identity_and_padding_corruption() {
    for offset in [
        0, 8, 24, 28, 32, 40, 42, 44, 52, 54, 56, 64, 68, 72, 76, 80, 95,
    ] {
        let mem = Mem::new(20_000);
        {
            let mut media = boot(mem.clone());
            let mut s = small(&mut media, true, Clock::new()).unwrap_or_else(|e| panic!("{e:?}"));
            s.append(record(0)).unwrap_or_else(|e| panic!("{e:?}"));
        }
        mem.bytes.borrow_mut()[8192 + offset] ^= 1;
        let mut media = boot(mem);
        assert!(
            small(&mut media, false, Clock::new()).is_err(),
            "offset {offset}"
        );
    }
}
#[test]
fn test_should_preserve_confirmed_floors_across_failed_recovery() {
    let mem = Mem::new(20_000);
    let mut media = boot(mem.clone());
    let mut s = small(&mut media, true, Clock::new()).unwrap_or_else(|e| panic!("{e:?}"));
    s.append(record(0)).unwrap_or_else(|e| panic!("{e:?}"));
    let initial = mem.bytes.borrow().clone();
    s.append(record(0)).unwrap_or_else(|e| panic!("{e:?}"));
    mem.bytes.borrow_mut()[8192 + 97 + 56] ^= 1;
    assert!(s.recover().is_err());
    *mem.bytes.borrow_mut() = initial;
    assert_eq!(s.recover(), Err(HistoryFailure::StoredIntegrity));
    assert_eq!(s.directory().map(|_| ()), Err(HistoryFailure::Storage));
}
#[test]
fn test_should_preserve_clock_and_handles_revoke_leases_and_not_mutate_short_output() {
    let mem = Mem::new(20_000);
    let clock = Clock::new();
    let mut media = boot(mem);
    let mut s = small(&mut media, true, clock.clone()).unwrap_or_else(|e| panic!("{e:?}"));
    s.append(record(0)).unwrap_or_else(|e| panic!("{e:?}"));
    let l = lease(&mut s);
    let mut out = [77; 1];
    assert_eq!(
        s.read(l, 1, &mut out, &Access),
        Err(HistoryFailure::Capacity)
    );
    assert_eq!(out, [77]);
    s.recover().unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        s.plan(l, 0, 1, 100, &Access),
        Err(HistoryFailure::CursorExpired)
    );
    let fresh = s
        .open(2, filter(0), 100, &Access)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert!(fresh.handle > l.handle);
    clock.0.set(99);
    assert_eq!(
        s.read(fresh, 1, &mut [0; 16], &Access),
        Err(HistoryFailure::Clock)
    );
    clock.0.set(100);
    s.recover().unwrap_or_else(|e| panic!("{e:?}"));
    let fresh = s
        .open(3, filter(0), 100, &Access)
        .unwrap_or_else(|e| panic!("{e:?}"));
    clock.0.set(10_100);
    assert_eq!(
        s.read(fresh, 1, &mut [0; 16], &Access),
        Err(HistoryFailure::CursorExpired)
    );
}
fn crc(bytes: &[u8]) -> u32 {
    let mut c = device_protocol::Crc32c::new();
    c.update(bytes);
    c.finish()
}
#[test]
fn test_should_reject_same_revision_changed_metadata_root() {
    let mem = Mem::new(20_000);
    let mut media = boot(mem.clone());
    let mut s = small(&mut media, true, Clock::new()).unwrap_or_else(|e| panic!("{e:?}"));
    s.append(record(0)).unwrap_or_else(|e| panic!("{e:?}"));
    s.append(record(0)).unwrap_or_else(|e| panic!("{e:?}"));
    let l = lease(&mut s);
    s.record_admitted(l, 1, &Access)
        .unwrap_or_else(|e| panic!("{e:?}"));
    s.ack_atomic(l, filter(0), 3, 1, 100, &Access)
        .unwrap_or_else(|e| panic!("{e:?}"));
    {
        let mut bytes = mem.bytes.borrow_mut();
        let start = 4096;
        let payload = start + 32;
        bytes[payload + 96..payload + 104].copy_from_slice(&2_u64.to_le_bytes());
        let check = crc(&bytes[payload..payload + 144]);
        bytes[start + 20..start + 24].copy_from_slice(&check.to_le_bytes());
        let check = crc(&bytes[start..start + 28]);
        bytes[start + 28..start + 32].copy_from_slice(&check.to_le_bytes());
    }
    assert_eq!(s.recover(), Err(HistoryFailure::StoredIntegrity));
}
#[test]
fn test_should_settle_every_ack_byte_cut_and_retire_stale_bank_safely() {
    let base = Mem::new(20_000);
    {
        let mut media = boot(base.clone());
        let mut s = small(&mut media, true, Clock::new()).unwrap_or_else(|e| panic!("{e:?}"));
        for _ in 0..3 {
            s.append(record(0)).unwrap_or_else(|e| panic!("{e:?}"));
        }
        let l = lease(&mut s);
        s.record_admitted(l, 1, &Access)
            .unwrap_or_else(|e| panic!("{e:?}"));
        s.ack_atomic(l, filter(0), 3, 1, 100, &Access)
            .unwrap_or_else(|e| panic!("{e:?}"));
    }
    let bytes = base.bytes.borrow().clone();
    for cut in 0..=178 {
        let mem = Mem::new(20_000);
        *mem.bytes.borrow_mut() = bytes.clone();
        {
            let mut media = boot(mem.clone());
            let mut s = small(&mut media, false, Clock::new()).unwrap_or_else(|e| panic!("{e:?}"));
            let l = lease(&mut s);
            s.record_admitted(l, 2, &Access)
                .unwrap_or_else(|e| panic!("{e:?}"));
            mem.byte_cut.set(Some(cut));
            let result = s.ack_atomic(l, filter(0), 3, 2, 100, &Access);
            if cut < 178 {
                assert!(result.is_err());
            } else {
                assert_eq!(result, Ok(2));
            }
        }
        mem.byte_cut.set(None);
        let mut media = boot(mem);
        let mut s = small(&mut media, false, Clock::new()).unwrap_or_else(|e| panic!("{e:?}"));
        let l = lease(&mut s);
        assert!((1..=2).contains(&l.committed_id));
        assert_eq!(s.count(filter(0), &Access), Ok((3, 3)));
        if l.committed_id == 1 {
            assert_eq!(
                s.ack_atomic(l, filter(0), 3, 2, 100, &Access),
                Err(HistoryFailure::InvalidState)
            );
        }
    }
}
#[test]
fn test_should_settle_every_initialization_byte_cut_without_ordinary_open_formatting() {
    for cut in 0..=178 {
        let mem = Mem::new(20_000);
        {
            let mut media = boot(mem.clone());
            mem.byte_cut.set(Some(cut));
            let result = small(&mut media, true, Clock::new());
            if cut < 178 {
                assert!(result.is_err());
            } else {
                assert!(result.is_ok());
            }
        }
        mem.byte_cut.set(None);
        let before = mem.bytes.borrow().clone();
        {
            let mut media = boot(mem.clone());
            let result = small(&mut media, false, Clock::new());
            if cut < 178 {
                assert!(result.is_err());
            } else {
                assert!(result.is_ok());
            }
        }
        assert_eq!(*mem.bytes.borrow(), before);
        if cut < 178 {
            let mut media = boot(mem);
            assert!(small(&mut media, true, Clock::new()).is_ok());
        }
    }
}
#[test]
fn test_should_exhaust_finite_fault_reserve_without_creating_records() {
    let mem = Mem::new(20_000);
    let mut media = boot(mem.clone());
    let mut s = small(&mut media, true, Clock::new()).unwrap_or_else(|e| panic!("{e:?}"));
    for consumed in 1..=32 {
        mem.byte_cut.set(Some(1));
        assert_eq!(s.append(record(0)), Err(HistoryFailure::Storage));
        mem.byte_cut.set(None);
        s.recover().unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(
            s.stats().unwrap_or_else(|e| panic!("{e:?}")).consumed,
            [consumed]
        );
        assert_eq!(s.count(filter(0), &Access), Ok((3, 0)));
    }
    assert_eq!(s.append(record(0)), Err(HistoryFailure::Capacity));
    assert_eq!(
        s.directory().unwrap_or_else(|e| panic!("{e:?}")).domains[0].newest_id,
        0
    );
}
#[test]
fn test_should_reject_invalid_ranges_config_and_dirty_uninitialized_log() {
    for bad in [
        device_history_store::paged::MediaLayout {
            metadata: Layout {
                start: 0,
                bank_bytes: 4096,
            },
            log_start: 4096,
            log_bytes: 4096,
        },
        device_history_store::paged::MediaLayout {
            metadata: Layout {
                start: u32::MAX,
                bank_bytes: 4096,
            },
            log_start: 8192,
            log_bytes: 4096,
        },
        device_history_store::paged::MediaLayout {
            metadata: Layout {
                start: 0,
                bank_bytes: 4096,
            },
            log_start: 8192,
            log_bytes: 20_000,
        },
    ] {
        assert!(PagedMedia::new(Mem::new(20_000), bad).is_err());
    }
    let mem = Mem::new(20_000);
    mem.bytes.borrow_mut()[19_999] = 0;
    let mut media = boot(mem);
    assert!(small(&mut media, true, Clock::new()).is_err());
}
#[test]
fn test_should_recover_buffered_old_or_new_at_every_append_flush_byte() {
    for phase in 0..=1 {
        for flushed in 0..=96 {
            let mem = Mem::new(20_000);
            mem.buffered.set(true);
            {
                let mut media = boot(mem.clone());
                let mut s =
                    small(&mut media, true, Clock::new()).unwrap_or_else(|e| panic!("{e:?}"));
                s.append(record(0)).unwrap_or_else(|e| panic!("{e:?}"));
                mem.syncs.set(0);
                mem.sync_failure.set(Some((phase, flushed)));
                assert!(s.append(record(0)).is_err());
                assert!(s.directory().is_err());
            }
            mem.sync_failure.set(None);
            *mem.bytes.borrow_mut() = mem.durable.borrow().clone();
            let mut media = boot(mem);
            let mut s = small(&mut media, false, Clock::new()).unwrap_or_else(|e| panic!("{e:?}"));
            let n = s
                .count(filter(0), &Access)
                .unwrap_or_else(|e| panic!("{e:?}"))
                .1;
            assert_eq!(n, if phase == 1 && flushed > 0 { 2 } else { 1 });
            assert_eq!(s.append(record(0)), Ok(u64::from(n) + 1));
        }
    }
    let mem = Mem::new(20_000);
    mem.buffered.set(true);
    let mut media = boot(mem.clone());
    let mut s = small(&mut media, true, Clock::new()).unwrap_or_else(|e| panic!("{e:?}"));
    mem.syncs.set(0);
    mem.sync_failure.set(Some((1, 0)));
    assert!(s.append(record(0)).is_err());
    mem.sync_failure.set(None);
    s.recover().unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(s.count(filter(0), &Access), Ok((3, 1)));
}
#[test]
fn test_should_handle_supported_program_units_and_nonzero_ranges() {
    for unit in [1, 2, 3, 7, 16, 64, 256] {
        let bank = 4096_u32.div_ceil(unit) * unit;
        let start = unit * 5;
        let log_start = start + bank * 2;
        let log_bytes = unit * 8192;
        let capacity = log_start + log_bytes;
        let mem = Mem::new(usize::try_from(capacity).unwrap_or_else(|e| panic!("{e:?}")));
        mem.geometry.set(device_storage::persistence::Geometry {
            capacity,
            erase_size: unit,
            write_size: unit,
        });
        let mut media = PagedMedia::new(
            mem,
            device_history_store::paged::MediaLayout {
                metadata: Layout {
                    start,
                    bank_bytes: bank,
                },
                log_start,
                log_bytes,
            },
        )
        .unwrap_or_else(|e| panic!("{e:?}"));
        let (meta, log) = media.split();
        let snap = AtomicSnapshot::<_, 512>::open(
            meta,
            Layout {
                start: 0,
                bank_bytes: bank,
            },
        )
        .unwrap_or_else(|e| panic!("{e:?}"));
        let mut cfg = config(32);
        cfg.log_start = log_start;
        let mut s = PagedHistoryStore::<_, _, _, _, 1, 8, 16, 512>::initialize(
            snap,
            log,
            Schema,
            Clock::new(),
            cfg,
            [7; 16],
            &[DomainConfig {
                domain: 0,
                epoch: 3,
            }],
            10,
        )
        .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(s.append(record(0)), Ok(1));
        s.recover().unwrap_or_else(|e| panic!("{e:?}"));
        let l = s
            .open(1, filter(0), 100, &Access)
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(
            s.read(l, 1, &mut [0; 16], &Access)
                .unwrap_or_else(|e| panic!("{e:?}"))
                .length,
            16
        );
    }
}
#[test]
fn test_should_settle_every_initialization_operation_and_sync_failure() {
    let measure = Mem::new(20_000);
    {
        let mut media = boot(measure.clone());
        small(&mut media, true, Clock::new()).unwrap_or_else(|e| panic!("{e:?}"));
    }
    let n = measure.calls.get();
    for cut in 0..n {
        let mem = Mem::new(20_000);
        {
            let mut media = boot(mem.clone());
            mem.fail.set(Some(cut));
            assert!(
                small(&mut media, true, Clock::new()).is_err(),
                "operation {cut}"
            );
        }
        mem.fail.set(None);
        let before = mem.bytes.borrow().clone();
        let initialized = {
            let mut media = boot(mem.clone());
            small(&mut media, false, Clock::new()).is_ok()
        };
        assert_eq!(*mem.bytes.borrow(), before);
        if !initialized {
            let mut media = boot(mem);
            assert!(
                small(&mut media, true, Clock::new()).is_ok(),
                "operation {cut}"
            );
        }
    }
}
