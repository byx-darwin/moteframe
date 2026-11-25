//! Protocol boundary regressions.
use device_protocol::{Crc32c, event_crc, object_crc};
#[test]
fn test_should_match_standard_and_domain_separated_vectors() {
    assert_eq!(Crc32c::new().finish(), 0);
    let mut c = Crc32c::new();
    c.update(b"1234");
    c.update(b"56789");
    assert_eq!(c.finish(), 0xe306_9283);
    let b = [0, 0, 255, 255, 1, 0, 0];
    assert_eq!(object_crc(4, 1, &b), Ok(0x66aa_5f61));
    assert_ne!(object_crc(5, 1, &b), object_crc(4, 1, &b));
    assert_ne!(object_crc(4, 2, &b), object_crc(4, 1, &b));
    assert!(object_crc(4, 1, &[0; 4096]).is_ok());
}
#[test]
fn test_should_detect_each_single_bit_change() {
    let bytes = [1, 2, 3, 4, 5];
    let good = event_crc(16, 128, &bytes).unwrap_or_else(|e| panic!("{e:?}"));
    for i in 0..40 {
        let mut bad = bytes;
        bad[i / 8] ^= 1 << (i % 8);
        assert_ne!(
            event_crc(16, 128, &bad).unwrap_or_else(|e| panic!("{e:?}")),
            good
        );
    }
}

#[test]
fn test_should_match_history_record_integrity_vector() {
    let body = [
        1, 0, 0, 0, 2, 0, 3, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 32, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0,
        0, 0, 4, 0, 60, 0, 0, 1,
    ];
    assert_eq!(event_crc(16, 128, &body), Ok(0x3a16_1abd));
}
