//! One connection owns one DATA lane and both nonreusing identifier spaces.
use device_protocol::{ConnectionDataLane, DataKind, LaneError};
#[test]
fn test_should_exclude_parallel_history_raw_and_preserve_id_spaces_after_replacement() {
    let mut lane = ConnectionDataLane::new(core::num::NonZeroU64::MIN);
    let history = lane
        .acquire(DataKind::History)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(history.stream_id(), 1);
    assert_eq!(lane.acquire(DataKind::Raw), Err(LaneError::Busy));
    let id = lane
        .begin_message(history)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(id, 1);
    assert_eq!(lane.begin_message(history), Err(LaneError::Busy));
    assert_eq!(lane.release(history), Err(LaneError::Busy));
    lane.finish_message(history, id)
        .unwrap_or_else(|e| panic!("{e:?}"));
    lane.release(history).unwrap_or_else(|e| panic!("{e:?}"));
    let raw = lane
        .acquire(DataKind::Raw)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(raw.stream_id(), 2);
    assert_eq!(lane.begin_message(raw), Ok(2));
    assert_eq!(lane.begin_message(history), Err(LaneError::Identity));
}
#[test]
fn test_should_drain_started_message_before_cancel_and_keep_zero_barrier_unambiguous() {
    let mut lane = ConnectionDataLane::new(core::num::NonZeroU64::MIN);
    let lease = lane
        .acquire(DataKind::History)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(lane.cancel(lease), Ok(Some(0)));
    assert_eq!(lane.begin_message(lease), Err(LaneError::Cancelled));
    lane.release(lease).unwrap_or_else(|e| panic!("{e:?}"));
    let lease = lane
        .acquire(DataKind::Raw)
        .unwrap_or_else(|e| panic!("{e:?}"));
    let id = lane
        .begin_message(lease)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(lane.cancel(lease), Ok(None));
    assert_eq!(lane.cancel_barrier(lease), Err(LaneError::Busy));
    lane.finish_message(lease, id)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(lane.cancel_barrier(lease), Ok(id));
    lane.release(lease).unwrap_or_else(|e| panic!("{e:?}"));
    lane.disconnect();
    assert_eq!(lane.acquire(DataKind::History), Err(LaneError::Closed));
}
#[test]
fn test_should_exhaust_message_identifiers_instead_of_wrapping_into_old_data() {
    let mut lane = ConnectionDataLane::new(core::num::NonZeroU64::MIN);
    let lease = lane
        .acquire(DataKind::History)
        .unwrap_or_else(|e| panic!("{e:?}"));
    for id in 1..=u16::MAX {
        assert_eq!(lane.begin_message(lease), Ok(id));
        lane.finish_message(lease, id)
            .unwrap_or_else(|e| panic!("{e:?}"));
    }
    assert_eq!(lane.begin_message(lease), Err(LaneError::Exhausted));
}
#[test]
fn test_should_reject_stale_lease_from_a_different_connection_owner() {
    let mut old = ConnectionDataLane::new(core::num::NonZeroU64::MIN);
    let old_lease = old
        .acquire(DataKind::History)
        .unwrap_or_else(|e| panic!("{e:?}"));
    let mut current =
        ConnectionDataLane::new(core::num::NonZeroU64::new(2).unwrap_or_else(|| panic!("nonzero")));
    current
        .acquire(DataKind::History)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(current.begin_message(old_lease), Err(LaneError::Identity));
}
#[test]
fn test_should_share_one_owner_without_heap_or_panicking_on_reentrant_borrow() {
    let shared = device_protocol::SharedDataLane::new(core::num::NonZeroU64::MIN);
    let lease = shared
        .with_mut(|lane| lane.acquire(DataKind::History))
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        shared.with_mut(|lane| lane.acquire(DataKind::Raw)),
        Err(LaneError::Busy)
    );
    assert_eq!(
        shared.with_mut(|_| shared.with_mut(|lane| lane.release(lease))),
        Err(LaneError::Busy)
    );
    shared
        .with_mut(|lane| lane.release(lease))
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert!(shared.available().unwrap_or_else(|e| panic!("{e:?}")));
}
