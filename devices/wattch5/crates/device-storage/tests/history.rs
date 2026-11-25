//! Logical admission is distinct from peer persistence and durable ACK.
use device_storage::{DataStream, HistoryError, Snapshot};
fn snapshot(filtered: bool) -> Snapshot {
    Snapshot {
        id: 1,
        epoch: 2,
        first: 1,
        last: 3,
        filtered,
    }
}
#[test]
fn test_should_reserve_credit_once_before_first_fragment_and_cancel_drain() {
    let mut ids = device_storage::DataIds::new();
    let mut s = DataStream::new(snapshot(false), 7).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(s.begin_record(1, &mut ids), Err(HistoryError::NoCredit));
    s.grant(7, 1).unwrap_or_else(|e| panic!("{e:?}"));
    let id = s
        .begin_record(1, &mut ids)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(id, 1);
    assert_eq!(s.begin_record(2, &mut ids), Err(HistoryError::Busy));
    assert_eq!(s.cancel(), Ok(None));
    assert_eq!(s.cancel_barrier(), Err(HistoryError::Busy));
    s.finish_record(id).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(s.cancel_barrier(), Ok(id));
    assert_eq!(s.begin_record(2, &mut ids), Err(HistoryError::InvalidState));
    let mut s = DataStream::new(snapshot(false), 7).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(s.cancel(), Ok(Some(0)));
}
#[test]
fn test_should_reject_filtered_ack_and_bound_unfiltered_to_admitted_prefix() {
    let mut ids = device_storage::DataIds::new();
    let mut filtered = DataStream::new(snapshot(true), 1).unwrap_or_else(|e| panic!("{e:?}"));
    filtered.grant(1, 2).unwrap_or_else(|e| panic!("{e:?}"));
    for n in [1, 3] {
        let id = filtered
            .begin_record(n, &mut ids)
            .unwrap_or_else(|e| panic!("{e:?}"));
        filtered
            .finish_record(id)
            .unwrap_or_else(|e| panic!("{e:?}"));
    }
    assert_eq!(filtered.ack_candidate(2, 3), Err(HistoryError::FilteredAck));
    let mut s = DataStream::new(snapshot(false), 1).unwrap_or_else(|e| panic!("{e:?}"));
    s.grant(1, 2).unwrap_or_else(|e| panic!("{e:?}"));
    let id = s
        .begin_record(1, &mut ids)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(s.ack_candidate(2, 1), Err(HistoryError::Sequence));
    s.finish_record(id).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(s.ack_candidate(2, 1), Ok(1));
    assert_eq!(s.ack_candidate(2, 3), Err(HistoryError::Sequence));
    assert_eq!(s.ack_candidate(3, 1), Err(HistoryError::Epoch));
}
#[test]
fn test_should_enforce_cumulative_grants_and_id_nonreuse() {
    let mut ids = device_storage::DataIds::new();
    let mut s = DataStream::new(snapshot(false), 1).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(s.grant(2, 3), Err(HistoryError::Identity));
    s.grant(1, 2).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(s.grant(1, 1), Err(HistoryError::Sequence));
    for n in [1, 2] {
        let id = s
            .begin_record(n, &mut ids)
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(u64::from(id), n);
        s.finish_record(id).unwrap_or_else(|e| panic!("{e:?}"));
    }
    assert_eq!(s.begin_record(3, &mut ids), Err(HistoryError::NoCredit));
    s.grant(1, 3).unwrap_or_else(|e| panic!("{e:?}"));
    let id = s
        .begin_record(3, &mut ids)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(id, 3);
    assert_eq!(s.finish_record(2), Err(HistoryError::Identity));
    s.finish_record(id).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(s.begin_record(2, &mut ids), Err(HistoryError::Sequence));
}
#[test]
fn test_should_allocate_default_ids_and_share_across_stream_replacement() {
    let mut ids = device_storage::DataIds::default();
    assert_eq!(ids.reserve(), Ok(1));
    assert_eq!(ids.reserve(), Ok(2));
    let mut s = DataStream::new(snapshot(false), 7).unwrap_or_else(|e| panic!("{e:?}"));
    s.grant(7, 1).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(s.begin_record(1, &mut ids), Ok(3));
    s.finish_record(3).unwrap_or_else(|e| panic!("{e:?}"));
    s.cancel().unwrap_or_else(|e| panic!("{e:?}"));
    let mut next = DataStream::new(snapshot(false), 8).unwrap_or_else(|e| panic!("{e:?}"));
    next.grant(8, 1).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(next.begin_record(1, &mut ids), Ok(4));
}
