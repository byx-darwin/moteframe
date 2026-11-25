//! Protocol boundary regressions.
use device_protocol::{HelloRequest, HelloResponse, ModuleEntry, ProtocolLimits};
#[test]
fn test_should_decode_exact_hello_contract() {
    let req = [1, 1, 128, 0, 128, 0, 3, 0];
    let h = HelloRequest::decode(&req).unwrap_or_else(|e| panic!("{e:?}"));
    let mut out = [0; 8];
    h.encode(&mut out).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(out, req);
    for bad in [
        [1, 1, 128, 0, 128, 0, 1, 0],
        [2, 2, 128, 0, 128, 0, 3, 0],
        [1, 1, 127, 0, 0, 0, 3, 0],
    ] {
        assert!(HelloRequest::decode(&bad).is_err());
    }
    let resp = [1, 2, 1, 0, 0, 0, 128, 0, 128, 0, 1, 1, 1, 0, 1, 0, 0, 0];
    let h = HelloResponse::decode(&resp).unwrap_or_else(|e| panic!("{e:?}"));
    let mut out = [0; 18];
    h.encode(&mut out).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(resp, out);
    for i in [0, 1, 10, 11, 12, 13] {
        let mut b = resp;
        b[i] = 255;
        assert!(HelloResponse::decode(&b).is_err());
    }
    assert!(HelloResponse::decode(&[0; 22]).is_err());
}
#[test]
fn test_should_use_twelve_byte_module_and_fourteen_byte_limits() {
    let b = [0, 1, 2, 1, 0, 0, 0, 0, 128, 0, 128, 0];
    let m = ModuleEntry::decode(&b).unwrap_or_else(|e| panic!("{e:?}"));
    let mut out = [0; 12];
    assert_eq!(m.encode(&mut out), Ok(12));
    assert_eq!(out, b);
    let mut bad = b;
    bad[3] = 3;
    assert!(ModuleEntry::decode(&bad).is_err());
    let b = [20, 0, 20, 0, 128, 0, 128, 0, 0xd0, 7, 100, 0, 0x10, 0x27];
    let l = ProtocolLimits::decode(&b).unwrap_or_else(|e| panic!("{e:?}"));
    let mut out = [0; 14];
    l.encode(&mut out).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(out, b);
    let mut bad = b;
    bad[8] = 0;
    assert!(ProtocolLimits::decode(&bad).is_err());
    assert!(ProtocolLimits::decode(&[0; 15]).is_err());
}
#[test]
fn test_should_reject_unassigned_capability_feature_bits() {
    assert!(ModuleEntry::decode(&[0, 1, 2, 1, 1, 0, 0, 0, 128, 0, 128, 0]).is_err());
}
