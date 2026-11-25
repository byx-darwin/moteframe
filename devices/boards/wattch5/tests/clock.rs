//! Fixed-storage monotonic conversion for the polling runtime.
#![allow(clippy::unwrap_used, reason = "Unexpected test setup failure")]
use board_syd8810::clock::CycleClock;
use core::num::NonZeroU32;
#[test]
fn test_should_extend_wrapping_downcounter_and_retain_fractional_ticks() {
    let mut c = CycleClock::new(NonZeroU32::new(64_000_000).unwrap(), 100).unwrap();
    assert_eq!(c.sample(37).unwrap(), 0);
    assert_eq!(c.sample(36).unwrap(), 1);
    assert_eq!(c.sample(0x00ff_fff0).unwrap(), 1);
    assert_eq!(c.sample(0x00ff_ffd4).unwrap(), 2);
    assert!(c.sample(0x0100_0000).is_err());
    assert_eq!(c.sample(0x00ff_ff94).unwrap(), 3);
}
#[test]
fn test_should_reject_initial_values_outside_the_hardware_counter() {
    assert!(CycleClock::new(NonZeroU32::MIN, 0x0100_0000).is_err());
}
