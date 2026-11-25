//! Optional paged history is a real selected recovery owner, without DATA activation.
#![cfg(feature = "runtime")]
#![allow(clippy::unwrap_used, clippy::expect_used)]
#[path = "../../../crates/device-history-store/tests/paged_service_support/mod.rs"]
mod paged_support;
use device_history_service::{HistoryFailure, RecordStore};
use device_runtime::PortError;
use paged_support::*;
use wattch5_product::runtime::{HistoryRecovery, RecoverOwner};

fn require_history_family<H: HistoryRecovery>(owner: &mut H) -> Result<(), PortError> {
    owner.recover_owner()
}

// A no-op recovery or missing family adapter leaves the old proof/handle alive.
#[test]
fn test_should_recover_selected_paged_history_owner_preserving_ack_and_revoking_proof() {
    let control = MediumControl::blank();
    let clock = Clock::new();
    {
        let mut media = control.media();
        let mut store = initialize(&mut media, clock.clone());
        store.append(input(&[60, 7, 0, 1])).unwrap();
        store.append(input(&[61, 8, 0, 1])).unwrap();
        let old = store.open(1, filter(), 0, &Access).unwrap();
        store.record_admitted(old, 1, &Access).unwrap();
        store.ack_atomic(old, filter(), 3, 1, 0, &Access).unwrap();
        store.record_admitted(old, 2, &Access).unwrap();
        assert_eq!(require_history_family(&mut store), Ok(()));
        assert_eq!(
            store.record_admitted(old, 2, &Access),
            Err(HistoryFailure::CursorExpired)
        );
        let current = store.open(2, filter(), 0, &Access).unwrap();
        assert!(current.handle > old.handle);
        assert_eq!(current.committed_id, 1);
        assert_eq!(store.count(filter(), &Access).unwrap(), (3, 2));
        assert_eq!(
            store.ack_atomic(current, filter(), 3, 2, 0, &Access),
            Err(HistoryFailure::InvalidState)
        );
        let mut body = [0; 16];
        let content = store.read(current, 2, &mut body, &Access).unwrap();
        assert_eq!(&body[..content.length], &[61, 8, 0, 1]);
        store.record_admitted(current, 2, &Access).unwrap();
        store
            .ack_atomic(current, filter(), 3, 2, 0, &Access)
            .unwrap();
    }
    let mut media = control.media();
    clock.set(100);
    let mut store = reopen(&mut media, clock);
    let current = store.open(3, filter(), 100, &Access).unwrap();
    assert_eq!(current.committed_id, 2);
    assert_eq!(store.count(filter(), &Access).unwrap(), (3, 2));
    store.close(current).unwrap();
    assert_eq!(store.append(input(&[62, 9, 0, 1])).unwrap(), 3);
}

// An unconditional success adapter would hide a real recover sync failure.
#[test]
fn test_should_propagate_paged_recovery_failure_and_retain_durable_ack_after_retry() {
    let control = MediumControl::blank();
    let mut media = control.media();
    let mut store = initialize(&mut media, Clock::new());
    store.append(input(&[60, 7, 0, 1])).unwrap();
    let lease = store.open(1, filter(), 0, &Access).unwrap();
    store.record_admitted(lease, 1, &Access).unwrap();
    store.ack_atomic(lease, filter(), 3, 1, 0, &Access).unwrap();
    store.close(lease).unwrap();
    control.fail_sync.set(Some(control.syncs.get() + 1));
    assert_eq!(
        RecoverOwner::recover_owner(&mut store),
        Err(PortError::Fault)
    );
    assert!(store.directory().is_err());
    assert_eq!(require_history_family(&mut store), Ok(()));
    let current = store.open(2, filter(), 0, &Access).unwrap();
    assert_eq!(current.committed_id, 1);
    assert_eq!(store.count(filter(), &Access).unwrap(), (3, 1));
}
