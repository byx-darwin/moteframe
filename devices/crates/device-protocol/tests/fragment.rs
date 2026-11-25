//! Protocol boundary regressions.
use device_protocol::{Fragmenter, Header, Kind, ProtocolError};
fn header(n: u16) -> Header {
    Header {
        kind: Kind::Response,
        module: 0,
        opcode: 1,
        message_id: 7,
        offset: 0,
        total_length: n,
    }
}
#[test]
fn test_should_retry_same_fragment_until_queue_admission() {
    let body = [0x55; 22];
    let mut f = Fragmenter::new(header(22), &body, 20).unwrap_or_else(|e| panic!("{e:?}"));
    let mut out = [0; 20];
    let mut retry = [0; 20];
    for (offset, size) in [(0, 20), (10, 20), (20, 12)] {
        assert!(!f.is_complete());
        assert_eq!(f.write_next(&mut out), Ok(size));
        assert_eq!(f.write_next(&mut retry), Ok(size));
        assert_eq!(out, retry);
        assert_eq!(out[6], offset);
        assert_eq!(out[8], 22);
        f.advance().unwrap_or_else(|e| panic!("{e:?}"));
    }
    assert!(f.is_complete());
    assert_eq!(f.write_next(&mut out), Err(ProtocolError::InvalidState));
    assert_eq!(f.advance(), Err(ProtocolError::InvalidState));
}
#[test]
fn test_should_send_empty_once_and_reject_inconsistent_input() {
    let mut f = Fragmenter::new(header(0), &[], 20).unwrap_or_else(|e| panic!("{e:?}"));
    assert!(!f.is_complete());
    assert_eq!(f.write_next(&mut [0; 10]), Ok(10));
    f.advance().unwrap_or_else(|e| panic!("{e:?}"));
    assert!(f.is_complete());
    assert!(Fragmenter::new(header(4), &[1; 3], 20).is_err());
    let mut h = header(4);
    h.offset = 1;
    assert!(Fragmenter::new(h, &[1; 4], 20).is_err());
    assert!(Fragmenter::new(header(513), &[0; 513], 20).is_err());
    let f = Fragmenter::new(header(1), &[4], 20).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(f.write_next(&mut [0; 10]), Err(ProtocolError::Capacity));
}
