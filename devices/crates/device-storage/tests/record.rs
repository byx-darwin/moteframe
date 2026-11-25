//! WREC appendix vector; CRC is independent of wire snapshot/stream context and physical layout.
use device_storage::{CommitState, RecordIntegrity, StoredRecord};
#[test]
fn test_should_match_frozen_storage_crc_and_refuse_uncommitted_or_corrupt_data() {
    let r = StoredRecord {
        domain: 0x20,
        epoch: 3,
        record_id: 4,
        record_type: 0x20,
        schema: 1,
        utc_ms: 0,
        flags: 0,
        payload: &[0x3c, 0, 0, 1],
    };
    assert_eq!(r.crc(), Ok(0xf561_e86c));
    assert_eq!(
        r.verify(CommitState::Pending, 0xf561_e86c),
        Err(RecordIntegrity::Uncommitted)
    );
    assert_eq!(r.verify(CommitState::Committed, 0xf561_e86c), Ok(()));
    assert_eq!(
        r.verify(CommitState::Committed, 0x3a16_1abd),
        Err(RecordIntegrity::Mismatch)
    );
    assert!(!format!("{r:?}").contains("60, 0, 0, 1"));
    for bit in 0..32 {
        let mut b = [0x3c, 0, 0, 1];
        b[bit / 8] ^= 1 << (bit % 8);
        let bad = StoredRecord { payload: &b, ..r };
        assert_eq!(
            bad.verify(CommitState::Committed, 0xf561_e86c),
            Err(RecordIntegrity::Mismatch)
        );
    }
    let bad = StoredRecord { utc_ms: 1, ..r };
    assert_eq!(bad.crc(), Err(RecordIntegrity::Invalid));
    let bad = StoredRecord { epoch: 0, ..r };
    assert_eq!(bad.crc(), Err(RecordIntegrity::Invalid));
}
