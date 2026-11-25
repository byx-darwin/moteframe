//! Shared property/object wire commands, independent of physical storage.
use device_protocol::{ObjectRequest, PropertyRequest};
#[test]
fn test_should_decode_and_reencode_create_and_chunk_literals() {
    let b = [
        1, 0, 7, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 1, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14,
        15, 16,
    ];
    let r = ObjectRequest::decode(0x20, &b).unwrap_or_else(|e| panic!("{e:?}"));
    let mut out = [0; 64];
    assert_eq!(r.encode(&mut out), Ok(31));
    assert_eq!(&out[..31], &b);
    let b = [1, 0, 0, 0, 0, 0, 0, 0, 3, 0, 9, 8, 7];
    let r = ObjectRequest::decode(0x21, &b).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(r.encode(&mut out), Ok(13));
    assert_eq!(&out[..13], &b);
    assert!(!format!("{r:?}").contains("9, 8, 7"));
}
#[test]
fn test_should_reject_bad_lengths_ids_tokens_and_update_create_mixups() {
    let good = [
        1, 0, 7, 0, 0, 0, 0, 0, 0, 0, 3, 0, 0, 0, 1, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14,
        15, 16,
    ];
    for n in 0..31 {
        assert!(ObjectRequest::decode(0x20, &good[..n]).is_err());
    }
    for (i, v) in [(0, 0), (2, 0), (14, 2), (6, 1)] {
        let mut b = good;
        b[i] = v;
        assert!(ObjectRequest::decode(0x20, &b).is_err());
    }
    let mut b = good;
    b[15..].fill(0);
    assert!(ObjectRequest::decode(0x20, &b).is_err());
    assert!(ObjectRequest::decode(0x21, &[1, 0, 0, 0, 0, 0, 0, 0, 2, 0, 7]).is_err());
    assert!(ObjectRequest::decode(0x22, &[0; 8]).is_err());
    assert!(ObjectRequest::decode(0x29, &[]).is_err());
}
#[test]
fn test_should_roundtrip_all_object_and_property_operations() {
    for (op, b) in [
        (0x22, vec![1, 0, 0, 0, 5, 6, 7, 8]),
        (0x23, vec![1, 0, 0, 0, 2, 0, 0, 0, 3, 0, 0, 0, 4, 0]),
        (0x24, vec![255, 255, 255, 255, 2, 0, 0, 0]),
        (0x25, vec![1, 0, 0, 0, 4]),
        (0x26, vec![1, 0, 0, 0]),
        (0x27, vec![1, 0, 0, 0]),
        (0x28, vec![1, 0, 0, 0]),
    ] {
        let r = ObjectRequest::decode(op, &b).unwrap_or_else(|e| panic!("{e:?}"));
        let mut out = [0; 64];
        let n = r.encode(&mut out).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(&out[..n], b);
    }
    for (op, b) in [
        (0x10, vec![1, 0]),
        (0x11, vec![1, 0, 2, 0, 0, 0, 2, 0, 7, 8]),
        (0x12, vec![1, 0, 3]),
    ] {
        let r = PropertyRequest::decode(op, &b).unwrap_or_else(|e| panic!("{e:?}"));
        let mut out = [0; 64];
        let n = r.encode(&mut out).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(&out[..n], b);
    }
    assert!(PropertyRequest::decode(0x11, &[1, 0, 0, 0, 0, 0, 2, 0, 7]).is_err());
    assert!(PropertyRequest::decode(0x12, &[1, 0, 0]).is_err());
}
#[test]
fn test_should_reject_object_chunks_that_exceed_logical_message_capacity() {
    let mut b = vec![1, 0, 0, 0, 0, 0, 0, 0, 0xf7, 1];
    b.extend_from_slice(&[1; 503]);
    assert!(ObjectRequest::decode(0x21, &b).is_err());
    let mut b = vec![1, 0, 1, 0, 0, 0, 0xf9, 1];
    b.extend_from_slice(&[1; 505]);
    assert!(PropertyRequest::decode(0x11, &b).is_err());
}
#[test]
fn test_should_allow_zero_enumeration_cursor_to_restart_the_complete_directory() {
    assert!(ObjectRequest::decode(0x25, &[0, 0, 0, 0, 8]).is_ok());
}
