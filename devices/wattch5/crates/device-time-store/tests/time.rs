//! UTC continuity, drift, authorization and restart contracts.
mod common;
use common::{Access, Nor, Source, must, store};
use device_services::ClockSourceError;
use device_services::TimeAccess;
use device_time_store::{ClockReading, Error, TimeStore};

#[test]
fn test_should_leave_blank_media_unchanged_and_use_actual_monotonic_time() {
    let media = Nor::blank();
    let source = Source::new(123);
    let owner = must(TimeStore::open(store(media.clone()), source, 0));
    let access = Access::all();
    let mapping = must(owner.read(&access));
    assert_eq!(
        (
            mapping.utc_ms,
            mapping.mono_us,
            mapping.uncertainty_ms,
            mapping.valid
        ),
        (0, 123, 0, false)
    );
    assert_eq!(must(owner.revision()), 0);
    assert!(must(owner.evidence()).is_none());
    assert_eq!(media.0.borrow().bytes, [255; 512]);
    must(owner.synchronize(1_000, 7, &access));
    assert_eq!(must(owner.revision()), 1);
}

#[test]
fn test_should_restore_only_the_attested_continuous_source_after_restart() {
    let source = Source::new(10_000);
    let access = Access::all();
    let owner = must(TimeStore::open(store(Nor::blank()), source.clone(), 2_000));
    must(owner.synchronize(1_000, 7, &access));
    source.at(11_501);
    let mapping = must(owner.read(&access));
    assert_eq!(
        (mapping.utc_ms, mapping.uncertainty_ms, mapping.valid),
        (1_001, 9, true)
    );
    let (snapshot, source) = owner.into_parts();
    let media = snapshot.into_medium();
    media.heal();
    source.at(20_000);
    let owner = must(TimeStore::open(store(media), source.clone(), 100));
    let mapping = must(owner.read(&access));
    assert_eq!(
        (mapping.utc_ms, mapping.uncertainty_ms, mapping.valid),
        (1_010, 8, true)
    );
    source.0.set(Ok(ClockReading {
        source_id: 11,
        epoch: 23,
        mono_us: 50,
    }));
    let mapping = must(owner.read(&access));
    assert_eq!(
        (
            mapping.utc_ms,
            mapping.mono_us,
            mapping.uncertainty_ms,
            mapping.valid
        ),
        (0, 50, 0, false)
    );
    assert!(must(owner.evidence()).is_some());
    source.at(21_000);
    assert!(!must(owner.read(&access)).valid);
    must(owner.recover_and_reload());
    assert!(!must(owner.read(&access)).valid);
    must(owner.synchronize(2_000, 3, &access));
    assert!(must(owner.read(&access)).valid);
}

#[test]
fn test_should_distinguish_temporary_unavailability_from_lost_continuity() {
    let source = Source::new(1_000);
    let access = Access::all();
    let owner = must(TimeStore::open(store(Nor::blank()), source.clone(), 0));
    must(owner.synchronize(100, 0, &access));
    source.0.set(Err(ClockSourceError::Unavailable));
    assert_eq!(
        owner.read(&access),
        Err(Error::Clock(ClockSourceError::Unavailable))
    );
    source.at(2_000);
    assert!(must(owner.read(&access)).valid);
    source.0.set(Err(ClockSourceError::Fault));
    assert_eq!(
        owner.read(&access),
        Err(Error::Clock(ClockSourceError::Fault))
    );
    source.at(3_000);
    assert!(!must(owner.read(&access)).valid);
    must(owner.recover_and_reload());
    assert!(!must(owner.read(&access)).valid);
    must(owner.synchronize(300, 0, &access));
    source.at(2_999);
    assert_eq!(owner.read(&access), Err(Error::NonMonotonic));
    source.at(4_000);
    assert!(!must(owner.read(&access)).valid);
}

#[test]
fn test_should_keep_actual_floor_across_recovery_and_denied_final_scope() {
    let source = Source::new(100);
    let access = Access::all();
    let owner = must(TimeStore::open(store(Nor::blank()), source.clone(), 0));
    must(owner.synchronize(1, 0, &access));
    source.at(200);
    must(owner.read(&access));
    must(owner.recover_and_reload());
    source.at(199);
    assert_eq!(owner.synchronize(2, 0, &access), Err(Error::NonMonotonic));
    access.set.set(false);
    source.at(300);
    assert_eq!(owner.synchronize(3, 0, &access), Err(Error::AccessDenied));
    assert_eq!(must(owner.revision()), 1);
}

#[test]
fn test_should_reject_unproven_identity_and_checked_utc_uncertainty_overflow() {
    let source = Source::new(0);
    let access = Access::all();
    let owner = must(TimeStore::open(store(Nor::blank()), source.clone(), 1));
    source.0.set(Ok(ClockReading {
        source_id: 0,
        epoch: 22,
        mono_us: 0,
    }));
    assert_eq!(owner.synchronize(0, 0, &access), Err(Error::InvalidReading));
    source.at(0);
    must(owner.synchronize(i64::MAX, 0, &access));
    source.at(1_000);
    assert_eq!(owner.read(&access), Err(Error::Overflow));
    must(owner.synchronize(0, u32::MAX, &access));
    source.at(1_001);
    assert_eq!(owner.read(&access), Err(Error::Overflow));
    assert_eq!(must(owner.revision()), 2);
}

