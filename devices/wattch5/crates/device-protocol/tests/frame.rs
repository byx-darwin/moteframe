//! Protocol boundary regressions.
use device_protocol::{Frame, Header, Kind, ProtocolError};

#[test]
fn test_should_decode_literal_empty_info_request() {
    let bytes = [0x10, 0, 2, 0, 1, 0, 0, 0, 0, 0];
    let frame = Frame::decode(&bytes, 20, 512).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        frame.header,
        Header {
            kind: Kind::Request,
            module: 0,
            opcode: 2,
            message_id: 1,
            offset: 0,
            total_length: 0
        }
    );
    let mut out = [0; 20];
    assert_eq!(frame.encode(&mut out), Ok(10));
    assert_eq!(&out[..10], &bytes);
    assert_eq!(frame.encode(&mut [0; 9]), Err(ProtocolError::Capacity));
}
#[test]
fn test_should_reject_malformed_headers_and_lengths() {
    for (bytes, error) in [
        (vec![0; 9], ProtocolError::Truncated),
        (
            vec![0x20, 0, 2, 0, 1, 0, 0, 0, 0, 0],
            ProtocolError::InvalidVersion,
        ),
        (
            vec![0x13, 0, 2, 0, 1, 0, 0, 0, 0, 0],
            ProtocolError::InvalidKind,
        ),
        (
            vec![0x10, 0, 2, 1, 1, 0, 0, 0, 0, 0],
            ProtocolError::Reserved,
        ),
        (
            vec![0x10, 0, 2, 0, 0, 0, 0, 0, 0, 0],
            ProtocolError::InvalidId,
        ),
        (
            vec![0x10, 0, 2, 0, 1, 0, 0, 0, 1, 2, 7],
            ProtocolError::InvalidLength,
        ),
        (
            vec![0x10, 0, 2, 0, 1, 0, 1, 0, 1, 0, 7],
            ProtocolError::InvalidLength,
        ),
        (
            vec![0x10, 0, 2, 0, 1, 0, 0, 0, 0, 0, 7],
            ProtocolError::InvalidLength,
        ),
        (
            vec![0x10, 0, 2, 0, 1, 0, 0, 0, 20, 0, 7],
            ProtocolError::InvalidLength,
        ),
    ] {
        assert_eq!(Frame::decode(&bytes, 20, 512), Err(error), "{bytes:x?}");
    }
    assert_eq!(
        Frame::decode(&[0x10, 0, 2, 0, 1, 0, 0, 0, 1, 0], 20, 512),
        Err(ProtocolError::InvalidLength)
    );
    assert!(Frame::decode(&[0; 21], 20, 512).is_err());
    assert!(Frame::decode(&[0; 10], 10, 512).is_err());
}
#[test]
fn test_should_hide_payload_in_debug_and_use_little_endian() {
    let b = [0x12, 4, 0x80, 0, 0x34, 0x12, 0, 0, 3, 0, 0xaa, 0xbb, 0xcc];
    let f = Frame::decode(&b, 20, 512).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(f.header.message_id, 0x1234);
    assert_eq!(f.payload, &[0xaa, 0xbb, 0xcc]);
    assert!(!format!("{f:?}").contains("170"));
}
#[test]
fn test_should_reject_below_baseline_frame_maximum() {
    assert!(Frame::decode(&[0x10, 0, 2, 0, 1, 0, 0, 0, 1, 0, 7], 11, 512).is_err());
}
