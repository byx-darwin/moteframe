#![cfg(feature = "blood-pressure")]
//! Legacy cuff1..7 errors retain their meaning in their own module25 reason domain.
use device_health::{Error, Reason};

#[test]
fn test_should_map_legacy_cuff_errors_into_explicit_module25_reason_domain() {
    for raw in 1u8..=7 {
        let reason = Reason::from_legacy_cuff_error(raw).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(reason as u16, 0x0200 + u16::from(raw));
        assert!(reason.applies_to(0x25));
        for module in [0x20, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x2b] {
            assert!(!reason.applies_to(module));
        }
    }
    for raw in [0, 8, 0xff] {
        assert_eq!(
            Reason::from_legacy_cuff_error(raw),
            Err(Error::InvalidField)
        );
    }
    assert_eq!(core::mem::size_of::<Reason>(), 2);
}
