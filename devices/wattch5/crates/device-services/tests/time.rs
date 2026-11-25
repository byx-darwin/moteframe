//! UTC mapping uses real caller-provided monotonic time; no RTC/clock frequency is simulated.
use device_services::{SoftwareClock, TimeError, UtcMapping};
#[test]
fn test_should_distinguish_unknown_utc_and_preserve_monotonic_mapping_on_resync() {
    let mut clock = SoftwareClock::new(0);
    let m = clock.read(10_000).unwrap_or_else(|e| panic!("{e:?}"));
    assert!(!m.valid);
    assert_eq!(m.utc_ms, 0);
    assert_eq!(m.mono_us, 10000);
    clock
        .synchronize(1000, 7, 10_000)
        .unwrap_or_else(|e| panic!("{e:?}"));
    let m = clock.read(11_999).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(m.utc_ms, 1001);
    assert_eq!(m.mono_us, 11999);
    clock
        .synchronize(500, 3, 12_000)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        clock
            .read(13_000)
            .unwrap_or_else(|e| panic!("{e:?}"))
            .utc_ms,
        501
    );
    assert!(clock.read(12_999).is_err());
    assert!(!format!("{clock:?}").contains("500"));
}
#[test]
fn test_should_encode_exact_mapping_and_reject_extra_bytes_or_false_unknown_time() {
    let bytes = [
        0xe8, 3, 0, 0, 0, 0, 0, 0, 0x10, 0x27, 0, 0, 0, 0, 0, 0, 7, 0, 0, 0, 1,
    ];
    let m = UtcMapping::decode(&bytes).unwrap_or_else(|e| panic!("{e:?}"));
    let mut out = [0; 21];
    assert_eq!(m.encode(&mut out), Ok(21));
    assert_eq!(out, bytes);
    let mut bad = bytes;
    bad[20] = 0;
    assert!(UtcMapping::decode(&bad).is_err());
    assert!(UtcMapping::decode(&[0; 22]).is_err());
    let mut clock = SoftwareClock::new(0);
    clock
        .synchronize(i64::MAX, 0, 0)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(clock.read(1000), Err(TimeError::Overflow));
}
#[test]
fn test_should_accumulate_adapter_drift_bound_and_refuse_uncertainty_overflow() {
    let mut c = SoftwareClock::new(100);
    c.synchronize(10_000, 7, 0)
        .unwrap_or_else(|e| panic!("{e:?}"));
    let m = c.read(1_000_000_000).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(m.utc_ms, 1_010_000);
    assert_eq!(m.uncertainty_ms, 107);
    let mut c = SoftwareClock::new(100);
    c.synchronize(0, u32::MAX, 0)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(c.read(1), Err(TimeError::Overflow));
    c.invalidate();
    assert!(!c.read(2).unwrap_or_else(|e| panic!("{e:?}")).valid);
}
