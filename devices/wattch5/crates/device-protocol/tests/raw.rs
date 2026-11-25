//! Frozen E06 synthetic raw vector, not a sampling-rate or sensor capability claim.
#![cfg(feature = "raw-data")]
use device_protocol::{ProtocolError, RawBatch};
fn vector() -> Vec<u8> {
    vec![
        0x88, 0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11, 4, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x40, 0x42,
        0x0f, 0, 0, 0, 0, 0, 0x80, 0x96, 0x98, 0, 3, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0xfe,
        0xff, 3, 0, 0xd7, 0x59, 0xd0, 0x33,
    ]
}
#[test]
fn test_should_match_full_raw_batch_crc_and_timing_fields() {
    let b = vector();
    let batch = RawBatch::decode(&b, 128, 6).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(batch.capture_id, 0x1122_3344_5566_7788);
    assert_eq!(batch.t0_mono_us, 1_000_000);
    assert_eq!(batch.period_ns, 10_000_000);
    assert_eq!(batch.samples, &[1, 0, 0xfe, 0xff, 3, 0]);
    let mut out = [0; 128];
    let n = batch
        .encode(&mut out, 128, 6)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(n, 52);
    assert_eq!(&out[..n], b);
    assert!(!format!("{batch:?}").contains("254, 255"));
}
#[test]
fn test_should_reject_wrong_stride_count_flags_crc_and_coding_limits() {
    let b = vector();
    assert!(RawBatch::decode(&b, 128, 0).is_err());
    assert!(RawBatch::decode(&b, 128, 4).is_err());
    assert!(RawBatch::decode(&b, 51, 6).is_err());
    for bit in 0..b.len() * 8 {
        let mut bad = b.clone();
        bad[bit / 8] ^= 1 << (bit % 8);
        assert_eq!(
            RawBatch::decode(&bad, 128, 6),
            Err(ProtocolError::Integrity)
        );
    }
    let mut batch = RawBatch::decode(&b, 128, 6).unwrap_or_else(|e| panic!("{e:?}"));
    batch.quality_flags = 8;
    assert!(batch.encode(&mut [0; 128], 128, 6).is_err());
    batch.quality_flags = 0;
    batch.sample_count = 2;
    assert!(batch.encode(&mut [0; 128], 128, 6).is_err());
}
