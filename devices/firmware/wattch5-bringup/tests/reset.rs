//! Actual forwarding boundary: failed reset never grants UI ownership; late reset never reaches radio.
#![allow(clippy::unwrap_used)]
use ble_controller::{Advertisement, LinkBackend, LinkEvent, LinkFrame, TxAdmission};
use core::cell::Cell;
use wattch5_bringup::{ObservedReset, ResetGate};
#[derive(Debug)]
struct Backend<'a> {
    calls: &'a Cell<u32>,
    fail: &'a Cell<bool>,
}
impl LinkBackend for Backend<'_> {
    type Error = ();
    fn reset(&mut self) -> Result<(), ()> {
        self.calls.set(self.calls.get() + 1);
        if self.fail.get() { Err(()) } else { Ok(()) }
    }
    fn advertise(&mut self, _: &Advertisement) -> Result<(), ()> {
        Ok(())
    }
    fn stop(&mut self) -> Result<(), ()> {
        Ok(())
    }
    fn poll(&mut self) -> Result<Option<LinkEvent>, ()> {
        Ok(None)
    }
    fn transmit(&mut self, _: u32, _: LinkFrame) -> Result<TxAdmission, ()> {
        Ok(TxAdmission::Busy)
    }
    fn connection_update(&mut self, _: &[u8]) -> Result<(), ()> {
        Ok(())
    }
    fn channel_map(&mut self, _: &[u8]) -> Result<(), ()> {
        Ok(())
    }
}
#[test]
fn test_should_observe_actual_reset_and_reject_later_reset_before_backend_access() {
    let calls = Cell::new(0);
    let fail = Cell::new(true);
    let gate = ResetGate::new();
    let mut observed = ObservedReset::new(
        Backend {
            calls: &calls,
            fail: &fail,
        },
        &gate,
    );
    assert!(observed.reset().is_err());
    assert!(!gate.reset_completed());
    assert!(gate.arm_ui().is_err());
    fail.set(false);
    observed.reset().unwrap();
    assert!(gate.reset_completed());
    gate.begin().unwrap();
    assert!(!gate.reset_completed());
    observed.reset().unwrap();
    assert!(gate.reset_completed());
    gate.arm_ui().unwrap();
    let before = calls.get();
    assert!(observed.reset().is_err());
    assert_eq!(calls.get(), before);
    assert!(!gate.ui_allowed());
    assert!(observed.reset().is_err());
    assert_eq!(calls.get(), before);
}