#[test]
fn test_should_apply_increased_current_drift_to_old_anchors() {
    let source = Source::new(0);
    let access = Access::all();
    let owner = must(TimeStore::open(store(Nor::blank()), source.clone(), 0));
    must(owner.synchronize(1, 2, &access));
    let (snapshot, source) = owner.into_parts();
    let owner = must(TimeStore::open(snapshot, source.clone(), 2_000));
    source.at(1_000_000);
    let mapping = must(owner.read(&access));
    assert_eq!((mapping.utc_ms, mapping.uncertainty_ms), (1_001, 4));
}

#[test]
fn test_should_keep_rebooted_or_replaced_sources_unavailable_until_new_sync() {
    for reading in [
        ClockReading {
            source_id: 11,
            epoch: 22,
            mono_us: 0,
        },
        ClockReading {
            source_id: 12,
            epoch: 22,
            mono_us: 100,
        },
        ClockReading {
            source_id: 11,
            epoch: 23,
            mono_us: 100,
        },
    ] {
        let source = Source::new(1_000);
        let access = Access::all();
        let owner = must(TimeStore::open(store(Nor::blank()), source.clone(), 0));
        must(owner.synchronize(10, 0, &access));
        let (snapshot, source) = owner.into_parts();
        let media = snapshot.into_medium();
        media.heal();
        source.0.set(Ok(reading));
        let owner = must(TimeStore::open(store(media), source.clone(), 0));
        if reading.source_id == 11 && reading.epoch == 22 {
            assert_eq!(owner.read(&access), Err(Error::NonMonotonic));
            source.at(1_001);
        }
        assert!(!must(owner.read(&access)).valid);
        must(owner.synchronize(20, 0, &access));
        assert!(must(owner.read(&access)).valid);
    }
}

#[test]
fn test_should_latch_zero_identity_and_preserve_floor_after_post_sample_revocation() {
    use core::cell::Cell;
    struct RevokeAfterSample(Cell<u8>);
    impl TimeAccess for RevokeAfterSample {
        fn allowed(&self, _set: bool) -> bool {
            let count = self.0.get() + 1;
            self.0.set(count);
            count < 2
        }
    }
    let source = Source::new(100);
    let owner = must(TimeStore::open(store(Nor::blank()), source.clone(), 0));
    let access = Access::all();
    must(owner.synchronize(10, 0, &access));
    source.at(200);
    assert_eq!(
        owner.synchronize(20, 0, &RevokeAfterSample(Cell::new(0))),
        Err(Error::AuthorizationRevoked)
    );
    source.at(199);
    assert_eq!(owner.read(&access), Err(Error::NonMonotonic));
    source.at(201);
    must(owner.synchronize(20, 0, &access));
    for invalid in [
        ClockReading {
            source_id: 0,
            epoch: 22,
            mono_us: 201,
        },
        ClockReading {
            source_id: 11,
            epoch: 0,
            mono_us: 201,
        },
    ] {
        source.0.set(Ok(invalid));
        assert_eq!(owner.read(&access), Err(Error::InvalidReading));
        source.at(202);
        assert!(!must(owner.read(&access)).valid);
        must(owner.synchronize(30, 0, &access));
    }
}

#[test]
fn test_should_refuse_drift_multiplication_overflow_without_wrapping() {
    let source = Source::new(0);
    let access = Access::all();
    let owner = must(TimeStore::open(
        store(Nor::blank()),
        source.clone(),
        u32::MAX,
    ));
    must(owner.synchronize(-10, 0, &access));
    source.at(u64::MAX);
    assert_eq!(owner.read(&access), Err(Error::Overflow));
    source.at(u64::MAX - 1);
    assert_eq!(owner.read(&access), Err(Error::NonMonotonic));
}

#[test]
fn test_should_reject_recursive_mutation_from_trusted_source_callback() {
    use core::cell::{Cell, RefCell};
    use device_time_store::TrustedClock;
    use std::rc::Rc;
    type Hook = Rc<RefCell<Option<Box<dyn Fn()>>>>;
    struct HookSource {
        sample: Source,
        hook: Hook,
    }
    impl TrustedClock for HookSource {
        fn reading(&self) -> Result<ClockReading, ClockSourceError> {
            if let Some(hook) = self.hook.borrow().as_ref() {
                hook();
            }
            self.sample.reading()
        }
    }
    let hook: Hook = Rc::new(RefCell::new(None));
    let owner = Rc::new(must(TimeStore::open(
        store(Nor::blank()),
        HookSource {
            sample: Source::new(0),
            hook: hook.clone(),
        },
        0,
    )));
    let weak = Rc::downgrade(&owner);
    let nested_busy = Rc::new(Cell::new(false));
    let result = nested_busy.clone();
    *hook.borrow_mut() = Some(Box::new(move || {
        let owner = weak
            .upgrade()
            .unwrap_or_else(|| panic!("owner disappeared during callback"));
        result.set(owner.synchronize(99, 0, &Access::all()) == Err(Error::Busy));
    }));
    must(owner.synchronize(10, 0, &Access::all()));
    assert!(nested_busy.get());
    assert_eq!(must(owner.revision()), 1);
    assert_eq!(must(owner.read(&Access::all())).utc_ms, 10);
}
