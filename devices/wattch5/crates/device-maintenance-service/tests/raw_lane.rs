//! Real shared owner integration; no fake production allocator is supplied.
#![cfg(feature = "raw-data")]
use device_maintenance::{Error, raw::DataLane};
use device_protocol::{DataKind, SharedDataLane};
use device_maintenance_service::RawLaneAdapter;
#[test]
fn test_should_share_history_raw_stream_and_message_spaces_and_require_actual_drain() {
    let owner = SharedDataLane::new(core::num::NonZeroU64::MIN);
    let history = owner
        .with_mut(|o| o.acquire(DataKind::History))
        .unwrap_or_else(|e| panic!("{e:?}"));
    let mid = owner
        .with_mut(|o| o.begin_message(history))
        .unwrap_or_else(|e| panic!("{e:?}"));
    let mut raw = RawLaneAdapter::new(&owner);
    assert!(!raw.available());
    assert_eq!(raw.acquire(), Err(Error::Busy));
    owner
        .with_mut(|o| o.finish_message(history, mid))
        .unwrap_or_else(|e| panic!("{e:?}"));
    owner
        .with_mut(|o| o.release(history))
        .unwrap_or_else(|e| panic!("{e:?}"));
    let stream = raw.acquire().unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(stream, 2);
    let id = raw
        .reserve_message(stream)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(id, 2);
    assert_eq!(raw.release(stream), Err(Error::Busy));
    assert_eq!(raw.finish_message(stream, 1), Err(Error::Identity));
    assert_eq!(raw.reserve_message(stream), Err(Error::Busy));
    raw.finish_message(stream, id)
        .unwrap_or_else(|e| panic!("{e:?}"));
    raw.release(stream).unwrap_or_else(|e| panic!("{e:?}"));
    let next = owner
        .with_mut(|o| o.acquire(DataKind::History))
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(next.stream_id(), 3);
    assert_eq!(owner.with_mut(|o| o.begin_message(next)), Ok(3));
    assert!(!raw.owns(stream));
}
#[test]
fn test_should_encode_real_batch_metadata_using_frozen_event_crc() {
    let samples = [1, 0, 0xfe, 0xff, 3, 0];
    let batch = device_maintenance::raw::Batch {
        message_id: 1,
        capture_id: 0x1122_3344_5566_7788,
        stream_id: 4,
        batch_seq: 0,
        first_sample: 0,
        t0_mono_us: 1_000_000,
        period_ns: 10_000_000,
        config_rev: 3,
        dropped_total: 0,
        sample_count: 1,
        quality_flags: 0,
        samples: &samples,
    };
    let mut out = [0; 128];
    let n = device_maintenance_service::encode_raw_batch(&batch, &mut out, 128, 6)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(n, 52);
    assert_eq!(&out[48..52], &[0xd7, 0x59, 0xd0, 0x33]);
    let decoded =
        device_protocol::RawBatch::decode(&out[..n], 128, 6).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(decoded.samples, samples);
}
